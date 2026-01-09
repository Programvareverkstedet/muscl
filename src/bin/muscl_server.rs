use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use tracing_subscriber::layer::SubscriberExt;

use muscl_lib::{
    core::common::{ASCII_BANNER, DEFAULT_CONFIG_PATH, KIND_REGARDS},
    server::{landlock::landlock_restrict_server, supervisor::Supervisor},
};

#[derive(Parser, Debug, Clone)]
pub struct ServerArgs {
    #[command(subcommand)]
    pub subcmd: ServerCommand,

    /// Enable systemd mode
    #[cfg(target_os = "linux")]
    #[arg(long)]
    pub systemd: bool,

    /// Disable Landlock sandboxing.
    ///
    /// This is useful if you are planning to reload the server's configuration.
    #[arg(long)]
    pub disable_landlock: bool,

    // NOTE: be careful not to add short options that collide with the `edit-privs` privilege
    //       characters. It should in theory be possible for `edit-privs` to ignore any options
    //       specified here, but in practice clap is being difficult to work with.
    /// Path to where the server's unix socket should be created. This is only relevant when
    /// not using systemd socket activation.
    #[arg(
        long = "socket",
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
    )]
    socket_path: Option<PathBuf>,

    /// Config file to use for the server.
    #[arg(
        long = "config",
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
    )]
    config_path: Option<PathBuf>,

    #[command(flatten)]
    verbosity: Verbosity<InfoLevel>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ServerCommand {
    /// Start the server and listen for incoming connections on the unix socket
    /// specified in the configuration file.
    Listen,

    /// Start the server using systemd socket activation.
    SocketActivate,
}

const LOG_LEVEL_WARNING: &str = r#"
===================================================
== WARNING: LOG LEVEL IS SET TO 'TRACE'!         ==
== THIS WILL CAUSE THE SERVER TO LOG SQL QUERIES ==
== THAT MAY CONTAIN SENSITIVE INFORMATION LIKE   ==
== PASSWORDS AND AUTHENTICATION TOKENS.          ==
== THIS IS INTENDED FOR DEBUGGING PURPOSES ONLY  ==
== AND SHOULD *NEVER* BE USED IN PRODUCTION.     ==
===================================================
"#;

const MIN_TOKIO_WORKER_THREADS: usize = 4;

fn main() -> anyhow::Result<()> {
    let args = ServerArgs::parse();

    if !args.disable_landlock {
        landlock_restrict_server(args.config_path.as_deref())
            .context("Failed to apply Landlock restrictions to the server process")?;
    }

    let worker_thread_count = std::cmp::max(num_cpus::get(), MIN_TOKIO_WORKER_THREADS);

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_thread_count)
        .enable_all()
        .build()
        .context("Failed to start Tokio runtime")?
        .block_on(handle_command(args))?;

    Ok(())
}

fn trace_server_prelude() {
    let message = [ASCII_BANNER, "", KIND_REGARDS, ""].join("\n");
    tracing::info!(message);
}

async fn handle_command(args: ServerArgs) -> anyhow::Result<()> {
    let mut auto_detected_systemd_mode = false;

    #[cfg(target_os = "linux")]
    let systemd_mode = args.systemd || {
        if let Ok(true) = sd_notify::booted() {
            auto_detected_systemd_mode = true;
            true
        } else {
            false
        }
    };

    #[cfg(not(target_os = "linux"))]
    let systemd_mode = false;

    if systemd_mode {
        #[cfg(target_os = "linux")]
        {
            let subscriber = tracing_subscriber::Registry::default()
                .with(args.verbosity.tracing_level_filter())
                .with(tracing_journald::layer()?);

            tracing::subscriber::set_global_default(subscriber)
                .context("Failed to set global default tracing subscriber")?;

            trace_server_prelude();

            if args.verbosity.tracing_level_filter() >= tracing::Level::TRACE {
                tracing::warn!("{}", LOG_LEVEL_WARNING.trim());
            }

            if auto_detected_systemd_mode {
                tracing::debug!("Running in systemd mode, auto-detected");
            } else {
                tracing::debug!("Running in systemd mode");
            }
        }
    } else {
        let subscriber = tracing_subscriber::Registry::default()
            .with(args.verbosity.tracing_level_filter())
            .with(
                tracing_subscriber::fmt::layer()
                    .with_line_number(cfg!(debug_assertions))
                    .with_target(cfg!(debug_assertions))
                    .with_thread_ids(false)
                    .with_thread_names(false),
            );

        tracing::subscriber::set_global_default(subscriber)
            .context("Failed to set global default tracing subscriber")?;

        trace_server_prelude();

        tracing::debug!("Running in standalone mode");
    }

    let config_path = args
        .config_path
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

    match args.subcmd {
        ServerCommand::Listen => {
            Supervisor::new(config_path, systemd_mode)
                .await?
                .run()
                .await
        }
        ServerCommand::SocketActivate => {
            if !args.systemd {
                anyhow::bail!(concat!(
                    "The `--systemd` flag must be used with the `socket-activate` command.\n",
                    "This command currently only supports socket activation under systemd."
                ));
            }

            Supervisor::new(config_path, systemd_mode)
                .await?
                .run()
                .await
        }
    }
}
