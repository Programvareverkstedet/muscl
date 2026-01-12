use std::{
    fs,
    os::{fd::FromRawFd, unix::net::UnixListener as StdUnixListener},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, anyhow};
use sqlx::MySqlPool;
use tokio::{
    net::UnixListener as TokioUnixListener,
    select,
    sync::{Mutex, RwLock, broadcast},
    task::JoinHandle,
    time::interval,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    core::protocol::request_validation::GroupDenylist,
    server::{
        authorization::read_and_parse_group_denylist,
        config::{MysqlConfig, ServerConfig},
        session_handler::{SessionId, session_handler},
    },
};

#[derive(Clone, Debug)]
pub enum SupervisorMessage {
    StopAcceptingNewConnections,
    ResumeAcceptingNewConnections,
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct ReloadEvent;

#[allow(dead_code)]
pub struct Supervisor {
    config_path: PathBuf,
    config: Arc<Mutex<ServerConfig>>,
    group_deny_list: Arc<RwLock<GroupDenylist>>,
    systemd_mode: bool,

    shutdown_cancel_token: CancellationToken,
    reload_message_receiver: broadcast::Receiver<ReloadEvent>,
    signal_handler_task: JoinHandle<()>,

    db_connection_pool: Arc<RwLock<MySqlPool>>,
    db_is_mariadb: Arc<RwLock<bool>>,
    listener: Arc<RwLock<TokioUnixListener>>,
    listener_task: JoinHandle<anyhow::Result<()>>,
    handler_task_tracker: TaskTracker,
    supervisor_message_sender: broadcast::Sender<SupervisorMessage>,

    watchdog_timeout: Option<Duration>,
    systemd_watchdog_task: Option<JoinHandle<()>>,

    status_notifier_task: Option<JoinHandle<()>>,
}

impl Supervisor {
    pub async fn new(config_path: PathBuf, systemd_mode: bool) -> anyhow::Result<Self> {
        tracing::debug!("Starting server supervisor");
        tracing::debug!(
            "Running in tokio with {} worker threads",
            tokio::runtime::Handle::current().metrics().num_workers()
        );

        let config = ServerConfig::read_config_from_path(&config_path)
            .context("Failed to read server configuration")?;

        let group_deny_list = if let Some(denylist_path) = &config.authorization.group_denylist_file
        {
            let denylist = read_and_parse_group_denylist(denylist_path)
                .context("Failed to read group denylist file")?;
            tracing::debug!(
                "Loaded group denylist with {} entries from {:?}",
                denylist.len(),
                denylist_path
            );
            Arc::new(RwLock::new(denylist))
        } else {
            tracing::debug!("No group denylist file specified, proceeding without a denylist");
            Arc::new(RwLock::new(GroupDenylist::new()))
        };

        let mut watchdog_duration = None;
        let mut watchdog_micro_seconds = 0;
        #[cfg(target_os = "linux")]
        let watchdog_task =
            if systemd_mode && sd_notify::watchdog_enabled(true, &mut watchdog_micro_seconds) {
                let watchdog_duration_ = Duration::from_micros(watchdog_micro_seconds);
                tracing::debug!(
                    "Systemd watchdog enabled with {} millisecond interval",
                    watchdog_micro_seconds.div_ceil(1000),
                );
                watchdog_duration = Some(watchdog_duration_);
                Some(spawn_watchdog_task(watchdog_duration_))
            } else {
                tracing::debug!("Systemd watchdog not enabled, skipping watchdog thread");
                None
            };
        #[cfg(not(target_os = "linux"))]
        let watchdog_task = None;

        let db_connection_pool =
            Arc::new(RwLock::new(create_db_connection_pool(&config.mysql).await?));

        let db_is_mariadb = {
            let connection = db_connection_pool.read().await;
            let version: String = sqlx::query_scalar("SELECT VERSION()")
                .fetch_one(&*connection)
                .await
                .context("Failed to query database version")?;

            let result = version.to_lowercase().contains("mariadb");
            tracing::debug!(
                "Connected to {} database server",
                if result { "MariaDB" } else { "MySQL" }
            );

            Arc::new(RwLock::new(result))
        };

        let task_tracker = TaskTracker::new();

        #[cfg(target_os = "linux")]
        let status_notifier_task = if systemd_mode {
            Some(spawn_status_notifier_task(task_tracker.clone()))
        } else {
            None
        };
        #[cfg(not(target_os = "linux"))]
        let status_notifier_task = None;

        let (tx, rx) = broadcast::channel(1);

        // TODO: try to detect systemd socket before using the provided socket path
        #[cfg(target_os = "linux")]
        let listener = Arc::new(RwLock::new(match config.socket_path {
            Some(ref path) => create_unix_listener_with_socket_path(path.clone()).await?,
            None => create_unix_listener_with_systemd_socket().await?,
        }));
        #[cfg(not(target_os = "linux"))]
        let listener = Arc::new(RwLock::new(
            create_unix_listener_with_socket_path(
                config
                    .socket_path
                    .as_ref()
                    .ok_or(anyhow!("Socket path must be set"))?
                    .clone(),
            )
            .await?,
        ));

        let (reload_tx, reload_rx) = broadcast::channel(1);
        let shutdown_cancel_token = CancellationToken::new();
        let signal_handler_task =
            spawn_signal_handler_task(reload_tx, shutdown_cancel_token.clone());

        let listener_clone = listener.clone();
        let task_tracker_clone = task_tracker.clone();
        let listener_task = {
            tokio::spawn(listener_task(
                listener_clone,
                task_tracker_clone,
                db_connection_pool.clone(),
                rx,
                db_is_mariadb.clone(),
                group_deny_list.clone(),
            ))
        };

        Ok(Self {
            config_path,
            config: Arc::new(Mutex::new(config)),
            group_deny_list,
            systemd_mode,
            reload_message_receiver: reload_rx,
            shutdown_cancel_token,
            signal_handler_task,
            db_connection_pool,
            db_is_mariadb,
            listener,
            listener_task,
            handler_task_tracker: task_tracker,
            supervisor_message_sender: tx,
            watchdog_timeout: watchdog_duration,
            systemd_watchdog_task: watchdog_task,
            status_notifier_task,
        })
    }

