use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::core::{
    protocol::request_validation::ValidationError,
    types::{DbOrUser, MySQLUser},
};

pub type CreateUsersRequest = Vec<MySQLUser>;

pub type CreateUsersResponse = BTreeMap<MySQLUser, Result<(), CreateUserError>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CreateUserError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),

    #[error("User already exists")]
    UserAlreadyExists,

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

pub fn print_create_users_output_status(output: &CreateUsersResponse) {
    for (username, result) in output {
        match result {
            Ok(()) => {
                println!("User '{username}' created successfully.");
            }
            Err(err) => {
                eprintln!("{}", err.to_error_message(username));
                eprintln!("Skipping...");
            }
        }
        println!();
    }
}

pub fn print_create_users_output_status_json(output: &CreateUsersResponse) {
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

impl CreateUserError {
    #[must_use]
    pub fn to_error_message(&self, username: &MySQLUser) -> String {
        match self {
            CreateUserError::ValidationError(err) => {
                err.to_error_message(&DbOrUser::User(username.clone()))
            }
            CreateUserError::UserAlreadyExists => {
                format!("User '{username}' already exists.")
            }
            CreateUserError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            CreateUserError::ValidationError(err) => err.error_type(),
            CreateUserError::UserAlreadyExists => "user-already-exists".to_string(),
            CreateUserError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request: CreateUsersRequest = vec!["alice".into(), "bob".into(), "charlie".into()];

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: CreateUsersRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response: CreateUsersResponse = BTreeMap::from([
            ("alice".into(), Ok(())),
            ("bob".into(), Err(CreateUserError::UserAlreadyExists)),
            (
                "charlie".into(),
                Err(CreateUserError::MySqlError("Some MySQL error".into())),
            ),
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: CreateUsersResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
