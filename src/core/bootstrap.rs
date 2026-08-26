use std::{
    fs,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, anyhow};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use nix::{
    libc::{EXIT_SUCCESS, exit},
    unistd::{AccessFlags, access},
};
use sqlx::mysql::MySqlPoolOptions;
use std::os::unix::net::UnixStream as StdUnixStream;
use tokio::{net::UnixStream as TokioUnixStream, sync::RwLock};
use tracing_subscriber::prelude::*;

use crate::{
    core::{
        common::{DEFAULT_CONFIG_PATH, DEFAULT_SOCKET_PATH, UnixUser, executing_in_suid_sgid_mode},
        protocol::request_validation::GroupDenylist,
    },
    server::{
        authorization::read_and_parse_group_denylist,
        config::{MysqlConfig, ServerConfig},
        landlock::landlock_restrict_server,
        session_handler::{self, SessionId},
    },
};

/// Determine whether we will make a connection to an external server
/// or start an internal server with elevated privileges.
///
/// If neither is feasible, an error is returned.
fn will_connect_to_external_server(
    server_socket_path: Option<&PathBuf>,
    // This parameter is only used in suid-sgid-mode
    #[allow(unused_variables)] config_path: Option<&PathBuf>,
) -> anyhow::Result<bool> {
    if server_socket_path.is_some() {
        return Ok(true);
    }

    #[cfg(feature = "suid-sgid-mode")]
    if config_path.is_some() {
        return Ok(false);
    }

    if fs::metadata(DEFAULT_SOCKET_PATH).is_ok() {
        return Ok(true);
    }

    #[cfg(feature = "suid-sgid-mode")]
    if fs::metadata(DEFAULT_CONFIG_PATH).is_ok() {
        return Ok(false);
    }

    #[cfg(feature = "suid-sgid-mode")]
    anyhow::bail!("No socket path or config path provided, and no default socket or config found");

    #[cfg(not(feature = "suid-sgid-mode"))]
    anyhow::bail!("No socket path provided, and no default socket found");
}

/// This function is used to bootstrap the connection to the server.
/// This can happen in two ways:
///
/// 1. If a socket path is provided, or exists in the default location,
///    the function will connect to the socket and authenticate with the
///    server to ensure that the server knows the uid of the client.
///
/// 2. If a config path is provided, or exists in the default location,
///    and the config is readable, the function will assume it is either
///    setuid or setgid, and will fork a child process to run the server
///    with the provided config. The server will exit silently by itself
///    when it is done, and this function will only return for the client
///    with the socket for the server.
///
/// If neither of these options are available, the function will fail.
///
/// Note that this function is also responsible for setting up logging,
/// because in the case of an internal server, we need to drop privileges
/// before we can initialize logging.
///
/// **WARNING:** This function may be run with elevated privileges.
pub fn bootstrap_server_connection_and_drop_privileges(
    server_socket_path: Option<PathBuf>,
    config: Option<PathBuf>,
    verbose: Verbosity<InfoLevel>,
) -> anyhow::Result<StdUnixStream> {
    if will_connect_to_external_server(server_socket_path.as_ref(), config.as_ref())? {
        assert!(
            !executing_in_suid_sgid_mode()?,
            "The executable should not be SUID or SGID when connecting to an external server"
        );

        init_stderr_tracing_subscriber(verbose)?;

        connect_to_external_server(server_socket_path)
    } else if cfg!(feature = "suid-sgid-mode") {
        // NOTE: We need to be really careful with the code up until this point,
        //       as we might be running with elevated privileges.
        let server_connection = bootstrap_internal_server_and_drop_privs(config)?;

        init_stderr_tracing_subscriber(verbose)?;

        Ok(server_connection)
    } else {
        anyhow::bail!("SUID/SGID support is not enabled, cannot start internal server");
    }
}

