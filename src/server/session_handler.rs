use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use futures_util::{SinkExt, StreamExt};
use indoc::concatdoc;
use itertools::Itertools;
use sqlx::{MySqlConnection, MySqlPool};
use tokio::net::UnixStream;
use tracing::Instrument;

use crate::{
    core::{
        common::UnixUser,
        protocol::{
            PasswordSource, Request, Response, ServerToClientMessageStream, SetPasswordError,
            create_server_to_client_message_stream, request_validation::GroupDenylist,
        },
    },
    server::{
        authorization::check_authorization,
        common::get_user_filtered_groups,
        sql::{
            database_operations::{
                complete_database_name, create_databases, drop_databases,
                list_all_databases_for_user, list_databases,
            },
            database_privilege_operations::{
                apply_privilege_diffs, get_all_database_privileges, get_databases_privilege_data,
            },
            user_operations::{
                complete_user_name, create_database_users, drop_database_users,
                list_all_database_users_for_unix_user, list_database_users, lock_database_users,
                set_password_for_database_user, unlock_database_users,
            },
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new(id: u64) -> Self {
        SessionId(id)
    }

    pub fn inner(&self) -> u64 {
        self.0
    }
}

// TODO: don't use database connection unless necessary.

pub async fn session_handler(
    socket: UnixStream,
    session_id: SessionId,
    db_pool: Arc<MySqlPool>,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
    request_counter: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let uid = match socket.peer_cred() {
        Ok(cred) => cred.uid(),
        Err(e) => {
            tracing::error!("Failed to get peer credentials from socket: {}", e);
            let mut message_stream = create_server_to_client_message_stream(socket);
            message_stream
                .send(Response::Error(
                    (concatdoc! {
                        "Server failed to get peer credentials from socket\n",
                        "Please check the server logs or contact the system administrators"
                    })
                    .to_string(),
                ))
                .await
                .ok();
            anyhow::bail!("Failed to get peer credentials from socket");
        }
    };

    tracing::trace!("Validated peer UID: {}", uid);

    let unix_user = match UnixUser::from_uid(uid) {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("Failed to get username from uid: {}", e);
            let mut message_stream = create_server_to_client_message_stream(socket);
            message_stream
                .send(Response::Error(
                    (concatdoc! {
                        "Server failed to get user data from the system\n",
                        "Please check the server logs or contact the system administrators"
                    })
                    .to_string(),
                ))
                .await
                .ok();
            anyhow::bail!("Failed to get username from uid: {e}");
        }
    };

    let span = tracing::info_span!(
        "user_session",
        session_id = session_id.inner(),
        user = %unix_user,
    );

    (async move {
        tracing::debug!("Accepted connection from user: {}", unix_user);

        let result = session_handler_with_unix_user(
            socket,
            session_id,
            &unix_user,
            db_pool,
            db_is_mariadb,
            group_denylist,
            request_counter,
        )
        .await;

        tracing::debug!(
            "Finished handling requests for connection from user: {}",
            unix_user,
        );

        result
    })
    .instrument(span)
    .await
}

pub async fn session_handler_with_unix_user(
    socket: UnixStream,
    session_id: SessionId,
    unix_user: &UnixUser,
    db_pool: Arc<MySqlPool>,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
    request_counter: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let mut message_stream = create_server_to_client_message_stream(socket);

    tracing::trace!("Requesting database connection from pool");
    let mut db_connection = match db_pool.acquire().await {
        Ok(connection) => connection,
        Err(err) => {
            message_stream
                .send(Response::Error(
                    (concatdoc! {
                        "Server failed to connect to database\n",
                        "Please check the server logs or contact the system administrators"
                    })
                    .to_string(),
                ))
                .await?;
            message_stream.flush().await?;
            return Err(err.into());
        }
    };
    tracing::trace!("Successfully acquired database connection from pool");

    let result = session_handler_with_db_connection(
        message_stream,
        session_id,
        unix_user,
        &mut db_connection,
        db_is_mariadb,
        group_denylist,
        request_counter,
    )
    .await;

    tracing::trace!("Releasing database connection back to pool");

    result
}

// TODO: ensure proper db_connection hygiene for functions that invoke
//       this function

async fn session_handler_with_db_connection(
    mut stream: ServerToClientMessageStream,
    session_id: SessionId,
    unix_user: &UnixUser,
    db_connection: &mut MySqlConnection,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
    request_counter: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    stream.send(Response::Ready).await?;
    loop {
        // TODO: better error handling
        // TODO: timeout for receiving requests
        // TODO: cancel on request by supervisor
        let request = match stream.next().await {
            Some(Ok(request)) => request,
            Some(Err(e)) => return Err(e.into()),
            None => {
                tracing::warn!("Client disconnected without sending an exit message");
                break;
            }
        };

        request_counter.fetch_add(1, Ordering::Relaxed);

        let request_span = tracing::info_span!("request", command = request.command_name());

        if !handle_request(
            request,
            session_id,
            unix_user,
            db_connection,
            db_is_mariadb,
            group_denylist,
            &mut stream,
        )
        .instrument(request_span)
        .await?
        {
            break;
        }
    }

    Ok(())
}

/// Handle a single request from a client.
///
/// If the function returns `true`, the session should continue.
async fn handle_request(
    request: Request,
    session_id: SessionId,
    unix_user: &UnixUser,
    db_connection: &mut MySqlConnection,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
    stream: &mut ServerToClientMessageStream,
) -> anyhow::Result<bool> {
    match &request {
        Request::Exit => tracing::debug!("Request: exit"),
        Request::PasswdUser((db_user, password)) => tracing::debug!(
            "Request:\n{}",
            serde_json::to_string_pretty(&Request::PasswdUser((
                db_user.to_owned(),
                match password {
                    PasswordSource::Explicit(_) =>
                        PasswordSource::Explicit("<REDACTED>".to_string()),
                    PasswordSource::Generate => PasswordSource::Generate,
                    PasswordSource::Clear => PasswordSource::Clear,
                }
            )))?
        ),
        request => tracing::debug!("Request:\n{}", serde_json::to_string_pretty(request)?),
    }

    let affected_dbs = request.affected_databases();
    if !affected_dbs.is_empty() {
        tracing::trace!(
            "Affected databases: {}",
            affected_dbs.into_iter().map(|db| db.to_string()).join(", ")
        );
    }

    let affected_users = request.affected_users();
    if !affected_users.is_empty() {
        tracing::trace!(
            "Affected users: {}",
            affected_users.into_iter().map(|u| u.to_string()).join(", "),
        );
    }

    let response = match request {
        Request::CheckAuthorization(ref dbs_or_users) => {
            let result = check_authorization(dbs_or_users, unix_user, group_denylist).await;
            Response::CheckAuthorization(result)
        }
        Request::ListValidNamePrefixes => {
            let mut result = Vec::with_capacity(unix_user.groups.len() + 1);
            result.push(unix_user.username.clone());

            for group in get_user_filtered_groups(unix_user, group_denylist) {
                result.push(group.clone());
            }

            Response::ListValidNamePrefixes(result)
        }
        Request::CompleteDatabaseName(ref partial_database_name) => {
            // TODO: more correct validation here
            if partial_database_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                let result = complete_database_name(
                    partial_database_name,
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                )
                .await;
                Response::CompleteDatabaseName(result)
            } else {
                Response::CompleteDatabaseName(vec![])
            }
        }
        Request::CompleteUserName(ref partial_user_name) => {
            // TODO: more correct validation here
            if partial_user_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                let result = complete_user_name(
                    partial_user_name,
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                )
                .await;
                Response::CompleteUserName(result)
            } else {
                Response::CompleteUserName(vec![])
            }
        }
        Request::CreateDatabases(ref databases_names) => {
            let result = create_databases(
                databases_names,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::CreateDatabases(result)
        }
        Request::DropDatabases(ref databases_names) => {
            let result = drop_databases(
                databases_names,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::DropDatabases(result)
        }
        Request::ListDatabases(ref request) => {
            if let Some(database_names) = &request.names {
                let result = list_databases(
                    database_names,
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                    request.include_all_tables_and_users,
                )
                .await;
                Response::ListDatabases(result)
            } else {
                let result = list_all_databases_for_user(
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                    request.include_all_tables_and_users,
                )
                .await;
                Response::ListAllDatabases(result)
            }
        }
        Request::ListPrivileges(ref database_names) => {
            if let Some(database_names) = database_names {
                let privilege_data = get_databases_privilege_data(
                    database_names,
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                )
                .await;
                Response::ListPrivileges(privilege_data)
            } else {
                let privilege_data = get_all_database_privileges(
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                )
                .await;
                Response::ListAllPrivileges(privilege_data)
            }
        }
        Request::ModifyPrivileges(ref database_privilege_diffs) => {
            let result = apply_privilege_diffs(
                database_privilege_diffs,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::ModifyPrivileges(result)
        }
        Request::CreateUsers(ref db_users) => {
            let result = create_database_users(
                db_users,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::CreateUsers(result)
        }
        Request::DropUsers(ref db_users) => {
            let result = drop_database_users(
                db_users,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::DropUsers(result)
        }
        Request::PasswdUser((ref db_user, ref password)) => {
            let result = set_password_for_database_user(
                db_user,
                password,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::SetUserPassword(result)
        }
        Request::ListUsers(ref request) => {
            if let Some(db_users) = &request.names {
                let result = list_database_users(
                    db_users,
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                    request.include_all_databases,
                )
                .await;
                Response::ListUsers(result)
            } else {
                let result = list_all_database_users_for_unix_user(
                    unix_user,
                    db_connection,
                    db_is_mariadb,
                    group_denylist,
                    request.include_all_databases,
                )
                .await;
                Response::ListAllUsers(result)
            }
        }
        Request::LockUsers(ref db_users) => {
            let result = lock_database_users(
                db_users,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::LockUsers(result)
        }
        Request::UnlockUsers(ref db_users) => {
            let result = unlock_database_users(
                db_users,
                unix_user,
                db_connection,
                db_is_mariadb,
                group_denylist,
            )
            .await;
            Response::UnlockUsers(result)
        }
        Request::Exit => {
            return Ok(false);
        }
    };

    let response_to_display = match &response {
        Response::SetUserPassword(Err(SetPasswordError::MySqlError(_))) => {
            &Response::SetUserPassword(Err(SetPasswordError::MySqlError("<REDACTED>".to_string())))
        }
        Response::SetUserPassword(Ok(Some(_))) => {
            &Response::SetUserPassword(Ok(Some("<REDACTED>".to_string())))
        }
        response => response,
    };
    tracing::debug!(
        "Response:\n{}",
        serde_json::to_string_pretty(&response_to_display)?
    );

    log_request(session_id, unix_user, &request, &response);

    stream.send(response).await?;
    stream.flush().await?;
    tracing::trace!("Successfully processed request");

    Ok(true)
}

/// Log a summary of the request and its result.
fn log_request(
    session_id: SessionId,
    unix_user: &UnixUser,
    request: &Request,
    response: &Response,
) {
    tracing::info!(
        "[{}|session:{}|user:{unix_user}] {}",
        response.ok_status(),
        session_id.inner(),
        request.log_summary(),
    );
}
