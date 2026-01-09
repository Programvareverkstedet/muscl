use std::{collections::HashSet, path::Path};

use anyhow::Context;
use nix::unistd::Group;

use crate::core::{
    common::UnixUser,
    protocol::{
        CheckAuthorizationError,
        request_validation::{GroupDenylist, validate_db_or_user_request},
    },
    types::DbOrUser,
};

pub async fn check_authorization(
    dbs_or_users: &[DbOrUser],
    unix_user: &UnixUser,
    group_denylist: &GroupDenylist,
) -> std::collections::BTreeMap<DbOrUser, Result<(), CheckAuthorizationError>> {
    dbs_or_users
        .iter()
        .cloned()
        .map(|db_or_user| {
            let result = validate_db_or_user_request(&db_or_user, unix_user, group_denylist)
                .map_err(CheckAuthorizationError);
            (db_or_user, result)
        })
        .collect()
}

/// Reads and parses a group denylist file, returning a set of GUIDs
///
/// The format of the denylist file is expected to be one group name or GID per line.
/// Lines starting with '#' are treated as comments and ignored.
/// Empty lines are also ignored.
///
/// Each line looks like one of the following:
/// - `gid:1001`
/// - `group:admins`
pub fn read_and_parse_group_denylist(denylist_path: &Path) -> anyhow::Result<GroupDenylist> {
    let content = std::fs::read_to_string(denylist_path)
        .context(format!("Failed to read denylist file at {denylist_path:?}"))?;

    let mut groups = HashSet::with_capacity(content.lines().count());

    for (line_number, line) in content.lines().enumerate() {
        let trimmed_line = line.trim();

        if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = trimmed_line.splitn(2, ':').collect();
        if parts.len() != 2 {
            tracing::warn!(
                "Invalid format in denylist file at {:?} on line {}: {}",
                denylist_path,
                line_number + 1,
                line
            );
            continue;
        }

        match parts[0] {
            "gid" => {
                let gid: u32 = match parts[1].parse() {
                    Ok(gid) => gid,
                    Err(err) => {
                        tracing::warn!(
                            "Invalid GID '{}' in denylist file at {:?} on line {}: {}",
                            parts[1],
                            denylist_path,
                            line_number + 1,
                            err
                        );
                        continue;
                    }
                };
                let group = match Group::from_gid(nix::unistd::Gid::from_raw(gid)) {
                    Ok(Some(g)) => g,
                    Ok(None) => {
                        tracing::warn!(
                            "No group found for GID {} in denylist file at {:?} on line {}",
                            gid,
                            denylist_path,
                            line_number + 1
                        );
                        continue;
                    }
                    Err(err) => {
                        tracing::warn!(
                            "Failed to get group for GID {} in denylist file at {:?} on line {}: {}",
                            gid,
                            denylist_path,
                            line_number + 1,
                            err
                        );
                        continue;
                    }
                };

                groups.insert(group.gid.as_raw());
            }
            "group" => match Group::from_name(parts[1]) {
                Ok(Some(group)) => {
                    groups.insert(group.gid.as_raw());
                }
                Ok(None) => {
                    tracing::warn!(
                        "No group found for name '{}' in denylist file at {:?} on line {}",
                        parts[1],
                        denylist_path,
                        line_number + 1
                    );
                    continue;
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to get group for name '{}' in denylist file at {:?} on line {}: {}",
                        parts[1],
                        denylist_path,
                        line_number + 1,
                        err
                    );
                }
            },
            _ => {
                tracing::warn!(
                    "Invalid prefix '{}' in denylist file at {:?} on line {}: {}",
                    parts[0],
                    denylist_path,
                    line_number + 1,
                    line
                );
                continue;
            }
        }
    }

    Ok(groups)
}
