use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::core::{protocol::request_validation::ValidationError, types::DbOrUser};

pub type CheckAuthorizationRequest = Vec<DbOrUser>;

pub type CheckAuthorizationResponse = BTreeMap<DbOrUser, Result<(), CheckAuthorizationError>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[error("Validation error: {0}")]
pub struct CheckAuthorizationError(#[from] pub ValidationError);

pub fn print_check_authorization_output_status(output: &CheckAuthorizationResponse) {
    for (db_or_user, result) in output {
        match result {
            Ok(()) => {
                println!("'{}': OK", db_or_user.name());
            }
            Err(err) => {
                eprintln!(
                    "'{}': {}",
                    db_or_user.name(),
                    err.to_error_message(db_or_user)
                );
            }
        }
    }
}

pub fn print_check_authorization_output_status_json(output: &CheckAuthorizationResponse) {
    let value = output
        .iter()
        .map(|(db_or_user, result)| match result {
            Ok(()) => (
                db_or_user.name().to_string(),
                json!({ "status": "success" }),
            ),
            Err(err) => (
                db_or_user.name().to_string(),
                json!({
                  "status": "error",
                  "type": err.error_type(),
                  "error": err.to_error_message(db_or_user),
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

impl CheckAuthorizationError {
    #[must_use]
    pub fn to_error_message(&self, db_or_user: &DbOrUser) -> String {
        self.0.to_error_message(db_or_user)
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        self.0.error_type()
    }
}

#[cfg(test)]
mod tests {
    use crate::core::protocol::request_validation::NameValidationError;

    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request: CheckAuthorizationRequest = vec![
            DbOrUser::Database("test_db".into()),
            DbOrUser::User("test_user".into()),
        ];

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: CheckAuthorizationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response: CheckAuthorizationResponse = BTreeMap::from([
            (DbOrUser::Database("test_db".into()), Ok(())),
            (
                DbOrUser::User("test_user".into()),
                Err(CheckAuthorizationError(
                    ValidationError::NameValidationError(NameValidationError::TooLong),
                )),
            ),
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: CheckAuthorizationResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
