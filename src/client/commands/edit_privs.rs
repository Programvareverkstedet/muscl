use std::{
    collections::{BTreeMap, BTreeSet},
    io::IsTerminal,
};

use anyhow::Context;
use clap::{Args, Parser};
use clap_complete::ArgValueCompleter;
use dialoguer::{Confirm, Editor};
use futures_util::SinkExt;
use itertools::Itertools;
use nix::unistd::{User, getuid};
use tokio_stream::StreamExt;

use crate::{
    client::commands::{erroneous_server_response, print_authorization_owner_hint},
    core::{
        completion::{mysql_database_completer, mysql_user_completer},
        database_privileges::{
            DatabasePrivilegeEdit, DatabasePrivilegeEditEntry, DatabasePrivilegeRow,
            DatabasePrivilegeRowDiff, DatabasePrivilegesDiff, create_or_modify_privilege_rows,
            diff_privileges, display_privilege_diffs, generate_editor_content_from_privilege_data,
            parse_privilege_data_from_editor_content, reduce_privilege_diffs,
        },
        protocol::{
            ClientToServerMessageStream, ListDatabasesError, ListDatabasesRequest, ListUsersError,
            ListUsersRequest, ModifyDatabasePrivilegesError, Request, Response,
            print_modify_database_privileges_output_status, request_validation::ValidationError,
        },
        types::{MySQLDatabase, MySQLUser},
    },
};

#[derive(Parser, Debug, Clone)]
pub struct EditPrivsArgs {
    /// The privileges to set, grant or revoke, in the format `DATABASE:USER:[+-]PRIVILEGES`
    ///
    /// This option allows for changing privileges for multiple databases and users in batch.
    ///
    /// This can not be used together with the positional `DB_NAME`, `USER_NAME` and `PRIVILEGES` arguments.
    #[arg(
      short,
      long,
      value_name = "DB_NAME:USER_NAME:[+-]PRIVILEGES",
      num_args = 0..,
      value_parser = DatabasePrivilegeEditEntry::parse_from_str,
      conflicts_with("single_priv"),
    )]
    pub privs: Vec<DatabasePrivilegeEditEntry>,

    #[command(flatten)]
    pub single_priv: Option<SinglePrivilegeEditArgs>,

    /// Print the information as JSON
    #[arg(short, long)]
    pub json: bool,

    /// Specify the text editor to use for editing privileges
    #[arg(
      short,
      long,
      value_name = "COMMAND",
      value_hint = clap::ValueHint::CommandString,
    )]
    pub editor: Option<String>,

    /// Disable interactive confirmation before saving changes
    #[arg(short, long)]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct SinglePrivilegeEditArgs {
    /// The `MySQL` database to edit privileges for
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_database_completer)))]
    #[arg(
        value_name = "DB_NAME",
        requires = "user_name",
        requires = "single_priv"
    )]
    pub db_name: Option<MySQLDatabase>,

    /// The `MySQL` database to edit privileges for
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_user_completer)))]
    #[arg(value_name = "USER_NAME")]
    pub user_name: Option<MySQLUser>,

    /// The privileges to set, grant or revoke
    #[arg(
      allow_hyphen_values = true,
      value_name = "[+-]PRIVILEGES",
      value_parser = DatabasePrivilegeEdit::parse_from_str,
    )]
    pub single_priv: Option<DatabasePrivilegeEdit>,
}

async fn users_exist(
    server_connection: &mut ClientToServerMessageStream,
    privilege_diff: &BTreeSet<DatabasePrivilegesDiff>,
) -> anyhow::Result<BTreeMap<MySQLUser, Result<(), ListUsersError>>> {
    let user_list = privilege_diff
        .iter()
        .map(|diff| diff.get_user_name().clone())
        .collect();

    let message = Request::ListUsers(ListUsersRequest::new(Some(user_list), false));
    server_connection.send(message).await?;

    let result = match server_connection.next().await {
        Some(Ok(Response::ListUsers(user_map))) => user_map,
        response => {
            erroneous_server_response(response)?;
            // Unreachable, but needed to satisfy the type checker
            BTreeMap::new()
        }
    };

    let result = result
        .into_iter()
        .map(|(user, user_result)| (user, user_result.map(|_| ())))
        .collect();

    Ok(result)
}