fn init_stderr_tracing_subscriber(verbose: Verbosity<InfoLevel>) -> anyhow::Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(verbose.tracing_level_filter().into())
        .from_env_lossy();

    let subscriber = tracing_subscriber::Registry::default()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_line_number(cfg!(debug_assertions))
                .with_target(cfg!(debug_assertions))
                .with_thread_ids(false)
                .with_thread_names(false),
        );

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global default tracing subscriber")
}

fn socket_path_is_ok(path: &Path) -> anyhow::Result<()> {
    fs::metadata(path)
        .context(format!("Failed to get metadata for {:?}", path))
        .and_then(|meta| {
            if !meta.file_type().is_socket() {
                anyhow::bail!("{:?} is not a unix socket", path);
            }

            access(path, AccessFlags::R_OK | AccessFlags::W_OK)
                .with_context(|| format!("Socket at {:?} is not readable/writable", path))?;

            Ok(())
        })
}

fn connect_to_external_server(
    server_socket_path: Option<PathBuf>,
) -> anyhow::Result<StdUnixStream> {
    if let Some(socket_path) = server_socket_path {
        tracing::trace!("Checking socket at {:?}", socket_path);
        socket_path_is_ok(&socket_path)?;

        tracing::debug!("Connecting to socket at {:?}", socket_path);
        return match StdUnixStream::connect(socket_path) {
            Ok(socket) => Ok(socket),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(anyhow::anyhow!("Socket not found")),
                std::io::ErrorKind::PermissionDenied => Err(anyhow::anyhow!("Permission denied")),
                _ => Err(anyhow::anyhow!("Failed to connect to socket: {e}")),
            },
        };
    }

    if fs::metadata(DEFAULT_SOCKET_PATH).is_ok() {
        tracing::trace!("Checking socket at {:?}", DEFAULT_SOCKET_PATH);
        socket_path_is_ok(Path::new(DEFAULT_SOCKET_PATH))?;

        tracing::debug!("Connecting to default socket at {:?}", DEFAULT_SOCKET_PATH);
        return match StdUnixStream::connect(DEFAULT_SOCKET_PATH) {
            Ok(socket) => Ok(socket),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(anyhow::anyhow!("Socket not found")),
                std::io::ErrorKind::PermissionDenied => Err(anyhow::anyhow!("Permission denied")),
                _ => Err(anyhow::anyhow!("Failed to connect to socket: {e}")),
            },
        };
    }

    anyhow::bail!(
        "No socket path provided, and no socket found found at default location {DEFAULT_SOCKET_PATH}"
    );
}

// TODO: this function is security critical, it should be integration tested
//       in isolation.
/// Drop privileges to the real user and group of the process.
/// If the process is not running with elevated privileges, this function
/// is a no-op.
pub fn drop_privs() -> anyhow::Result<()> {
    tracing::debug!("Dropping privileges");
    let real_uid = nix::unistd::getuid();
    let real_gid = nix::unistd::getgid();

    nix::unistd::setuid(real_uid).context("Failed to drop privileges")?;
    nix::unistd::setgid(real_gid).context("Failed to drop privileges")?;

    debug_assert_eq!(nix::unistd::getuid(), real_uid);
    debug_assert_eq!(nix::unistd::getgid(), real_gid);

    tracing::debug!("Privileges dropped successfully");
    Ok(())
}

/// Bootstrap an internal server by forking a child process to run the server, giving it
/// the other half of a Unix socket pair to communicate with the client process.
fn bootstrap_internal_server_and_drop_privs(
    config_path: Option<PathBuf>,
) -> anyhow::Result<StdUnixStream> {
    if let Some(config_path) = config_path {
        if !executing_in_suid_sgid_mode()? {
            anyhow::bail!("Executable is not SUID/SGID - refusing to start internal sever");
        }

        // ensure config exists and is readable
        if fs::metadata(&config_path).is_err() {
            return Err(anyhow::anyhow!("Config file not found or not readable"));
        }

        tracing::debug!("Starting server with config at {:?}", config_path);
        let socket = invoke_server_with_config(&config_path)?;
        drop_privs()?;
        return Ok(socket);
    }

    let config_path = PathBuf::from(DEFAULT_CONFIG_PATH);
    if fs::metadata(&config_path).is_ok() {
        if !executing_in_suid_sgid_mode()? {
            anyhow::bail!("Executable is not SUID/SGID - refusing to start internal sever");
        }
        tracing::debug!("Starting server with default config at {:?}", config_path);
        let socket = invoke_server_with_config(&config_path)?;
        drop_privs()?;
        return Ok(socket);
    }

    anyhow::bail!("No config path provided, and no default config found");
}

