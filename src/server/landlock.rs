#[cfg(target_os = "linux")]
use std::path::Path;

#[cfg(target_os = "linux")]
pub fn landlock_restrict_server(config_path: Option<&Path>) -> anyhow::Result<()> {
    use crate::{core::common::DEFAULT_CONFIG_PATH, server::config::ServerConfig};
    use anyhow::Context;
    use landlock::{
        ABI, Access, AccessFs, AccessNet, NetPort, Ruleset, RulesetAttr, RulesetCreatedAttr,
        path_beneath_rules,
    };

    let config_path = config_path.unwrap_or(Path::new(DEFAULT_CONFIG_PATH));

    let config = ServerConfig::read_config_from_path(config_path)?;

    let abi = ABI::V4;
    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .handle_access(AccessNet::from_all(abi))?
        .create()
        .context("Failed to create Landlock ruleset")?
        .add_rules(path_beneath_rules(
            &["/run/muscl"],
            AccessFs::from_read(abi),
        ))
        .context("Failed to add Landlock rules for /run/muscl")?
        // Needs read access to /etc to access unix user/group info
        .add_rules(path_beneath_rules(&["/etc"], AccessFs::from_read(abi)))
        .context("Failed to add Landlock rules for /etc")?
        .add_rules(path_beneath_rules(&[config_path], AccessFs::from_read(abi)))
        .context(format!(
            "Failed to add Landlock rules for server config path at {}",
            config_path.display()
        ))?;

    if let Some(socket_path) = &config.socket_path {
        ruleset = ruleset
            .add_rules(path_beneath_rules(&[socket_path], AccessFs::from_all(abi)))
            .context(format!(
                "Failed to add Landlock rules for server socket path at {}",
                socket_path.display()
            ))?;
    }

    if let Some(mysql_socket_path) = &config.mysql.socket_path {
        ruleset = ruleset
            .add_rules(path_beneath_rules(
                &[mysql_socket_path],
                AccessFs::from_all(abi),
            ))
            .context(format!(
                "Failed to add Landlock rules for MySQL socket path at {}",
                mysql_socket_path.display()
            ))?;
    }

    if let Some(mysql_host) = &config.mysql.host {
        ruleset = ruleset
            .add_rule(NetPort::new(config.mysql.port, AccessNet::ConnectTcp))
            .context(format!(
                "Failed to add Landlock rules for MySQL host at {}:{}",
                mysql_host, config.mysql.port
            ))?;
    }

    if let Some(mysql_passwd_file) = &config.mysql.password_file {
        ruleset = ruleset
            .add_rules(path_beneath_rules(
                &[mysql_passwd_file],
                AccessFs::from_read(abi),
            ))
            .context(format!(
                "Failed to add Landlock rules for MySQL password file at {}",
                mysql_passwd_file.display()
            ))?;
    }

    if let Some(group_denylist_file) = &config.authorization.group_denylist_file {
        ruleset = ruleset
            .add_rules(path_beneath_rules(
                &[group_denylist_file],
                AccessFs::from_read(abi),
            ))
            .context(format!(
                "Failed to add Landlock rules for group denylist file at {}",
                group_denylist_file.display()
            ))?;
    }

    ruleset
        .restrict_self()
        .context("Failed to apply Landlock restrictions to the server process")?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn landlock_restrict_server() -> anyhow::Result<()> {
    Ok(())
}