async fn databases_exist(
    server_connection: &mut ClientToServerMessageStream,
    privilege_diff: &BTreeSet<DatabasePrivilegesDiff>,
) -> anyhow::Result<BTreeMap<MySQLDatabase, Result<(), ListDatabasesError>>> {
    let database_list = privilege_diff
        .iter()
        .map(|diff| diff.get_database_name().clone())
        .collect();

    let message = Request::ListDatabases(ListDatabasesRequest::new(Some(database_list), false));
    server_connection.send(message).await?;

    let result = match server_connection.next().await {
        Some(Ok(Response::ListDatabases(database_map))) => database_map,
        response => {
            erroneous_server_response(response)?;
            // Unreachable, but needed to satisfy the type checker
            BTreeMap::new()
        }
    };

    let result = result
        .into_iter()
        .map(|(database, db_result)| (database, db_result.map(|_| ())))
        .collect();

    Ok(result)
}

// TODO: reduce the complexity of this function
pub async fn edit_database_privileges(
    args: EditPrivsArgs,
    // NOTE: this is only used for backwards compat with mysql-admutils
    use_database: Option<MySQLDatabase>,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let message = Request::ListPrivileges(use_database.clone().map(|db| vec![db]));

    server_connection.send(message).await?;

    debug_assert!(args.privs.is_empty() ^ args.single_priv.is_none());

    let privs = if let Some(single_priv_entry) = &args.single_priv {
        let database = single_priv_entry.db_name.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "DB_NAME must be specified when editing privileges in single privilege mode"
            )
        })?;
        let user = single_priv_entry.user_name.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "USER_NAME must be specified when DB_NAME is specified in single privilege mode"
            )
        })?;
        let privilege_edit = single_priv_entry.single_priv.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "PRIVILEGES must be specified when DB_NAME is specified in single privilege mode"
            )
        })?;

        vec![DatabasePrivilegeEditEntry {
            database,
            user,
            privilege_edit,
        }]
    } else {
        args.privs.clone()
    };

    let existing_privilege_rows = match server_connection.next().await {
        Some(Ok(Response::ListPrivileges(databases))) => databases
            .into_iter()
            .filter_map(|(database_name, result)| match result {
                Ok(privileges) => Some(privileges),
                Err(err) => {
                    eprintln!("{}", err.to_error_message(&database_name));
                    eprintln!("Skipping...");
                    println!();
                    None
                }
            })
            .flatten()
            .sorted_by_key(|row| (row.db.clone(), row.user.clone()))
            .collect::<Vec<_>>(),
        Some(Ok(Response::ListAllPrivileges(privilege_rows))) => match privilege_rows {
            Ok(list) => list
                .into_iter()
                .sorted_by_key(|row| (row.db.clone(), row.user.clone()))
                .collect(),
            Err(err) => {
                server_connection.send(Request::Exit).await?;
                return Err(anyhow::anyhow!(err.to_error_message())
                    .context("Failed to list database privileges"));
            }
        },
        response => return erroneous_server_response(response),
    };

    let diffs: BTreeSet<DatabasePrivilegesDiff> = if privs.is_empty() {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "Cannot launch editor in non-interactive mode. Please provide privileges via command line arguments."
            );
        }
        let privileges_to_change =
            edit_privileges_with_editor(&existing_privilege_rows, use_database.as_ref())?;
        diff_privileges(&existing_privilege_rows, &privileges_to_change)
    } else {
        let privileges_to_change = parse_privilege_tables(&privs)?;
        create_or_modify_privilege_rows(&existing_privilege_rows, &privileges_to_change)?
    };

    let database_existence_map = databases_exist(&mut server_connection, &diffs).await?;
    let user_existence_map = users_exist(&mut server_connection, &diffs).await?;

    let diffs = reduce_privilege_diffs(&existing_privilege_rows, diffs)?
        .into_iter()
        .filter(|diff| {
            let database_name = diff.get_database_name();
            let username = diff.get_user_name();

            if let Some(Err(err)) = database_existence_map.get(database_name) {
                println!("{}", err.to_error_message(database_name));
                println!("Skipping...");
                return false;
            }

            if let Some(Err(err)) = user_existence_map.get(username) {
                println!("{}", err.to_error_message(username));
                println!("Skipping...");
                return false;
            }

            true
        })
        .collect::<BTreeSet<_>>();

    if database_existence_map.values().any(|res| {
        matches!(
            res,
            Err(ListDatabasesError::ValidationError(
                ValidationError::AuthorizationError(_)
            ))
        )
    }) || user_existence_map.values().any(|res| {
        matches!(
            res,
            Err(ListUsersError::ValidationError(
                ValidationError::AuthorizationError(_)
            ))
        )
    }) {
        println!();
        print_authorization_owner_hint(&mut server_connection).await?;
        println!();
    }

    if diffs.is_empty() {
        println!("No changes to make.");
        server_connection.send(Request::Exit).await?;
        return Ok(());
    }

    println!("The following changes will be made:\n");
    println!("{}", display_privilege_diffs(&diffs));

    if std::io::stdin().is_terminal()
        && !args.yes
        && !Confirm::new()
            .with_prompt("Do you want to apply these changes?")
            .default(false)
            .show_default(true)
            .interact()?
    {
        server_connection.send(Request::Exit).await?;
        return Ok(());
    }

    let message = Request::ModifyPrivileges(diffs);
    server_connection.send(message).await?;

    let result = match server_connection.next().await {
        Some(Ok(Response::ModifyPrivileges(result))) => result,
        response => return erroneous_server_response(response),
    };

    print_modify_database_privileges_output_status(&result);

    if result.values().flatten().any(|(_, res)| {
        matches!(
            res,
            Err(ModifyDatabasePrivilegesError::UserValidationError(
                ValidationError::AuthorizationError(_)
            ) | ModifyDatabasePrivilegesError::DatabaseValidationError(
                ValidationError::AuthorizationError(_)
            ))
        )
    }) {
        print_authorization_owner_hint(&mut server_connection).await?;
    }

    server_connection.send(Request::Exit).await?;

    if result.values().flatten().any(|(_, res)| res.is_err()) {
        std::process::exit(1);
    }

    Ok(())
}

