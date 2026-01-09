use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::PathBuf;

use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand, crate_version};
use clap_complete::CompleteEnv;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use tokio::net::UnixStream as TokioUnixStream;
use tokio_stream::StreamExt;

use muscl_lib::{
    client::{
        commands::{
            CheckAuthArgs, CreateDbArgs, CreateUserArgs, DropDbArgs, DropUserArgs, EditPrivsArgs,
            LockUserArgs, PasswdUserArgs, ShowDbArgs, ShowPrivsArgs, ShowUserArgs, UnlockUserArgs,
            check_authorization, create_databases, create_users, drop_databases, drop_users,
            edit_database_privileges, lock_users, passwd_user, show_database_privileges,
            show_databases, show_users, unlock_users,
        },
        mysql_admutils_compatibility::{mysql_dbadm, mysql_useradm},
    },
    core::{
        bootstrap::bootstrap_server_connection_and_drop_privileges,
        common::{ASCII_BANNER, KIND_REGARDS},
        protocol::{ClientToServerMessageStream, Response, create_client_to_server_message_stream},
    },
};

#[cfg(feature = "suid-sgid-mode")]
use muscl_lib::core::common::executing_in_suid_sgid_mode;

const fn long_version() -> &'static str {
    macro_rules! feature {
        ($title:expr, $flag:expr) => {
            if cfg!(feature = $flag) {
                concat!($title, ": enabled")
            } else {
                concat!($title, ": disabled")
            }
        };
    }

    const_format::concatcp!(
        crate_version!(),
        "\n",
        "build profile: ",
        env!("BUILD_PROFILE"),
        "\n",
        "commit: ",
        env!("GIT_COMMIT"),
        "\n\n",
        "[features]\n",
        feature!("SUID/SGID mode", "suid-sgid-mode"),
        "\n",
        feature!(
            "mysql-admutils compatibility",
            "mysql-admutils-compatibility"
        ),
        "\n",
        "\n",
        "[dependencies]\n",
        const_format::str_replace!(env!("DEPENDENCY_LIST"), ";", "\n")
    )
}

const LONG_VERSION: &str = long_version();

const EXAMPLES: &str = const_format::concatcp!(
    color_print::cstr!("<bold><underline>Examples:</underline></bold>"),
    r#"
  # Display help information for any specific command
  muscl <command> --help

  # Create two users 'alice_user1' and 'alice_user2'
  muscl create-user alice_user1 alice_user2

  # Create two databases 'alice_db1' and 'alice_db2'
  muscl create-db alice_db1 alice_db2

  # Grant Select, Update, Insert and Delete privileges on 'alice_db1' to 'alice_user1'
  muscl edit-privs alice_db1 alice_user1 +suid

  # Show all databases
  muscl show-db
  muscl sd

  # Show which users have privileges on which databases
  muscl show-privs
  muscl sp
"#,
);

const BEFORE_LONG_HELP: &str = const_format::concatcp!("\x1b[1m", ASCII_BANNER, "\x1b[0m");
const AFTER_LONG_HELP: &str = const_format::concatcp!(EXAMPLES, "\n", KIND_REGARDS,);

/// Database administration tool for non-admin users to manage their own MySQL databases and users.
///
/// This tool allows you to manage users and databases in MySQL.
///
/// You are only allowed to manage databases and users that are prefixed with
/// either your username, or a group that you are a member of.
#[derive(Parser, Debug)]
#[command(
  bin_name = "muscl",
  author = "Programvareverkstedet <projects@pvv.ntnu.no>",
  version,
  about,
  disable_help_subcommand = true,
  propagate_version = true,
  before_long_help = BEFORE_LONG_HELP,
  after_long_help = AFTER_LONG_HELP,
  long_version = LONG_VERSION,
  // NOTE: All non-registered "subcommands" are processed before Arg::parse() is called.
  subcommand_required = true,
)]
struct Args {
    #[command(subcommand)]
    command: ClientCommand,

    // NOTE: be careful not to add short options that collide with the `edit-privs` privilege
    //       characters. It should in theory be possible for `edit-privs` to ignore any options
    //       specified here, but in practice clap is being difficult to work with.
    /// Path to the socket of the server.
    #[arg(
        long = "server-socket",
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
        global = true,
        hide_short_help = true
    )]
    server_socket_path: Option<PathBuf>,

    /// Config file to use for the server.
    ///
    /// This is only useful when running in SUID/SGID mode.
    #[cfg(feature = "suid-sgid-mode")]
    #[arg(
        long = "config",
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
        global = true,
        hide_short_help = true
    )]
    config_path: Option<PathBuf>,

    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

