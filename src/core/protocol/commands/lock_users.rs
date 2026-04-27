use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::core::{
    protocol::request_validation::ValidationError,
    types::{DbOrUser, MySQLUser},
};

pub type LockUsersRequest = Vec<MySQLUser>;

pub type LockUsersResponse = BTreeMap<MySQLUser, Result<(), LockUserError>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LockUserError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),

    #[error("User does not exist")]
    UserDoesNotExist,

    #[error("User is already locked")]
    UserIsAlreadyLocked,

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

pub fn print_lock_users_output_status(output: &LockUsersResponse) {
    for (username, result) in output {
        match result {
            Ok(()) => {
                println!("User '{username}' locked successfully.");
            }
            Err(err) => {
                eprintln!("{}", err.to_error_message(username));
                eprintln!("Skipping...");
            }
        }
        println!();
    }
}

pub fn print_lock_users_output_status_json(output: &LockUsersResponse) {
    let value = output
        .iter()
        .map(|(name, result)| match result {
            Ok(()) => (name.to_string(), json!({ "status": "success" })),
            Err(err) => (
                name.to_string(),
                json!({
                  "status": "error",
                  "type": err.error_type(),
                  "error": err.to_error_message(name),
                }),
            ),
        })
        .collect::<serde_json::Map<_, _>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&value)
            .unwrap_or("Failed to serialize result to JSON".to_string())
    );
}

impl LockUserError {
    #[must_use]
    pub fn to_error_message(&self, username: &MySQLUser) -> String {
        match self {
            LockUserError::ValidationError(err) => {
                err.to_error_message(&DbOrUser::User(username.clone()))
            }
            LockUserError::UserDoesNotExist => {
                format!("User '{username}' does not exist.")
            }
            LockUserError::UserIsAlreadyLocked => {
                format!("User '{username}' is already locked.")
            }
            LockUserError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            LockUserError::ValidationError(err) => err.error_type(),
            LockUserError::UserDoesNotExist => "user-does-not-exist".to_string(),
            LockUserError::UserIsAlreadyLocked => "user-is-already-locked".to_string(),
            LockUserError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request: LockUsersRequest = vec!["test_user1".into(), "test_user2".into()];

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: LockUsersRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response_ok: LockUsersResponse = BTreeMap::from([
            ("test_user1".into(), Ok(())),
            ("test_user2".into(), Err(LockUserError::UserDoesNotExist)),
        ]);

        let json = serde_json::to_string_pretty(&response_ok).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: LockUsersResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response_ok, deserialized);
    }
}
