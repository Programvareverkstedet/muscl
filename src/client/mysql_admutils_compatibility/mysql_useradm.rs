use clap::{Parser, Subcommand};
use clap_complete::ArgValueCompleter;
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;

use std::os::unix::net::UnixStream as StdUnixStream;
use tokio::net::UnixStream as TokioUnixStream;

use crate::{
    client::{
        commands::{erroneous_server_response, read_password_from_stdin_with_double_check},
        mysql_admutils_compatibility::{
            common::trim_user_name_to_32_chars,
            error_messages::{
                handle_create_user_error, handle_drop_user_error, handle_list_users_error,
            },
        },
    },
    core::{
        bootstrap::bootstrap_server_connection_and_drop_privileges,
        completion::{mysql_user_completer, prefix_completer},
        protocol::{
            ClientToServerMessageStream, PasswordSource, Request, Response,
            create_client_to_server_message_stream,
        },
        types::MySQLUser,
    },
    server::sql::user_operations::DatabaseUser,
};

/// Create, delete or change password for the USER(s),
/// as determined by the COMMAND.
///
/// This is a compatibility layer for the 'mysql-useradm' command.
/// Please consider using the newer 'muscl' command instead.
#[derive(Parser)]
#[command(
    bin_name = "mysql-useradm",
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
}

#[derive(Subcommand)]
pub enum Command {
    /// create the USER(s).
    Create(CreateArgs),

    /// delete the USER(s).
    Delete(DeleteArgs),

    /// change the `MySQL` password for the USER(s).
    Passwd(PasswdArgs),

    /// give information about the USERS(s), or, if
    /// none are given, all the users you have.
    Show(ShowArgs),
}

#[derive(Parser)]
pub struct CreateArgs {
    /// The name of the USER(s) to create.
    #[arg(num_args = 1..)]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(prefix_completer)))]
    name: Vec<MySQLUser>,
}

#[derive(Parser)]
pub struct DeleteArgs {
    /// The name of the USER(s) to delete.
    #[arg(num_args = 1..)]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_user_completer)))]
    name: Vec<MySQLUser>,
}

#[derive(Parser)]
pub struct PasswdArgs {
    /// The name of the USER(s) to change the password for.
    #[arg(num_args = 1..)]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_user_completer)))]
    name: Vec<MySQLUser>,
}

#[derive(Parser)]
pub struct ShowArgs {
    /// The name of the USER(s) to show.
    #[arg(num_args = 0..)]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_user_completer)))]
    name: Vec<MySQLUser>,
}

/// **WARNING:** This function may be run with elevated privileges.
pub fn main() -> anyhow::Result<()> {
    let args: Args = Args::parse();

    let Some(command) = args.command else {
        println!(
            "Try `{} --help' for more information.",
            std::env::args()
                .next()
                .unwrap_or("mysql-useradm".to_string())
        );
        return Ok(());
    };

    let server_connection = bootstrap_server_connection_and_drop_privileges(
        args.server_socket_path,
        args.config,
        Default::default(),
    )?;

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
                Command::Create(args) => create_user(args, message_stream).await,
                Command::Delete(args) => drop_users(args, message_stream).await,
                Command::Passwd(args) => passwd_users(args, message_stream).await,
                Command::Show(args) => show_users(args, message_stream).await,
            }
        })
}

async fn create_user(
    args: CreateArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let db_users = args.name.iter().map(trim_user_name_to_32_chars).collect();

    let message = Request::CreateUsers(db_users);
    server_connection.send(message).await?;

    let result = match server_connection.next().await {
        Some(Ok(Response::CreateUsers(result))) => result,
        response => return erroneous_server_response(response),
    };

    server_connection.send(Request::Exit).await?;

    for (name, result) in result {
        match result {
            Ok(()) => println!("User '{name}' created."),
            Err(err) => handle_create_user_error(&err, &name),
        }
    }

    Ok(())
}

async fn drop_users(
    args: DeleteArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let db_users = args.name.iter().map(trim_user_name_to_32_chars).collect();

    let message = Request::DropUsers(db_users);
    server_connection.send(message).await?;

    let result = match server_connection.next().await {
        Some(Ok(Response::DropUsers(result))) => result,
        response => return erroneous_server_response(response),
    };

    server_connection.send(Request::Exit).await?;

    for (name, result) in result {
        match result {
            Ok(()) => println!("User '{name}' deleted."),
            Err(err) => handle_drop_user_error(&err, &name),
        }
    }

    Ok(())
}

async fn passwd_users(
    args: PasswdArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let db_users = args.name.iter().map(trim_user_name_to_32_chars).collect();

    let message = Request::ListUsers(Some(db_users));
    server_connection.send(message).await?;

    let response = match server_connection.next().await {
        Some(Ok(Response::ListUsers(result))) => result,
        response => return erroneous_server_response(response),
    };

    let argv0 = std::env::args()
        .next()
        .unwrap_or("mysql-useradm".to_string());

    let users = response
        .into_iter()
        .filter_map(|(name, result)| match result {
            Ok(user) => Some(user),
            Err(err) => {
                handle_list_users_error(&err, &name);
                None
            }
        })
        .collect::<Vec<_>>();

    for user in users {
        let password = read_password_from_stdin_with_double_check(&user.user)?;
        let message = Request::PasswdUser((user.user.clone(), PasswordSource::Explicit(password)));
        server_connection.send(message).await?;
        match server_connection.next().await {
            Some(Ok(Response::SetUserPassword(result))) => match result {
                Ok(_) => println!("Password updated for user '{}'.", &user.user),
                Err(_) => eprintln!(
                    "{}: Failed to update password for user '{}'.",
                    argv0, user.user,
                ),
            },
            response => return erroneous_server_response(response),
        }
    }

    server_connection.send(Request::Exit).await?;

    Ok(())
}

async fn show_users(
    args: ShowArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    let db_users: Vec<_> = args.name.iter().map(trim_user_name_to_32_chars).collect();

    let message = if db_users.is_empty() {
        Request::ListUsers(None)
    } else {
        Request::ListUsers(Some(db_users))
    };
    server_connection.send(message).await?;

    let users: Vec<DatabaseUser> = match server_connection.next().await {
        Some(Ok(Response::ListAllUsers(result))) => match result {
            Ok(users) => users,
            Err(err) => {
                eprintln!("Failed to list users: {err:?}");
                return Ok(());
            }
        },
        Some(Ok(Response::ListUsers(result))) => result
            .into_iter()
            .filter_map(|(name, result)| match result {
                Ok(user) => Some(user),
                Err(err) => {
                    handle_list_users_error(&err, &name);
                    None
                }
            })
            .collect(),
        response => return erroneous_server_response(response),
    };

    server_connection.send(Request::Exit).await?;

    for user in users {
        if user.has_password {
            println!("User '{}': password set.", user.user);
        } else {
            println!("User '{}': no password set.", user.user);
        }
    }

    Ok(())
}
