use std::{io::IsTerminal, path::PathBuf};

use anyhow::Context;
use clap::Parser;
use clap_complete::ArgValueCompleter;
use dialoguer::Password;
use futures_util::SinkExt;
use tokio_stream::StreamExt;

use crate::{
    client::commands::{erroneous_server_response, print_authorization_owner_hint},
    core::{
        completion::mysql_user_completer,
        protocol::{
            ClientToServerMessageStream, ListUsersError, PasswordSource, Request, Response,
            SetPasswordError, print_set_password_output_status,
            request_validation::ValidationError,
        },
        types::MySQLUser,
    },
};

#[derive(Parser, Debug, Clone)]
pub struct PasswdUserArgs {
    /// The `MySQL` user whose password is to be changed
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(mysql_user_completer)))]
    #[arg(value_name = "USER_NAME")]
    username: MySQLUser,

    /// Read the new password from a file instead of prompting for it
    #[clap(
        short,
        long,
        value_name = "PATH",
        conflicts_with_all = ["stdin", "generate"]
    )]
    password_file: Option<PathBuf>,

    /// Read the new password from stdin instead of prompting for it
    #[clap(
        short = 'i',
        long,
        conflicts_with_all = ["password_file", "generate"]
    )]
    stdin: bool,

    /// Generate a new random password instead of prompting for one
    #[clap(
        short,
        long,
        conflicts_with_all = ["password_file", "stdin"]
    )]
    generate: bool,

    /// Print the information as JSON
    #[arg(short, long)]
    json: bool,
}

pub fn read_password_from_stdin_with_double_check(username: &MySQLUser) -> anyhow::Result<String> {
    Password::new()
        .with_prompt(format!("New MySQL password for user '{username}'"))
        .with_confirmation(
            format!("Retype new MySQL password for user '{username}'"),
            "Passwords do not match",
        )
        .interact()
        .map_err(Into::into)
}

pub async fn passwd_user(
    args: PasswdUserArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    // TODO: create a "user" exists check" command
    let message = Request::ListUsers(Some(vec![args.username.clone()]));
    if let Err(err) = server_connection.send(message).await {
        server_connection.close().await.ok();
        anyhow::bail!(err);
    }
    let response = match server_connection.next().await {
        Some(Ok(Response::ListUsers(users))) => users,
        response => return erroneous_server_response(response),
    };
    match response
        .get(&args.username)
        .unwrap_or(&Err(ListUsersError::UserDoesNotExist))
    {
        Ok(_) => {}
        Err(err) => {
            server_connection.send(Request::Exit).await?;
            server_connection.close().await.ok();
            anyhow::bail!("{}", err.to_error_message(&args.username));
        }
    }

    let password = if args.generate {
        PasswordSource::Generate
    } else if let Some(password_file) = args.password_file {
        PasswordSource::Explicit(
            std::fs::read_to_string(password_file)
                .context("Failed to read password file")?
                .trim()
                .to_string(),
        )
    } else if args.stdin {
        let mut buffer = String::new();
        std::io::stdin()
            .read_line(&mut buffer)
            .context("Failed to read password from stdin")?;
        PasswordSource::Explicit(buffer.trim().to_string())
    } else {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "Cannot prompt for password in non-interactive mode. Use --stdin, --password-file, or --generate to provide the password."
            );
        }
        PasswordSource::Explicit(read_password_from_stdin_with_double_check(&args.username)?)
    };

    let message = Request::PasswdUser((args.username.clone(), password));

    if let Err(err) = server_connection.send(message).await {
        server_connection.close().await.ok();
        anyhow::bail!(err);
    }

    let result = match server_connection.next().await {
        Some(Ok(Response::SetUserPassword(result))) => result,
        response => return erroneous_server_response(response),
    };

    print_set_password_output_status(&result, &args.username);

    if matches!(
        result,
        Err(SetPasswordError::ValidationError(
            ValidationError::AuthorizationError(_)
        ))
    ) {
        print_authorization_owner_hint(&mut server_connection).await?;
    }

    server_connection.send(Request::Exit).await?;

    if result.is_err() {
        std::process::exit(1);
    }

    Ok(())
}