fn parse_privilege_tables(
    privs: &[DatabasePrivilegeEditEntry],
) -> anyhow::Result<BTreeSet<DatabasePrivilegeRowDiff>> {
    debug_assert!(!privs.is_empty());
    privs
        .iter()
        .map(|priv_edit_entry| {
            priv_edit_entry
                .as_database_privileges_diff()
                .context(format!(
                    "Failed parsing database privileges: `{priv_edit_entry}`"
                ))
        })
        .collect::<anyhow::Result<BTreeSet<DatabasePrivilegeRowDiff>>>()
}

fn edit_privileges_with_editor(
    privilege_data: &[DatabasePrivilegeRow],
    // NOTE: this is only used for backwards compat with mysql-admtools
    database_name: Option<&MySQLDatabase>,
) -> anyhow::Result<Vec<DatabasePrivilegeRow>> {
    let unix_user = User::from_uid(getuid())
        .context("Failed to look up your UNIX username")
        .and_then(|u| u.ok_or(anyhow::anyhow!("Failed to look up your UNIX username")))?;

    let editor_content =
        generate_editor_content_from_privilege_data(privilege_data, &unix_user.name, database_name);

    // TODO: handle errors better here
    let result = Editor::new().extension("tsv").edit(&editor_content)?;

    match result {
        None => Ok(privilege_data.to_vec()),
        Some(result) => parse_privilege_data_from_editor_content(&result)
            .context("Could not parse privilege data from editor"),
    }
}
