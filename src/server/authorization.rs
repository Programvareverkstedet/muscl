use std::{path::Path, str::Lines};

use anyhow::Context;
use nix::unistd::Group;

use crate::core::{
    common::UnixUser,
    protocol::{
        CheckAuthorizationError,
        request_validation::{GroupDenylist, GroupNamePattern, validate_db_or_user_request},
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

/// Reads and parses a group denylist file.
///
/// The format of the denylist file is expected to be one group name or GID per line.
/// Lines starting with '#' are treated as comments and ignored.
/// Empty lines are also ignored.
///
/// Each line looks like one of the following:
/// - `gid:1001`
/// - `group:admins`
///
/// Note that the latter form supports wildcards `*` and `?`.
///
/// Non-wildcard group names are resolved to their GID immediately.
pub fn read_and_parse_group_denylist(denylist_path: &Path) -> anyhow::Result<GroupDenylist> {
    let content = std::fs::read_to_string(denylist_path)
        .context(format!("Failed to read denylist file at {denylist_path:?}"))?;

    let lines = content.lines();

    let groups = parse_group_denylist(denylist_path, lines);

    Ok(groups)
}

fn parse_group_denylist(denylist_path: &Path, lines: Lines) -> GroupDenylist {
    let mut groups = GroupDenylist::new();

    for (line_number, line) in lines.enumerate() {
        let trimmed_line = if let Some(comment_start) = line.find('#') {
            &line[..comment_start]
        } else {
            line
        }
        .trim();

        if trimmed_line.is_empty() {
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

                groups.insert_gid(group.gid.as_raw());
            }
            "group" if parts[1].contains(['*', '?']) => {
                let pattern = GroupNamePattern::new(parts[1]);
                match pattern.to_regex() {
                    Ok(_) => groups.insert_name_pattern(pattern),
                    Err(err) => {
                        tracing::warn!(
                            "Invalid wildcard pattern '{}' in denylist file at {:?} on line {}: {}",
                            parts[1],
                            denylist_path,
                            line_number + 1,
                            err
                        );
                    }
                }
            }
            "group" => match Group::from_name(parts[1]) {
                Ok(Some(group)) => {
                    groups.insert_gid(group.gid.as_raw());
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

    groups
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    fn fake_group(name: &str, gid: u32) -> Group {
        Group {
            name: name.to_owned(),
            passwd: std::ffi::CString::default(),
            gid: nix::unistd::Gid::from_raw(gid),
            mem: Vec::new(),
        }
    }

    #[test]
    fn test_parse_group_denylist() {
        let denylist_content = indoc! {"
            # Valid entries
            gid:0 # This is usually the 'root' group
            group:root # This is also the 'root' group, should deduplicate

            # Invalid entries
            invalid_line
            gid:not_a_number
            group:nonexistent_group
        "};

        let lines = denylist_content.lines();
        let group_denylist = parse_group_denylist(Path::new("test_denylist"), lines);

        assert_eq!(group_denylist.len(), 1);
        assert!(group_denylist.matches(&fake_group("root", 0)));
    }

    #[test]
    fn test_parse_group_denylist_wildcard() {
        let denylist_content = indoc! {"
            group:admin*
            group:svc-?db
        "};

        let lines = denylist_content.lines();
        let group_denylist = parse_group_denylist(Path::new("test_denylist"), lines);

        assert_eq!(group_denylist.len(), 2);

        assert!(group_denylist.matches(&fake_group("admin", 100)));
        assert!(group_denylist.matches(&fake_group("admins", 101)));
        assert!(!group_denylist.matches(&fake_group("badmin", 102)));

        assert!(group_denylist.matches(&fake_group("svc-1db", 103)));
        assert!(!group_denylist.matches(&fake_group("svc-12db", 104)));
        assert!(!group_denylist.matches(&fake_group("other", 105)));
    }

    #[test]
    fn test_wildcards_not_supported_for_gid() {
        let denylist_content = indoc! {"
            gid:*
        "};

        let lines = denylist_content.lines();
        let group_denylist = parse_group_denylist(Path::new("test_denylist"), lines);

        assert!(group_denylist.is_empty());
    }

    #[test]
    fn test_parse_group_denylist_wildcard_only_entry_star() {
        let denylist_content = indoc! {"
            group:*
        "};

        let lines = denylist_content.lines();
        let group_denylist = parse_group_denylist(Path::new("test_denylist"), lines);

        assert_eq!(group_denylist.len(), 1);

        // `group:*` matches any group name, including empty and multi-character ones.
        assert!(group_denylist.matches(&fake_group("", 100)));
        assert!(group_denylist.matches(&fake_group("a", 101)));
        assert!(group_denylist.matches(&fake_group("anything", 102)));
    }

    #[test]
    fn test_parse_group_denylist_wildcard_only_entry_question_mark() {
        let denylist_content = indoc! {"
            group:?
        "};

        let lines = denylist_content.lines();
        let group_denylist = parse_group_denylist(Path::new("test_denylist"), lines);

        assert_eq!(group_denylist.len(), 1);

        // `group:?` matches any single-character group name only.
        assert!(!group_denylist.matches(&fake_group("", 103)));
        assert!(group_denylist.matches(&fake_group("a", 104)));
        assert!(!group_denylist.matches(&fake_group("ab", 105)));
    }
}