    fn stop_receiving_new_connections(&self) -> anyhow::Result<()> {
        self.handler_task_tracker.close();
        self.supervisor_message_sender
            .send(SupervisorMessage::StopAcceptingNewConnections)
            .context("Failed to send stop accepting new connections message to listener task")?;
        Ok(())
    }

    fn resume_receiving_new_connections(&self) -> anyhow::Result<()> {
        self.handler_task_tracker.reopen();
        self.supervisor_message_sender
            .send(SupervisorMessage::ResumeAcceptingNewConnections)
            .context("Failed to send resume accepting new connections message to listener task")?;
        Ok(())
    }

    async fn wait_for_existing_connections_to_finish(&self) -> anyhow::Result<()> {
        self.handler_task_tracker.wait().await;
        Ok(())
    }

    async fn reload_config(&self) -> anyhow::Result<()> {
        let new_config = ServerConfig::read_config_from_path(&self.config_path)
            .context("Failed to read server configuration")?;
        let mut config = self.config.clone().lock_owned().await;
        *config = new_config;

        let group_deny_list = if let Some(denylist_path) = &config.authorization.group_denylist_file
        {
            let denylist = read_and_parse_group_denylist(denylist_path)
                .context("Failed to read group denylist file")?;

            tracing::debug!(
                "Loaded group denylist with {} entries from {:?}",
                denylist.len(),
                denylist_path
            );
            denylist
        } else {
            tracing::debug!("No group denylist file specified, proceeding without a denylist");
            GroupDenylist::new()
        };
        let mut group_deny_list_lock = self.group_deny_list.write().await;
        *group_deny_list_lock = group_deny_list;
        Ok(())
    }

    async fn restart_db_connection_pool(&self) -> anyhow::Result<()> {
        let config = self.config.lock().await;
        let mut connection_pool = self.db_connection_pool.clone().write_owned().await;
        let mut db_is_mariadb_lock = self.db_is_mariadb.write().await;

        let new_db_pool = create_db_connection_pool(&config.mysql).await?;
        let db_is_mariadb = {
            let version: String = sqlx::query_scalar("SELECT VERSION()")
                .fetch_one(&new_db_pool)
                .await
                .context("Failed to query database version")?;

            let result = version.to_lowercase().contains("mariadb");
            tracing::debug!(
                "Connected to {} database server",
                if result { "MariaDB" } else { "MySQL" }
            );

            result
        };

        *connection_pool = new_db_pool;
        *db_is_mariadb_lock = db_is_mariadb;
        Ok(())
    }

    // NOTE: the listener task will block the write lock unless the task is cancelled
    //       first. Make sure to handle that appropriately to avoid a deadlock.
    async fn reload_listener(&self) -> anyhow::Result<()> {
        let config = self.config.lock().await;
        #[cfg(target_os = "linux")]
        let new_listener = match config.socket_path {
            Some(ref path) => create_unix_listener_with_socket_path(path.clone()).await?,
            None => create_unix_listener_with_systemd_socket().await?,
        };
        #[cfg(not(target_os = "linux"))]
        let new_listener = create_unix_listener_with_socket_path(
            config
                .socket_path
                .as_ref()
                .ok_or(anyhow!("Socket path must be set"))?
                .clone(),
        )
        .await?;

        let mut listener = self.listener.write().await;
        *listener = new_listener;
        Ok(())
    }

