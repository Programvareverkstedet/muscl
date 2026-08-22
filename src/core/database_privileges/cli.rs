//! This module contains serialization and deserialization logic for
//! database privileges related CLI commands.

use itertools::Itertools;

use super::diff::{DatabasePrivilegeChange, DatabasePrivilegeRowDiff};
use crate::core::types::{MySQLDatabase, MySQLUser};

const VALID_PRIVILEGE_EDIT_CHARS: &[char] = &[
    's', 'i', 'u', 'd', 'c', 'D', 'a', 'I', 't', 'l', 'r', 'v', 'V', 'T', 'A',
];

/// This enum represents a part of a CLI argument for editing database privileges,
/// indicating whether privileges are to be added, set, or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabasePrivilegeEditEntryType {
    Add,
    Set,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabasePrivilegeEdit {
    pub type_: DatabasePrivilegeEditEntryType,
    pub privileges: Vec<char>,
}

impl DatabasePrivilegeEdit {
    pub fn parse_from_str(input: &str) -> anyhow::Result<Self> {
        let (edit_type, privs_str) = if let Some(privs_str) = input.strip_prefix('+') {
            (DatabasePrivilegeEditEntryType::Add, privs_str)
        } else if let Some(privs_str) = input.strip_prefix('-') {
            (DatabasePrivilegeEditEntryType::Remove, privs_str)
        } else {
            (DatabasePrivilegeEditEntryType::Set, input)
        };

        let privileges: Vec<char> = privs_str.chars().collect();

        if privileges
            .iter()
            .any(|c| !VALID_PRIVILEGE_EDIT_CHARS.contains(c))
        {
            let invalid_chars: String = privileges
                .iter()
                .filter(|c| !VALID_PRIVILEGE_EDIT_CHARS.contains(c))
                .map(|c| format!("'{c}'"))
                .join(", ");
            let valid_characters: String = VALID_PRIVILEGE_EDIT_CHARS
                .iter()
                .map(|c| format!("'{c}'"))
                .join(", ");
            anyhow::bail!(
                "Invalid character(s) in privilege edit entry: {invalid_chars}\n\nValid characters are: {valid_characters}",
            );
        }

        Ok(DatabasePrivilegeEdit {
            type_: edit_type,
            privileges,
        })
    }
}

impl std::fmt::Display for DatabasePrivilegeEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.type_ {
            DatabasePrivilegeEditEntryType::Add => write!(f, "+")?,
            DatabasePrivilegeEditEntryType::Set => {}
            DatabasePrivilegeEditEntryType::Remove => write!(f, "-")?,
        }
        for priv_char in &self.privileges {
            write!(f, "{priv_char}")?;
        }

        Ok(())
    }
}

/// This struct represents a single CLI argument for editing database privileges.
///
/// This is typically parsed from a string looking like:
///
///   `database_name:username:[+|-]privileges`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabasePrivilegeEditEntry {
    pub database: MySQLDatabase,
    pub user: MySQLUser,
    pub privilege_edit: DatabasePrivilegeEdit,
}

impl DatabasePrivilegeEditEntry {
    /// Parses a privilege edit entry from a string.
    ///
    /// The expected format is:
    ///
    ///   `database_name:username:[+|-]privileges`
    ///
    /// where:
    /// - `database_name` is the name of the database to edit privileges for
    /// - username is the name of the user to edit privileges for
    /// - privileges is a string of characters representing the privileges to add, set or remove
    /// - the `+` or `-` prefix indicates whether to add or remove the privileges, if omitted the privileges are set directly
    /// - privileges characters are: siudcDaItlrvVTA
    pub fn parse_from_str(arg: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = arg.split(':').collect();
        if parts.len() != 3 {
            anyhow::bail!("Invalid privilege edit entry format: {arg}");
        }

        let (database, user, user_privs) = (parts[0].to_string(), parts[1].to_string(), parts[2]);

        if user.is_empty() {
            anyhow::bail!("Username cannot be empty in privilege edit entry: {arg}");
        }

        let privilege_edit = DatabasePrivilegeEdit::parse_from_str(user_privs)?;

        Ok(DatabasePrivilegeEditEntry {
            database: MySQLDatabase::from(database),
            user: MySQLUser::from(user),
            privilege_edit,
        })
    }

