use clap::Parser;
use clap_complete::ArgValueCompleter;
use futures_util::SinkExt;
use tokio_stream::StreamExt;

use crate::{
    client::commands::{erroneous_server_response, print_authorization_owner_hint},
    core::{
        completion::mysql_database_completer,
        protocol::{
            ClientToServerMessageStream, ListDatabasesError, ListDatabasesRequest, Request,
            Response, print_list_databases_output_status, print_list_databases_output_status_json,
            request_validation::ValidationError,
        },
        types::MySQLDatabase,
    },
};

#[derive(Parser, Debug, Clone)]
pub struct ShowDbArgs {
    /// The `MySQL` database(s) to show
    #[arg(num_args = 0.., value_name = "DB_NAME")]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_database_completer)))]
    name: Vec<MySQLDatabase>,

    /// Print the information as JSON
    #[arg(short, long)]
    json: bool,

    /// Show all tables and users for each database
    #[arg(short = 'a', long)]
    all: bool,

    /// Show sizes in bytes instead of human-readable format
    #[arg(short, long)]
    bytes: bool,
}

pub async fn show_databases(
    args: ShowDbArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let message = Request::ListDatabases(ListDatabasesRequest::new(
        if args.name.is_empty() {
            None
        } else {
            Some(args.name.clone())
        },
        args.all || args.json,
    ));

    server_connection.send(message).await?;

    let databases = match server_connection.next().await {
        Some(Ok(Response::ListDatabases(databases))) => databases,
        Some(Ok(Response::ListAllDatabases(database_list))) => match database_list {
            Ok(list) => list
                .into_iter()
                .map(|db| (db.database.clone(), Ok(db)))
                .collect(),
            Err(err) => {
                server_connection.send(Request::Exit).await?;
                return Err(
                    anyhow::anyhow!(err.to_error_message()).context("Failed to list databases")
                );
            }
        },
        response => return erroneous_server_response(response),
    };

    if args.json {
        print_list_databases_output_status_json(&databases);
    } else {
        print_list_databases_output_status(&databases, args.bytes);

        if databases.iter().any(|(_, res)| {
            matches!(
                res,
                Err(ListDatabasesError::ValidationError(
                    ValidationError::AuthorizationError(_)
                ))
            )
        }) {
            print_authorization_owner_hint(&mut server_connection).await?;
        }
    }

    server_connection.send(Request::Exit).await?;

    if databases.values().any(std::result::Result::is_err) {
        std::process::exit(1);
    }

    Ok(())
}