const EDIT_PRIVS_EXAMPLES: &str = color_print::cstr!(
    r#"
<bold><underline>Examples:</underline></bold>
  # Open interactive editor to edit privileges
  muscl edit-privs

  # Set privileges `SELECT`, `INSERT`, and `UPDATE` for user `my_user` on database `my_db`
  muscl edit-privs my_db my_user siu

  # Set all privileges for user `my_other_user` on database `my_other_db`
  muscl edit-privs my_other_db my_other_user A

  # Add the `DELETE` privilege for user `my_user` on database `my_db`
  muscl edit-privs my_db my_user +d

  # Set miscellaneous privileges for multiple users on database `my_db`
  muscl edit-privs -p my_db:my_user:siu -p my_db:my_other_user:+ct -p my_db:yet_another_user:-d
"#
);

#[derive(Subcommand, Debug, Clone)]
#[command(subcommand_required = true)]
pub enum ClientCommand {
    /// Check whether you are authorized to manage the specified databases or users.
    #[command(alias = "ca")]
    CheckAuth(CheckAuthArgs),

    /// Create one or more databases
    #[command(alias = "cd")]
    CreateDb(CreateDbArgs),

    /// Delete one or more databases
    #[command(alias = "dd")]
    DropDb(DropDbArgs),

    /// Print information about one or more databases
    ///
    /// If no database name is provided, all databases you have access will be shown.
    #[command(alias = "sd")]
    ShowDb(ShowDbArgs),

    /// Print user privileges for one or more databases
    ///
    /// If no database names are provided, all databases you have access to will be shown.
    #[command(alias = "sp")]
    ShowPrivs(ShowPrivsArgs),

    /// Change user privileges for one or more databases. See `edit-privs --help` for details.
    ///
    /// This command has three modes of operation:
    ///
    /// 1. Interactive mode:
    ///
    ///    If no arguments are provided, the user will be prompted to edit the privileges using a text editor.
    ///
    ///    You can configure your preferred text editor by setting the `VISUAL` or `EDITOR` environment variables.
    ///
    ///    Follow the instructions inside the editor for more information.
    ///
    /// 2. Non-interactive human-friendly mode:
    ///
    ///    You can provide the command with three positional arguments:
    ///
    ///    - `<DB_NAME>`: The name of the database for which you want to edit privileges.
    ///    - `<USER_NAME>`: The name of the user whose privileges you want to edit.
    ///    - `<[+-]PRIVILEGES>`: A string representing the privileges to set for the user.
    ///
    ///    The `<[+-]PRIVILEGES>` argument is a string of characters, each representing a single privilege.
    ///    The character `A` is an exception - it represents all privileges.
    ///    The optional leading character can be either `+` to grant additional privileges or `-` to revoke privileges.
    ///    If omitted, the privileges will be set exactly as specified, removing any privileges not listed, and adding any that are.
    ///
    ///    The character-to-privilege mapping is defined as follows:
    ///
    ///    - `s` - SELECT
    ///    - `i` - INSERT
    ///    - `u` - UPDATE
    ///    - `d` - DELETE
    ///    - `c` - CREATE
    ///    - `D` - DROP
    ///    - `a` - ALTER
    ///    - `I` - INDEX
    ///    - `t` - CREATE TEMPORARY TABLES
    ///    - `l` - LOCK TABLES
    ///    - `r` - REFERENCES
    ///    - `A` - ALL PRIVILEGES
    ///
    /// 3. Non-interactive batch mode:
    ///
    ///    By using the `-p` flag, you can provide multiple privilege edits in a single command.
    ///
    ///    The flag value should be formatted as `DB_NAME:USER_NAME:[+-]PRIVILEGES`
    ///    where the privileges are a string of characters, each representing a single privilege.
    ///    (See the character-to-privilege mapping above.)
    ///
    #[command(
        verbatim_doc_comment,
        override_usage = "muscl edit-privs [OPTIONS] [ -p <DB_NAME:USER_NAME:[+-]PRIVILEGES>... | <DB_NAME> <USER_NAME> <[+-]PRIVILEGES> ]",
        after_long_help = EDIT_PRIVS_EXAMPLES,
        alias = "ep",
    )]
    EditPrivs(EditPrivsArgs),

    /// Create one or more users
    #[command(alias = "cu")]
    CreateUser(CreateUserArgs),

    /// Delete one or more users
    #[command(alias = "du")]
    DropUser(DropUserArgs),

    /// Change the MySQL password for a user
    #[command(alias = "pu")]
    PasswdUser(PasswdUserArgs),

    /// Print information about one or more users
    ///
    /// If no username is provided, all users you have access will be shown.
    #[command(alias = "su")]
    ShowUser(ShowUserArgs),

    /// Lock account for one or more users
    #[command(alias = "lu")]
    LockUser(LockUserArgs),

    /// Unlock account for one or more users
    #[command(alias = "uu")]
    UnlockUser(UnlockUserArgs),
}

