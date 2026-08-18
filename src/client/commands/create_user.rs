use std::io::IsTerminal;

use clap::Parser;
use clap_complete::ArgValueCompleter;
use dialoguer::Confirm;
use futures_util::SinkExt;
use tokio_stream::StreamExt;

use crate::{
    client::commands::{
        erroneous_server_response, print_authorization_owner_hint,
        read_password_from_stdin_with_double_check,
    },
    core::{
        completion::prefix_completer,
        protocol::{
            ClientToServerMessageStream, CreateUserError, PasswordSource, Request, Response,
            print_create_users_output_status, print_create_users_output_status_json,
            print_set_password_output_status, request_validation::ValidationError,
        },
        types::MySQLUser,
    },
};

#[derive(Parser, Debug, Clone)]
pub struct CreateUserArgs {
    /// The `MySQL` user(s) to create
    #[arg(num_args = 1.., value_name = "USER_NAME")]
    #[cfg_attr(not(feature = "suid-sgid-mode"), arg(add = ArgValueCompleter::new(prefix_completer)))]
    username: Vec<MySQLUser>,

    /// Do not ask for a password, leave it unset
    #[clap(long)]
    no_password: bool,

    /// Print the information as JSON
    ///
    /// Note that this implies `--no-password`, since the command will become non-interactive.
    #[arg(short, long)]
    json: bool,
}

pub async fn create_users(
    args: CreateUserArgs,
    mut server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    if args.username.is_empty() {
        anyhow::bail!("No usernames provided");
    }

    let message = Request::CreateUsers(args.username.clone());
    if let Err(err) = server_connection.send(message).await {
        server_connection.close().await.ok();
        anyhow::bail!(anyhow::Error::from(err).context("Failed to communicate with server"));
    }

    let result = match server_connection.next().await {
        Some(Ok(Response::CreateUsers(result))) => result,
        response => return erroneous_server_response(response),
    };

    if args.json {
        print_create_users_output_status_json(&result);
    } else {
        print_create_users_output_status(&result);

        if result.iter().any(|(_, res)| {
            matches!(
                res,
                Err(CreateUserError::ValidationError(
                    ValidationError::AuthorizationError(_)
                ))
            )
        }) {
            print_authorization_owner_hint(&mut server_connection).await?;
        }

        let successfully_created_users = result
            .iter()
            .filter_map(|(username, result)| result.as_ref().ok().map(|()| username))
            .collect::<Vec<_>>();

        if !std::io::stdin().is_terminal()
            && !args.no_password
            && !successfully_created_users.is_empty()
        {
            anyhow::bail!(
                "Cannot prompt for passwords in non-interactive mode. Use --no-password to skip setting passwords."
            );
        }

        for username in successfully_created_users {
            if !args.no_password
                && Confirm::new()
                    .with_prompt(format!(
                        "Do you want to set a password for user '{username}'?"
                    ))
                    .default(false)
                    .interact()?
            {
                let password = read_password_from_stdin_with_double_check(username)?;
                let message =
                    Request::PasswdUser((username.to_owned(), PasswordSource::Explicit(password)));

                if let Err(err) = server_connection.send(message).await {
                    server_connection.close().await.ok();
                    anyhow::bail!(err);
                }

                match server_connection.next().await {
                    Some(Ok(Response::SetUserPassword(result))) => {
                        print_set_password_output_status(&result, username);
                    }
                    response => return erroneous_server_response(response),
                }

                println!();
            }
        }
    }

    server_connection.send(Request::Exit).await?;

    if result.values().any(std::result::Result::is_err) {
        std::process::exit(1);
    }

    Ok(())
}
