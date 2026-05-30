use clap::{Parser, Subcommand};
use clap_complete::ArgValueCompleter;
use clap_verbosity_flag::Verbosity;
use futures_util::{SinkExt, StreamExt};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::PathBuf;
use tokio::net::UnixStream as TokioUnixStream;

use crate::{
    client::{
        commands::{EditPrivsArgs, edit_database_privileges, erroneous_server_response},
        mysql_admutils_compatibility::{
            common::trim_db_name_to_32_chars,
            error_messages::{
                format_show_database_error_message, handle_create_database_error,
                handle_drop_database_error,
            },
        },
    },
    core::{
        bootstrap::bootstrap_server_connection_and_drop_privileges,
        completion::{mysql_database_completer, prefix_completer},
        database_privileges::DatabasePrivilegeRow,
        protocol::{
            ClientToServerMessageStream, ListDatabasesRequest, ListPrivilegesError, Request,
            Response, create_client_to_server_message_stream,
        },
        types::MySQLDatabase,
    },
};

const HELP_DB_PERM: &str = r"
Edit permissions for the DATABASE(s). Running this command will
spawn the editor stored in the $EDITOR environment variable.
(pico will be used if the variable is unset)

The file should contain one line per user, starting with the
username and followed by ten Y/N-values separated by whitespace.
Lines starting with # are ignored.

The Y/N-values corresponds to the following mysql privileges:
  Select     - Enables use of SELECT
  Insert     - Enables use of INSERT
  Update     - Enables use of UPDATE
  Delete     - Enables use of DELETE
  Create     - Enables use of CREATE TABLE
  Drop       - Enables use of DROP TABLE
  Alter      - Enables use of ALTER TABLE
  Index      - Enables use of CREATE INDEX and DROP INDEX
  Temp       - Enables use of CREATE TEMPORARY TABLE
  Lock       - Enables use of LOCK TABLE
  References - Enables use of REFERENCES
";

/// Create, drop or edit permissions for the DATABASE(s),
/// as determined by the COMMAND.
///
/// This is a compatibility layer for the 'mysql-dbadm' command.
/// Please consider using the newer 'muscl' command instead.
#[derive(Parser)]
#[command(
    bin_name = "mysql-dbadm",
    version,
    about,
    disable_help_subcommand = true,
    verbatim_doc_comment
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Path to the socket of the server, if it already exists.
    #[arg(
        short,
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
        global = true,
        hide_short_help = true
    )]
    server_socket_path: Option<PathBuf>,

    /// Config file to use for the server.
    #[arg(
        short,
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
        global = true,
        hide_short_help = true
    )]
    config: Option<PathBuf>,

    /// Print help for the 'editperm' subcommand.
    #[arg(long, global = true)]
    pub help_editperm: bool,
}

// NOTE: mysql-dbadm explicitly calls privileges "permissions".
//       This is something we're trying to move away from.
//       See https://git.pvv.ntnu.no/Projects/muscl/issues/29
#[derive(Subcommand)]
pub enum Command {
    /// create the DATABASE(s).
    Create(CreateArgs),

    /// delete the DATABASE(s).
    Drop(DatabaseDropArgs),

    /// give information about the DATABASE(s), or, if
    /// none are given, all the ones you own.
    Show(DatabaseShowArgs),

    // TODO: make this output more verbatim_doc_comment-like,
    //       without messing up the indentation.
    /// change permissions for the DATABASE(s). Your
    /// favorite editor will be started, allowing you
    /// to make changes to the permission table.
    /// Run 'mysql-dbadm --help-editperm' for more
    /// information.
    Editperm(EditPermArgs),
}

#[derive(Parser)]
pub struct CreateArgs {
    /// The name of the DATABASE(s) to create.
    #[arg(num_args = 1..)]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(prefix_completer)))]
    name: Vec<MySQLDatabase>,
}

#[derive(Parser)]
pub struct DatabaseDropArgs {
    /// The name of the DATABASE(s) to drop.
    #[arg(num_args = 1..)]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_database_completer)))]
    name: Vec<MySQLDatabase>,
}

#[derive(Parser)]
pub struct DatabaseShowArgs {
    /// The name of the DATABASE(s) to show.
    #[arg(num_args = 0..)]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_database_completer)))]
    name: Vec<MySQLDatabase>,
}

#[derive(Parser)]
pub struct EditPermArgs {
    /// The name of the DATABASE to edit permissions for.
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_database_completer)))]
    pub database: MySQLDatabase,
}

/// **WARNING:** This function may be run with elevated privileges.
pub fn main() -> anyhow::Result<()> {
    let args: Args = Args::parse();

    if args.help_editperm {
        println!("{HELP_DB_PERM}");
        return Ok(());
    }

    let server_connection = bootstrap_server_connection_and_drop_privileges(
        args.server_socket_path,
        args.config,
        Verbosity::default(),
    )?;

    let Some(command) = args.command else {
        println!(
            "Try `{} --help' for more information.",
            std::env::args().next().unwrap_or("mysql-dbadm".to_string())
        );
        return Ok(());
    };

    tokio_run_command(command, server_connection)?;

    Ok(())
}