pub async fn handle_command(
    command: ClientCommand,
    server_connection: ClientToServerMessageStream,
) -> anyhow::Result<()> {
    match command {
        ClientCommand::CheckAuth(args) => check_authorization(args, server_connection).await,
        ClientCommand::CreateDb(args) => create_databases(args, server_connection).await,
        ClientCommand::DropDb(args) => drop_databases(args, server_connection).await,
        ClientCommand::ShowDb(args) => show_databases(args, server_connection).await,
        ClientCommand::ShowPrivs(args) => show_database_privileges(args, server_connection).await,
        ClientCommand::EditPrivs(args) => {
            edit_database_privileges(args, None, server_connection).await
        }
        ClientCommand::CreateUser(args) => create_users(args, server_connection).await,
        ClientCommand::DropUser(args) => drop_users(args, server_connection).await,
        ClientCommand::PasswdUser(args) => passwd_user(args, server_connection).await,
        ClientCommand::ShowUser(args) => show_users(args, server_connection).await,
        ClientCommand::LockUser(args) => lock_users(args, server_connection).await,
        ClientCommand::UnlockUser(args) => unlock_users(args, server_connection).await,
    }
}

/// **WARNING:** This function may be run with elevated privileges.
fn main() -> anyhow::Result<()> {
    if handle_dynamic_completion()?.is_some() {
        return Ok(());
    }

    #[cfg(feature = "mysql-admutils-compatibility")]
    if handle_mysql_admutils_command()?.is_some() {
        return Ok(());
    }

    let args: Args = Args::parse();

    let connection = bootstrap_server_connection_and_drop_privileges(
        args.server_socket_path,
        #[cfg(feature = "suid-sgid-mode")]
        args.config_path,
        #[cfg(not(feature = "suid-sgid-mode"))]
        None,
        args.verbose,
    )?;

    tokio_run_command(args.command, connection)?;

    Ok(())
}

/// **WARNING:** This function may be run with elevated privileges.
fn handle_dynamic_completion() -> anyhow::Result<Option<()>> {
    if std::env::var_os("COMPLETE").is_some() {
        #[cfg(feature = "suid-sgid-mode")]
        if executing_in_suid_sgid_mode()? {
            use muscl_lib::core::bootstrap::drop_privs;
            drop_privs()?
        }

        let argv0 = std::env::args()
            .next()
            .and_then(|s| {
                PathBuf::from(s)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .ok_or(anyhow::anyhow!(
                "Could not determine executable name for completion"
            ))?;

        let command = match argv0.as_str() {
            "muscl" => Args::command(),
            "mysql-dbadm" => mysql_dbadm::Args::command(),
            "mysql-useradm" => mysql_useradm::Args::command(),
            command => anyhow::bail!("Unknown executable name: `{}`", command),
        };

        CompleteEnv::with_factory(move || command.clone()).complete();

        Ok(Some(()))
    } else {
        Ok(None)
    }
}

/// **WARNING:** This function may be run with elevated privileges.
fn handle_mysql_admutils_command() -> anyhow::Result<Option<()>> {
    let argv0 = std::env::args().next().and_then(|s| {
        PathBuf::from(s)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
    });

    match argv0.as_deref() {
        Some("mysql-dbadm") => mysql_dbadm::main().map(Some),
        Some("mysql-useradm") => mysql_useradm::main().map(Some),
        _ => Ok(None),
    }
}

/// Run the given commmand (from the client side) using Tokio.
fn tokio_run_command(
    command: ClientCommand,
    server_connection: StdUnixStream,
) -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to start Tokio runtime")?
        .block_on(async {
            let tokio_socket = TokioUnixStream::from_std(server_connection)?;
            let mut message_stream = create_client_to_server_message_stream(tokio_socket);

            while let Some(Ok(message)) = message_stream.next().await {
                match message {
                    Response::Error(err) => {
                        anyhow::bail!("{}", err);
                    }
                    Response::Ready => break,
                    message => {
                        eprintln!("Unexpected message from server: {:?}", message);
                    }
                }
            }

            handle_command(command, message_stream).await
        })
}