// TODO: we should somehow ensure that the forked process is killed on completion,
//       just in case the client does not behave properly.
/// Fork a child process to run the server with the provided config.
/// The server will exit silently by itself when it is done, and this function
/// will only return for the client with the socket for the server.
fn invoke_server_with_config(config_path: &Path) -> anyhow::Result<StdUnixStream> {
    let (server_socket, client_socket) = StdUnixStream::pair()?;
    let unix_user = UnixUser::from_uid(nix::unistd::getuid().as_raw())?;

    match (unsafe { nix::unistd::fork() }).context("Failed to fork")? {
        nix::unistd::ForkResult::Parent { .. } => Ok(client_socket),
        nix::unistd::ForkResult::Child => {
            landlock_restrict_server(Some(config_path))
                .context("Failed to apply Landlock restrictions to the server process")?;

            match run_forked_server(config_path, server_socket, &unix_user) {
                Err(e) => Err(e),
                Ok(()) => unreachable!(),
            }
        }
    }
}

/// Construct a `MySQL` connection pool that consists of exactly one connection.
///
/// This is used for the internal server in SUID/SGID mode, where the server session
/// only ever will get a single client.
async fn construct_single_connection_mysql_pool(
    config: &MysqlConfig,
) -> anyhow::Result<sqlx::MySqlPool> {
    let mysql_config = config.as_mysql_connect_options()?;

    let pool_opts = MySqlPoolOptions::new()
        .max_connections(1)
        .min_connections(1);

    config.log_connection_notice();

    let pool = match tokio::time::timeout(
        Duration::from_secs(config.timeout),
        pool_opts.connect_with(mysql_config),
    )
    .await
    {
        Ok(connection) => connection.context("Failed to connect to the database"),
        Err(_) => Err(anyhow!("Timed out after {} seconds", config.timeout))
            .context("Failed to connect to the database"),
    }?;

    Ok(pool)
}

/// Run a single server session in the forked process.
///
/// This function will not return, but will exit the process with a success code.
/// The function assumes that it's caller has already forked the process.
fn run_forked_server(
    config_path: &Path,
    server_socket: StdUnixStream,
    unix_user: &UnixUser,
) -> anyhow::Result<()> {
    let config = ServerConfig::read_config_from_path(config_path)
        .context("Failed to read server config in forked process")?;

    let group_denylist = if let Some(denylist_path) = &config.authorization.group_denylist_file {
        read_and_parse_group_denylist(denylist_path)
            .context("Failed to read and parse group denylist")?
    } else {
        GroupDenylist::new()
    };

    let result: anyhow::Result<()> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to start Tokio runtime")?
        .block_on(async {
            let socket = TokioUnixStream::from_std(server_socket)?;
            let db_pool = construct_single_connection_mysql_pool(&config.mysql).await?;
            let db_is_mariadb = {
                let mut conn = db_pool.acquire().await?;
                let version_row: String = sqlx::query_scalar("SELECT VERSION()")
                    .fetch_one(&mut *conn)
                    .await
                    .context("Failed to query MySQL version")?;
                version_row.to_lowercase().contains("mariadb")
            };

            let session_id = SessionId::new(0);
            let db_pool = Arc::new(RwLock::new(db_pool));
            session_handler::session_handler_with_unix_user(
                socket,
                session_id,
                unix_user,
                db_pool,
                db_is_mariadb,
                &group_denylist,
            )
            .await?;
            Ok(())
        });

    result?;

    unsafe {
        exit(EXIT_SUCCESS);
    }
}
