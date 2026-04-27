//! This module contains some base datastructures and functionality for dealing with
//! database privileges in `MySQL`.

use std::fmt;

use crate::core::types::{MySQLDatabase, MySQLUser};
use serde::{Deserialize, Serialize};

/// This is the list of fields that are used to fetch the db + user + privileges
/// from the `db` table in the database. If you need to add or remove privilege
/// fields, this is a good place to start.
pub const DATABASE_PRIVILEGE_FIELDS: [&str; 13] = [
    "Db",
    "User",
    "select_priv",
    "insert_priv",
    "update_priv",
    "delete_priv",
    "create_priv",
    "drop_priv",
    "alter_priv",
    "index_priv",
    "create_tmp_table_priv",
    "lock_tables_priv",
    "references_priv",
];

// NOTE: ord is needed for BTreeSet to accept the type, but it
//       doesn't have any natural implementation semantics.

/// Representation of the set of privileges for a single user on a single database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Default)]
pub struct DatabasePrivilegeRow {
    // TODO: don't store the db and user here, let the type be stored in a mapping
    pub db: MySQLDatabase,
    pub user: MySQLUser,
    pub select_priv: bool,
    pub insert_priv: bool,
    pub update_priv: bool,
    pub delete_priv: bool,
    pub create_priv: bool,
    pub drop_priv: bool,
    pub alter_priv: bool,
    pub index_priv: bool,
    pub create_tmp_table_priv: bool,
    pub lock_tables_priv: bool,
    pub references_priv: bool,
}

impl DatabasePrivilegeRow {
    /// Gets the value of a privilege by its name as a &str.
    #[must_use]
    pub fn get_privilege_by_name(&self, name: &str) -> Option<bool> {
        match name {
            "select_priv" => Some(self.select_priv),
            "insert_priv" => Some(self.insert_priv),
            "update_priv" => Some(self.update_priv),
            "delete_priv" => Some(self.delete_priv),
            "create_priv" => Some(self.create_priv),
            "drop_priv" => Some(self.drop_priv),
            "alter_priv" => Some(self.alter_priv),
            "index_priv" => Some(self.index_priv),
            "create_tmp_table_priv" => Some(self.create_tmp_table_priv),
            "lock_tables_priv" => Some(self.lock_tables_priv),
            "references_priv" => Some(self.references_priv),
            _ => None,
        }
    }
}

impl fmt::Display for DatabasePrivilegeRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for field in DATABASE_PRIVILEGE_FIELDS.into_iter().skip(2) {
            if self.get_privilege_by_name(field).unwrap() {
                f.write_str(db_priv_field_human_readable_name(field).as_str())?;
                f.write_str(": Y\n")?;
            } else {
                f.write_str(db_priv_field_human_readable_name(field).as_str())?;
                f.write_str(": N\n")?;
            }
        }
        Ok(())
    }
}

/// Converts a database privilege field name to a human-readable name.
#[must_use]
pub fn db_priv_field_human_readable_name(name: &str) -> String {
    match name {
        "Db" => "Database".to_owned(),
        "User" => "User".to_owned(),
        "select_priv" => "Select".to_owned(),
        "insert_priv" => "Insert".to_owned(),
        "update_priv" => "Update".to_owned(),
        "delete_priv" => "Delete".to_owned(),
        "create_priv" => "Create".to_owned(),
        "drop_priv" => "Drop".to_owned(),
        "alter_priv" => "Alter".to_owned(),
        "index_priv" => "Index".to_owned(),
        "create_tmp_table_priv" => "Temp".to_owned(),
        "lock_tables_priv" => "Lock".to_owned(),
        "references_priv" => "References".to_owned(),
        _ => format!("Unknown({name})"),
    }
}

/// Converts a database privilege field name to a single-character name.
/// (the characters from the cli privilege editor)
#[must_use]
pub fn db_priv_field_single_character_name(name: &str) -> &str {
    match name {
        "select_priv" => "s",
        "insert_priv" => "i",
        "update_priv" => "u",
        "delete_priv" => "d",
        "create_priv" => "c",
        "drop_priv" => "D",
        "alter_priv" => "a",
        "index_priv" => "I",
        "create_tmp_table_priv" => "t",
        "lock_tables_priv" => "l",
        "references_priv" => "r",
        _ => "?",
    }
}