fn tokio_run_command(command: Command, server_connection: StdUnixStream) -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let tokio_socket = TokioUnixStream::from_std(server_connection)?;
            let mut message_stream = create_client_to_server_message_stream(tokio_socket);

            while let Some(Ok(message)) = message_stream.next().await {
                match message {
                    Response::Error(err) => {
                        anyhow::bail!("{err}");
                    }
                    Response::Ready => break,
                    message => {
                        eprintln!("Unexpected message from server: {message:?}");
                    }
                }
            }

            match command {
                Command::Create(args) => create_databases(args, message_stream).await,
                Command::Drop(args) => drop_databases(args, message_stream).await,
                Command::Show(args) => show_databases(args, message_stream).await,
                Command::Editperm(args) => {
                    let edit_privileges_args = EditPrivsArgs {
                        single_priv: None,
                        privs: vec![],
                        json: false,
                        editor: None,
                        yes: false,
                    };

                    edit_database_privileges(
                        edit_privileges_args,
                        Some(args.database),
                        message_stream,
                    )
                    .await
                }
            }
        })
}

async fn create_databases(
    args: CreateArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let database_names = args.name.iter().map(trim_db_name_to_32_chars).collect();

    let message = Request::CreateDatabases(database_names);
    server_connection.send(message).await?;

    let result = match server_connection.next().await {
        Some(Ok(Response::CreateDatabases(result))) => result,
        response => return erroneous_server_response(response),
    };

    server_connection.send(Request::Exit).await?;

    for (name, result) in result {
        match result {
            Ok(()) => println!("Database {name} created."),
            Err(err) => handle_create_database_error(&err, &name),
        }
    }

    Ok(())
}

async fn drop_databases(
    args: DatabaseDropArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let database_names = args.name.iter().map(trim_db_name_to_32_chars).collect();

    let message = Request::DropDatabases(database_names);
    server_connection.send(message).await?;

    let result = match server_connection.next().await {
        Some(Ok(Response::DropDatabases(result))) => result,
        response => return erroneous_server_response(response),
    };

    server_connection.send(Request::Exit).await?;

    for (name, result) in result {
        match result {
            Ok(()) => println!("Database {name} dropped."),
            Err(err) => handle_drop_database_error(&err, &name),
        }
    }

    Ok(())
}

async fn show_databases(
    args: DatabaseShowArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let database_names: Vec<MySQLDatabase> =
        args.name.iter().map(trim_db_name_to_32_chars).collect();

    let message = if database_names.is_empty() {
        let message = Request::ListDatabases(ListDatabasesRequest::new(None, false));
        server_connection.send(message).await?;
        let response = server_connection.next().await;
        let databases = match response {
            Some(Ok(Response::ListAllDatabases(databases))) => databases.unwrap_or(vec![]),
            response => return erroneous_server_response(response),
        };

        let database_names = databases.into_iter().map(|db| db.database).collect();

        Request::ListPrivileges(Some(database_names))
    } else {
        Request::ListPrivileges(Some(database_names))
    };
    server_connection.send(message).await?;

    let response = server_connection.next().await;

    server_connection.send(Request::Exit).await?;

    // NOTE: mysql-dbadm show has a quirk where valid database names
    //       for non-existent databases will report with no users.
    let results: Vec<Result<(MySQLDatabase, Vec<DatabasePrivilegeRow>), String>> = match response {
        Some(Ok(Response::ListPrivileges(result))) => result
            .into_iter()
            .map(|(name, rows)| match rows.map(|rows| (name.clone(), rows)) {
                Ok(rows) => Ok(rows),
                Err(ListPrivilegesError::DatabaseDoesNotExist) => Ok((name, vec![])),
                Err(err) => Err(format_show_database_error_message(&err, &name)),
            })
            .collect(),
        response => return erroneous_server_response(response),
    };

    for result in results {
        match result {
            Ok((name, rows)) => print_db_privs(&name, rows),
            Err(err) => eprintln!("{err}"),
        }
    }

    Ok(())
}

#[inline]
fn yn(value: bool) -> &'static str {
    if value { "Y" } else { "N" }
}

fn print_db_privs(name: &str, rows: Vec<DatabasePrivilegeRow>) {
    println!(
        concat!(
            "Database '{}':\n",
            "# User                Select  Insert  Update  Delete  Create   Drop   Alter   Index    Temp    Lock  References\n",
            "# ----------------    ------  ------  ------  ------  ------   ----   -----   -----    ----    ----  ----------"
        ),
        name,
    );
    if rows.is_empty() {
        println!("# (no permissions currently granted to any users)");
    } else {
        for privilege in rows {
            println!(
                "  {:<16}      {:<7} {:<7} {:<7} {:<7} {:<7} {:<7} {:<7} {:<7} {:<7} {:<7} {}",
                privilege.user,
                yn(privilege.select_priv),
                yn(privilege.insert_priv),
                yn(privilege.update_priv),
                yn(privilege.delete_priv),
                yn(privilege.create_priv),
                yn(privilege.drop_priv),
                yn(privilege.alter_priv),
                yn(privilege.index_priv),
                yn(privilege.create_tmp_table_priv),
                yn(privilege.lock_tables_priv),
                yn(privilege.references_priv)
            );
        }
    }
}