    pub async fn reload(&self) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        sd_notify::notify(
            false,
            &[
                sd_notify::NotifyState::Reloading,
                sd_notify::NotifyState::monotonic_usec_now()
                    .expect("Failed to get monotonic time to send to systemd while reloading"),
                sd_notify::NotifyState::Status("Reloading configuration"),
            ],
        )?;

        let previous_config = self.config.lock().await.clone();
        self.reload_config().await?;

        let mut listener_task_was_stopped = false;

        // NOTE: despite closing the existing db pool, any already acquired connections will remain valid until dropped,
        //       so we don't need to close existing connections here.
        if self.config.lock().await.mysql != previous_config.mysql {
            tracing::debug!("MySQL configuration has changed");

            tracing::debug!("Restarting database connection pool with new configuration");
            self.restart_db_connection_pool().await?;
        }

        if self.config.lock().await.socket_path != previous_config.socket_path {
            tracing::debug!("Socket path configuration has changed, reloading listener");
            if !listener_task_was_stopped {
                listener_task_was_stopped = true;
                tracing::debug!("Stop accepting new connections");
                self.stop_receiving_new_connections()?;

                tracing::debug!("Waiting for existing connections to finish");
                self.wait_for_existing_connections_to_finish().await?;
            }

            tracing::debug!("Reloading listener with new socket path");
            self.reload_listener().await?;
        }

        if listener_task_was_stopped {
            tracing::debug!("Resuming listener task");
            self.resume_receiving_new_connections()?;
        }

        #[cfg(target_os = "linux")]
        sd_notify::notify(false, &[sd_notify::NotifyState::Ready])?;

        Ok(())
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        sd_notify::notify(false, &[sd_notify::NotifyState::Stopping])?;

        tracing::debug!("Stop accepting new connections");
        self.stop_receiving_new_connections()?;

        let connection_count = self.handler_task_tracker.len();
        tracing::debug!(
            "Waiting for {} existing connections to finish",
            connection_count
        );
        self.wait_for_existing_connections_to_finish().await?;

        tracing::debug!("Shutting down listener task");
        self.supervisor_message_sender
            .send(SupervisorMessage::Shutdown)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to send shutdown message to listener task: {}", e);
                0
            });

        tracing::debug!("Shutting down database connection pool");
        self.db_connection_pool.read().await.close().await;

        tracing::debug!("Server shutdown complete");

        std::process::exit(0);
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            select! {
                biased;

                _ = async {
                  let mut rx = self.reload_message_receiver.resubscribe();
                  rx.recv().await
                } => {
                    tracing::info!("Reloading configuration");
                    match self.reload().await {
                        Ok(()) => {
                            tracing::info!("Configuration reloaded successfully");
                        }
                        Err(e) => {
                            tracing::error!("Failed to reload configuration: {}", e);
                        }
                    }
                }

                () = self.shutdown_cancel_token.cancelled() => {
                    tracing::info!("Shutting down server");
                    self.shutdown().await?;
                    break;
                }
            }
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn spawn_watchdog_task(duration: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = interval(duration.div_f32(2.0));
        tracing::debug!(
            "Starting systemd watchdog task, pinging every {} milliseconds",
            duration.div_f32(2.0).as_millis()
        );
        loop {
            interval.tick().await;
            if let Err(err) = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]) {
                tracing::warn!("Failed to notify systemd watchdog: {}", err);
            }
        }
    })
}

#[cfg(target_os = "linux")]
fn spawn_status_notifier_task(task_tracker: TaskTracker) -> JoinHandle<()> {
    const STATUS_UPDATE_INTERVAL_SECS: Duration = Duration::from_secs(1);

    tokio::spawn(async move {
        let mut interval = interval(STATUS_UPDATE_INTERVAL_SECS);
        loop {
            interval.tick().await;
            let count = task_tracker.len();

            let message = if count > 0 {
                format!("Handling {count} connections")
            } else {
                "Waiting for connections".to_string()
            };

            if let Err(e) =
                sd_notify::notify(false, &[sd_notify::NotifyState::Status(message.as_str())])
            {
                tracing::warn!("Failed to send systemd status notification: {}", e);
            }
        }
    })
}

async fn create_unix_listener_with_socket_path(
    socket_path: PathBuf,
) -> anyhow::Result<TokioUnixListener> {
    let parent_directory = socket_path.parent().unwrap();
    if !parent_directory.exists() {
        tracing::debug!("Creating directory {:?}", parent_directory);
        fs::create_dir_all(parent_directory)?;
    }

    tracing::info!("Listening on socket {:?}", socket_path);

    match fs::remove_file(socket_path.as_path()) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    let listener = TokioUnixListener::bind(socket_path)?;

    Ok(listener)
}

