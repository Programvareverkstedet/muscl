use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::{
    database_privileges::{DatabasePrivilegeRow, DatabasePrivilegeRowDiff, DatabasePrivilegesDiff},
    protocol::request_validation::ValidationError,
    types::{DbOrUser, MySQLDatabase, MySQLUser},
};

pub type ModifyPrivilegesRequest = BTreeSet<DatabasePrivilegesDiff>;

pub type ModifyPrivilegesResponse =
    BTreeMap<MySQLDatabase, BTreeMap<MySQLUser, Result<(), ModifyDatabasePrivilegesError>>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModifyDatabasePrivilegesError {
    #[error("Database validation error: {0}")]
    DatabaseValidationError(ValidationError),

    #[error("User validation error: {0}")]
    UserValidationError(ValidationError),

    #[error("Database does not exist")]
    DatabaseDoesNotExist,

    #[error("User does not exist")]
    UserDoesNotExist,

    #[error("Diff does not apply: {0}")]
    DiffDoesNotApply(Box<DiffDoesNotApplyError>),

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

#[allow(clippy::enum_variant_names)]
#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DiffDoesNotApplyError {
    #[error("Privileges row already exists for database '{0}' and user '{1}'")]
    RowAlreadyExists(MySQLDatabase, MySQLUser),

    #[error("Privileges row does not exist for database '{0}' and user '{1}'")]
    RowDoesNotExist(MySQLDatabase, MySQLUser),

    #[error("Privilege change '{0:?}' does not apply to row '{1:?}'")]
    RowPrivilegeChangeDoesNotApply(DatabasePrivilegeRowDiff, DatabasePrivilegeRow),
}

pub fn print_modify_database_privileges_output_status(output: &ModifyPrivilegesResponse) {
    for ((database_name, username), result) in output.iter().flat_map(|(db, user_map)| {
        user_map
            .iter()
            .map(move |(user, result)| ((db, user), result))
    }) {
        match result {
            Ok(()) => {
                println!(
                    "Privileges for user '{username}' on database '{database_name}' modified successfully."
                );
            }
            Err(err) => {
                eprintln!("{}", err.to_error_message(database_name, username));
                eprintln!("Skipping...");
            }
        }
        println!();
    }
}

impl ModifyDatabasePrivilegesError {
    #[must_use]
    pub fn to_error_message(&self, database_name: &MySQLDatabase, username: &MySQLUser) -> String {
        match self {
            ModifyDatabasePrivilegesError::DatabaseValidationError(err) => {
                err.to_error_message(&DbOrUser::Database(database_name.clone()))
            }
            ModifyDatabasePrivilegesError::UserValidationError(err) => {
                err.to_error_message(&DbOrUser::User(username.clone()))
            }
            ModifyDatabasePrivilegesError::DatabaseDoesNotExist => {
                format!("Database '{database_name}' does not exist.")
            }
            ModifyDatabasePrivilegesError::UserDoesNotExist => {
                format!("User '{username}' does not exist.")
            }
            ModifyDatabasePrivilegesError::DiffDoesNotApply(diff) => {
                format!(
                    "Could not apply privilege change:\n{}",
                    diff.to_error_message()
                )
            }
            ModifyDatabasePrivilegesError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ModifyDatabasePrivilegesError::DatabaseValidationError(err) => {
                err.error_type() + "/database"
            }
            ModifyDatabasePrivilegesError::UserValidationError(err) => err.error_type() + "/user",
            ModifyDatabasePrivilegesError::DatabaseDoesNotExist => {
                "database-does-not-exist".to_string()
            }
            ModifyDatabasePrivilegesError::UserDoesNotExist => "user-does-not-exist".to_string(),
            ModifyDatabasePrivilegesError::DiffDoesNotApply(err) => {
                format!("diff-does-not-apply/{}", err.error_type())
            }
            ModifyDatabasePrivilegesError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

impl DiffDoesNotApplyError {
    #[must_use]
    pub fn to_error_message(&self) -> String {
        match self {
            DiffDoesNotApplyError::RowAlreadyExists(database_name, username) => {
                format!(
                    "Privileges for user '{username}' on database '{database_name}' already exist."
                )
            }
            DiffDoesNotApplyError::RowDoesNotExist(database_name, username) => {
                format!(
                    "Privileges for user '{username}' on database '{database_name}' do not exist."
                )
            }
            DiffDoesNotApplyError::RowPrivilegeChangeDoesNotApply(diff, row) => {
                format!("Could not apply privilege change {diff:?} to row {row:?}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            DiffDoesNotApplyError::RowAlreadyExists(_, _) => "row-already-exists".to_string(),
            DiffDoesNotApplyError::RowDoesNotExist(_, _) => "row-does-not-exist".to_string(),
            DiffDoesNotApplyError::RowPrivilegeChangeDoesNotApply(_, _) => {
                "row-privilege-change-does-not-apply".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request =
            BTreeSet::from([DatabasePrivilegesDiff::Modified(DatabasePrivilegeRowDiff {
                db: "test_db".into(),
                user: "test_user".into(),
                select_priv: Some(database_privileges::DatabasePrivilegeChange::NoToYes),
                ..Default::default()
            })]);

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: ModifyPrivilegesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response: ModifyPrivilegesResponse = BTreeMap::from([(
            "test_db".into(),
            BTreeMap::from([
                ("test_user".into(), Ok(())),
                (
                    "invalid_user".into(),
                    Err(ModifyDatabasePrivilegesError::UserDoesNotExist),
                ),
            ]),
        )]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: ModifyPrivilegesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
