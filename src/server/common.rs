use crate::core::{common::UnixUser, protocol::request_validation::GroupDenylist};
use nix::unistd::Group;
use sqlx::prelude::*;

/// This function retrieves the groups of a user, filtering out any groups
/// that are present in the provided denylist.
pub fn get_user_filtered_groups(user: &UnixUser, group_denylist: &GroupDenylist) -> Vec<String> {
    user.groups
        .iter()
        .cloned()
        .filter_map(|group_name| {
            match Group::from_name(&group_name) {
                Ok(Some(group)) => {
                    if group_denylist.matches(&group) {
                        None
                    } else {
                        Some(group.name)
                    }
                }
                // NOTE: allow non-existing groups to pass through the filter
                _ => Some(group_name),
            }
        })
        .collect()
}

/// This function creates a regex that matches items (users, databases)
/// that belong to the user or any of the user's groups.
pub fn create_user_group_matching_regex(user: &UnixUser, group_denylist: &GroupDenylist) -> String {
    let escaped_username = regex::escape(&user.username);
    let filtered_groups = get_user_filtered_groups(user, group_denylist);
    if filtered_groups.is_empty() {
        format!("{escaped_username}_.+")
    } else {
        let escaped_groups = filtered_groups
            .iter()
            .map(|group| regex::escape(group))
            .collect::<Vec<_>>()
            .join("|");
        format!("({escaped_username}|{escaped_groups})_.+")
    }
}

/// Some mysql versions with some collations mark some columns as binary fields,
/// which in the current version of sqlx is not parsable as string.
/// See: <https://github.com/launchbadge/sqlx/issues/3387>
#[inline]
pub fn try_get_with_binary_fallback(
    row: &sqlx::mysql::MySqlRow,
    column: &str,
) -> Result<String, sqlx::Error> {
    row.try_get(column).or_else(|_| {
        row.try_get::<Vec<u8>, _>(column)
            .map(|v| String::from_utf8_lossy(&v).to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    #[test]
    fn test_create_user_group_matching_regex() {
        let user = UnixUser {
            username: "user".to_owned(),
            groups: vec!["group1".to_owned(), "group2".to_owned()],
        };

        let regex = create_user_group_matching_regex(&user, &GroupDenylist::new());
        println!("Generated regex: {}", regex);
        let re = Regex::new(&regex).unwrap();

        assert!(re.is_match("user_something"));
        assert!(re.is_match("group1_something"));
        assert!(re.is_match("group2_something"));

        assert!(!re.is_match("other_something"));
        assert!(!re.is_match("user"));
        assert!(!re.is_match("usersomething"));
    }
}
