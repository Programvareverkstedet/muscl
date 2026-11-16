use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::{
    ConnectOptions, Executor,
    mysql::{MySqlConnectOptions, MySqlPoolOptions},
};

pub const DEFAULT_PORT: u16 = 3306;
fn default_mysql_port() -> u16 {
    DEFAULT_PORT
}

pub const DEFAULT_TIMEOUT: u64 = 2;
fn default_mysql_timeout() -> u64 {
    DEFAULT_TIMEOUT
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename = "mysql")]
pub struct MysqlConfig {
    pub socket_path: Option<PathBuf>,
    pub host: Option<String>,
    #[serde(default = "default_mysql_port")]
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub password_file: Option<PathBuf>,
    #[serde(default = "default_mysql_timeout")]
    pub timeout: u64,
}

impl MysqlConfig {
    pub fn as_mysql_connect_options(&self) -> anyhow::Result<MySqlConnectOptions> {
        let mut options = MySqlConnectOptions::new()
            .database("mysql")
            .log_statements(tracing::log::LevelFilter::Trace);

        if let Some(username) = &self.username {
            options = options.username(username);
        }

        if let Some(password_file) = &self.password_file {
            let password = fs::read_to_string(password_file)
                .with_context(|| {
                    format!("Failed to read MySQL password file at {password_file:?}")
                })?
                .trim()
                .to_owned();
            options = options.password(&password);
        } else if let Some(password) = &self.password {
            options = options.password(password);
        }

        if let Some(socket_path) = &self.socket_path {
            options = options.socket(socket_path);
        } else if let Some(host) = &self.host {
            options = options.host(host);
            options = options.port(self.port);
        } else {
            anyhow::bail!("No MySQL host or socket path provided");
        }

        Ok(options)
    }

    /// Pool options that remove `NO_BACKSLASH_ESCAPES` from the `sql_mode`,
    /// which our SQL string literals depend on.
    pub fn mysql_pool_options() -> MySqlPoolOptions {
        // NOTE: this statement must not contain backslashes.
        const SQL: &str = "SET SESSION sql_mode = (SELECT TRIM(BOTH ',' FROM \
            REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), ',NO_BACKSLASH_ESCAPES,', ',')))";

        MySqlPoolOptions::new().after_connect(|connection, _| {
            Box::pin(async move { connection.execute(SQL).await.map(|_| ()) })
        })
    }

    pub fn log_connection_notice(&self) {
        let mut display_config = self.to_owned();
        display_config.password = display_config
            .password
            .as_ref()
            .map(|_| "<REDACTED>".to_owned());
        tracing::debug!(
            "Connecting to MySQL server with parameters: {:#?}",
            display_config
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AuthorizationConfig {
    pub group_denylist_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerConfig {
    pub socket_path: Option<PathBuf>,
    pub authorization: AuthorizationConfig,
    pub mysql: MysqlConfig,
}

impl ServerConfig {
    /// Reads the server configuration from the specified path, or the default path if none is provided.
    pub fn read_config_from_path(config_path: &Path) -> anyhow::Result<Self> {
        tracing::debug!("Reading config file at {:?}", config_path);

        fs::read_to_string(config_path)
            .context(format!("Failed to read config file at {config_path:?}"))
            .and_then(|c| toml::from_str(&c).context("Failed to parse config file"))
            .context(format!("Failed to parse config file at {config_path:?}"))
    }
}
