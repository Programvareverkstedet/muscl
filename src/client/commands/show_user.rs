use clap::Parser;
use clap_complete::ArgValueCompleter;
use futures_util::SinkExt;
use tokio_stream::StreamExt;

use crate::{
    client::commands::{erroneous_server_response, print_authorization_owner_hint},
    core::{
        completion::mysql_user_completer,
        protocol::{
            ClientToServerMessageStream, ListUsersError, ListUsersRequest, Request, Response,
            print_list_users_output_status, print_list_users_output_status_json,
            request_validation::ValidationError,
        },
        types::MySQLUser,
    },
};

#[derive(Parser, Debug, Clone)]
pub struct ShowUserArgs {
    /// The `MySQL` user(s) to show
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_user_completer)))]
    #[arg(num_args = 0.., value_name = "USER_NAME")]
    username: Vec<MySQLUser>,

    /// Print the information as JSON
    #[arg(short, long)]
    json: bool,

    /// Show all databases each user has privileges on
    #[arg(short = 'a', long)]
    all: bool,
}

pub async fn show_users(
    args: ShowUserArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let message = Request::ListUsers(ListUsersRequest::new(
        if args.username.is_empty() {
            None
        } else {
            Some(args.username.clone())
        },
        args.all || args.json,
    ));

    if let Err(err) = server_connection.send(message).await {
        server_connection.close().await.ok();
        anyhow::bail!(err);
    }

    let users = match server_connection.next().await {
        Some(Ok(Response::ListUsers(users))) => users,
        Some(Ok(Response::ListAllUsers(users))) => match users {
            Ok(users) => users
                .into_iter()
                .map(|user| (user.user.clone(), Ok(user)))
                .collect(),
            Err(err) => {
                server_connection.send(Request::Exit).await?;
                return Err(
                    anyhow::anyhow!(err.to_error_message()).context("Failed to list all users")
                );
            }
        },
        response => return erroneous_server_response(response),
    };

    if args.json {
        print_list_users_output_status_json(&users);
    } else {
        print_list_users_output_status(&users);

        if users.iter().any(|(_, res)| {
            matches!(
                res,
                Err(ListUsersError::ValidationError(
                    ValidationError::AuthorizationError(_)
                ))
            )
        }) {
            print_authorization_owner_hint(&mut server_connection).await?;
        }
    }

    server_connection.send(Request::Exit).await?;

    if users.values().any(std::result::Result::is_err) {
        std::process::exit(1);
    }

    Ok(())
}