    pub fn as_database_privileges_diff(&self) -> anyhow::Result<DatabasePrivilegeRowDiff> {
        let mut diff;
        match self.privilege_edit.type_ {
            DatabasePrivilegeEditEntryType::Set => {
                diff = DatabasePrivilegeRowDiff {
                    db: self.database.clone(),
                    user: self.user.clone(),
                    select_priv: Some(DatabasePrivilegeChange::YesToNo),
                    insert_priv: Some(DatabasePrivilegeChange::YesToNo),
                    update_priv: Some(DatabasePrivilegeChange::YesToNo),
                    delete_priv: Some(DatabasePrivilegeChange::YesToNo),
                    create_priv: Some(DatabasePrivilegeChange::YesToNo),
                    drop_priv: Some(DatabasePrivilegeChange::YesToNo),
                    alter_priv: Some(DatabasePrivilegeChange::YesToNo),
                    index_priv: Some(DatabasePrivilegeChange::YesToNo),
                    create_tmp_table_priv: Some(DatabasePrivilegeChange::YesToNo),
                    lock_tables_priv: Some(DatabasePrivilegeChange::YesToNo),
                    references_priv: Some(DatabasePrivilegeChange::YesToNo),
                    create_view_priv: Some(DatabasePrivilegeChange::YesToNo),
                    show_view_priv: Some(DatabasePrivilegeChange::YesToNo),
                    trigger_priv: Some(DatabasePrivilegeChange::YesToNo),
                };
                for priv_char in &self.privilege_edit.privileges {
                    match priv_char {
                        's' => diff.select_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'i' => diff.insert_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'u' => diff.update_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'd' => diff.delete_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'c' => diff.create_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'D' => diff.drop_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'a' => diff.alter_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'I' => diff.index_priv = Some(DatabasePrivilegeChange::NoToYes),
                        't' => diff.create_tmp_table_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'l' => diff.lock_tables_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'r' => diff.references_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'v' => diff.create_view_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'V' => diff.show_view_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'T' => diff.trigger_priv = Some(DatabasePrivilegeChange::NoToYes),
                        'A' => {
                            diff.select_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.insert_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.update_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.delete_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.create_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.drop_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.alter_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.index_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.create_tmp_table_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.lock_tables_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.references_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.create_view_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.show_view_priv = Some(DatabasePrivilegeChange::NoToYes);
                            diff.trigger_priv = Some(DatabasePrivilegeChange::NoToYes);
                        }
                        _ => unreachable!(),
                    }
                }
            }
            DatabasePrivilegeEditEntryType::Add | DatabasePrivilegeEditEntryType::Remove => {
                diff = DatabasePrivilegeRowDiff {
                    db: self.database.clone(),
                    user: self.user.clone(),
                    select_priv: None,
                    insert_priv: None,
                    update_priv: None,
                    delete_priv: None,
                    create_priv: None,
                    drop_priv: None,
                    alter_priv: None,
                    index_priv: None,
                    create_tmp_table_priv: None,
                    lock_tables_priv: None,
                    references_priv: None,
                    create_view_priv: None,
                    show_view_priv: None,
                    trigger_priv: None,
                };
                let value = match self.privilege_edit.type_ {
                    DatabasePrivilegeEditEntryType::Add => DatabasePrivilegeChange::NoToYes,
                    DatabasePrivilegeEditEntryType::Remove => DatabasePrivilegeChange::YesToNo,
                    _ => unreachable!(),
                };
                for priv_char in &self.privilege_edit.privileges {
                    match priv_char {
                        's' => diff.select_priv = Some(value),
                        'i' => diff.insert_priv = Some(value),
                        'u' => diff.update_priv = Some(value),
                        'd' => diff.delete_priv = Some(value),
                        'c' => diff.create_priv = Some(value),
                        'D' => diff.drop_priv = Some(value),
                        'a' => diff.alter_priv = Some(value),
                        'I' => diff.index_priv = Some(value),
                        't' => diff.create_tmp_table_priv = Some(value),
                        'l' => diff.lock_tables_priv = Some(value),
                        'r' => diff.references_priv = Some(value),
                        'v' => diff.create_view_priv = Some(value),
                        'V' => diff.show_view_priv = Some(value),
                        'T' => diff.trigger_priv = Some(value),
                        'A' => {
                            diff.select_priv = Some(value);
                            diff.insert_priv = Some(value);
                            diff.update_priv = Some(value);
                            diff.delete_priv = Some(value);
                            diff.create_priv = Some(value);
                            diff.drop_priv = Some(value);
                            diff.alter_priv = Some(value);
                            diff.index_priv = Some(value);
                            diff.create_tmp_table_priv = Some(value);
                            diff.lock_tables_priv = Some(value);
                            diff.references_priv = Some(value);
                            diff.create_view_priv = Some(value);
                            diff.show_view_priv = Some(value);
                            diff.trigger_priv = Some(value);
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }

        Ok(diff)
    }
}

impl std::fmt::Display for DatabasePrivilegeEditEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:, ", self.database)?;
        write!(f, "{}: ", self.user)?;
        write!(f, "{}", self.privilege_edit)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_arg_parse_set_db_user_all() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("db:user:A");
        assert_eq!(
            result.ok(),
            Some(DatabasePrivilegeEditEntry {
                database: "db".into(),
                user: "user".into(),
                privilege_edit: DatabasePrivilegeEdit {
                    type_: DatabasePrivilegeEditEntryType::Set,
                    privileges: vec!['A'],
                },
            })
        );
    }

    #[test]
    fn test_cli_arg_parse_set_db_user_none() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("db:user:");
        assert_eq!(
            result.ok(),
            Some(DatabasePrivilegeEditEntry {
                database: "db".into(),
                user: "user".into(),
                privilege_edit: DatabasePrivilegeEdit {
                    type_: DatabasePrivilegeEditEntryType::Set,
                    privileges: vec![],
                },
            })
        );
    }

    #[test]
    fn test_cli_arg_parse_set_db_user_misc() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("db:user:siud");
        assert_eq!(
            result.ok(),
            Some(DatabasePrivilegeEditEntry {
                database: "db".into(),
                user: "user".into(),
                privilege_edit: DatabasePrivilegeEdit {
                    type_: DatabasePrivilegeEditEntryType::Set,
                    privileges: vec!['s', 'i', 'u', 'd'],
                },
            })
        );
    }

    #[test]
    fn test_cli_arg_parse_set_db_user_nonexistent_privilege() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("db:user:F");
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_arg_parse_set_user_empty_string() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("::");
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_arg_parse_set_db_user_empty_string() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("db::");
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_arg_parse_add_db_user_misc() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("db:user:+siud");
        assert_eq!(
            result.ok(),
            Some(DatabasePrivilegeEditEntry {
                database: "db".into(),
                user: "user".into(),
                privilege_edit: DatabasePrivilegeEdit {
                    type_: DatabasePrivilegeEditEntryType::Add,
                    privileges: vec!['s', 'i', 'u', 'd'],
                },
            })
        );
    }

    #[test]
    fn test_cli_arg_parse_remove_db_user_misc() {
        let result = DatabasePrivilegeEditEntry::parse_from_str("db:user:-siud");
        assert_eq!(
            result.ok(),
            Some(DatabasePrivilegeEditEntry {
                database: "db".into(),
                user: "user".into(),
                privilege_edit: DatabasePrivilegeEdit {
                    type_: DatabasePrivilegeEditEntryType::Remove,
                    privileges: vec!['s', 'i', 'u', 'd'],
                },
            }),
        );
    }
}
