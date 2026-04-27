use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::core::{
    protocol::request_validation::ValidationError,
    types::{DbOrUser, MySQLDatabase},
};

pub type CreateDatabasesRequest = Vec<MySQLDatabase>;

pub type CreateDatabasesResponse = BTreeMap<MySQLDatabase, Result<(), CreateDatabaseError>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CreateDatabaseError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),

    #[error("Database already exists")]
    DatabaseAlreadyExists,

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

pub fn print_create_databases_output_status(output: &CreateDatabasesResponse) {
    for (database_name, result) in output {
        match result {
            Ok(()) => {
                println!("Database '{database_name}' created successfully.");
            }
            Err(err) => {
                eprintln!("{}", err.to_error_message(database_name));
                eprintln!("Skipping...");
            }
        }
        println!();
    }
}

pub fn print_create_databases_output_status_json(output: &CreateDatabasesResponse) {
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

impl CreateDatabaseError {
    #[must_use]
    pub fn to_error_message(&self, database_name: &MySQLDatabase) -> String {
        match self {
            CreateDatabaseError::ValidationError(err) => {
                err.to_error_message(&DbOrUser::Database(database_name.clone()))
            }
            CreateDatabaseError::DatabaseAlreadyExists => {
                format!("Database {database_name} already exists.")
            }
            CreateDatabaseError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            CreateDatabaseError::ValidationError(err) => err.error_type(),
            CreateDatabaseError::DatabaseAlreadyExists => "database-already-exists".to_string(),
            CreateDatabaseError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request: CreateDatabasesRequest =
            vec!["test_db1".into(), "test_db2".into(), "test_db3".into()];

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: CreateDatabasesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response: CreateDatabasesResponse = BTreeMap::from([
            ("test_db1".into(), Ok(())),
            (
                "test_db2".into(),
                Err(CreateDatabaseError::DatabaseAlreadyExists),
            ),
            (
                "test_db3".into(),
                Err(CreateDatabaseError::MySqlError("Some MySQL error".into())),
            ),
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: CreateDatabasesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