#[cfg(target_os = "linux")]
async fn create_unix_listener_with_systemd_socket() -> anyhow::Result<TokioUnixListener> {
    let fd = sd_notify::listen_fds()
        .context("Failed to get file descriptors from systemd")?
        .next()
        .context("No file descriptors received from systemd")?;

    debug_assert!(fd == 3, "Unexpected file descriptor from systemd: {fd}");

    tracing::debug!(
        "Received file descriptor from systemd with id: '{}', assuming socket",
        fd
    );

    let std_unix_listener = unsafe { StdUnixListener::from_raw_fd(fd) };
    std_unix_listener
        .set_nonblocking(true)
        .context("Failed to set non-blocking mode on systemd socket")?;
    let listener = TokioUnixListener::from_std(std_unix_listener)?;

    Ok(listener)
}

async fn create_db_connection_pool(config: &MysqlConfig) -> anyhow::Result<MySqlPool> {
    let mysql_config = config.as_mysql_connect_options()?;

    config.log_connection_notice();

    let pool = match tokio::time::timeout(
        Duration::from_secs(config.timeout),
        MySqlPool::connect_with(mysql_config),
    )
    .await
    {
        Ok(connection) => connection.context("Failed to connect to the database"),
        Err(_) => Err(anyhow!("Timed out after {} seconds", config.timeout))
            .context("Failed to connect to the database"),
    }?;

    let pool_opts = pool.options();
    tracing::debug!(
        "Successfully opened database connection pool with options (max_connections: {}, min_connections: {})",
        pool_opts.get_max_connections(),
        pool_opts.get_min_connections(),
    );

    Ok(pool)
}

fn spawn_signal_handler_task(
    reload_sender: broadcast::Sender<ReloadEvent>,
    shutdown_token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut sighup_stream =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                .expect("Failed to set up SIGHUP handler");
        let mut sigterm_stream =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to set up SIGTERM handler");

        loop {
            tokio::select! {
                _ = sighup_stream.recv() => {
                    tracing::info!("Received SIGHUP signal");
                    reload_sender.send(ReloadEvent).ok();
                }
                _ = sigterm_stream.recv() => {
                    tracing::info!("Received SIGTERM signal");
                    shutdown_token.cancel();
                    break;
                }
            }
        }
    })
}

async fn listener_task(
    listener: Arc<RwLock<TokioUnixListener>>,
    task_tracker: TaskTracker,
    db_pool: Arc<RwLock<MySqlPool>>,
    mut supervisor_message_receiver: broadcast::Receiver<SupervisorMessage>,
    db_is_mariadb: Arc<RwLock<bool>>,
    group_denylist: Arc<RwLock<GroupDenylist>>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    sd_notify::notify(false, &[sd_notify::NotifyState::Ready])?;

    let connection_counter = AtomicU64::new(0);

    loop {
        tokio::select! {
            biased;

            Ok(message) = supervisor_message_receiver.recv() => {
                match message {
                    SupervisorMessage::StopAcceptingNewConnections => {
                        tracing::info!("Listener task received stop accepting new connections message, stopping listener");
                        while let Ok(msg) = supervisor_message_receiver.try_recv() {
                            if let SupervisorMessage::ResumeAcceptingNewConnections = msg {
                                tracing::info!("Listener task received resume accepting new connections message, resuming listener");
                                break;
                            }
                        }
                    }
                    SupervisorMessage::Shutdown => {
                        tracing::info!("Listener task received shutdown message, exiting listener task");
                        break;
                    }
                    _ => {}
                }
            }

            accept_result = async {
                let listener = listener.read().await;
                listener.accept().await
            } => {
                match accept_result {
                    Ok((conn, _addr)) => {
                        connection_counter.fetch_add(1, Ordering::Relaxed);
                        let conn_id = connection_counter.load(Ordering::Relaxed);

                        tracing::debug!("Got new connection, assigned session ID {}", conn_id);

                        let session_id = SessionId::new(conn_id);
                        let db_pool_clone = db_pool.clone();
                        let db_is_mariadb_clone = *db_is_mariadb.read().await;
                        let group_denylist_arc_clone = group_denylist.clone();
                        task_tracker.spawn(async move {
                            match session_handler(
                                conn,
                                session_id,
                                db_pool_clone,
                                db_is_mariadb_clone,
                                &*group_denylist_arc_clone.read().await,
                            ).await {
                                Ok(()) => {},
                                Err(e) => tracing::error!("Session {} failed: {}", conn_id, e),
                            }
                        });
                    },
                    Err(e) => tracing::error!("Failed to accept new connection: {}", e),
                }
            }
        }
    }

    Ok(())
}
