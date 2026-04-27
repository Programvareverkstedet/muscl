use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::core::{
    protocol::request_validation::ValidationError,
    types::{DbOrUser, MySQLDatabase},
};

pub type DropDatabasesRequest = Vec<MySQLDatabase>;

pub type DropDatabasesResponse = BTreeMap<MySQLDatabase, Result<(), DropDatabaseError>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DropDatabaseError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),

    #[error("Database does not exist")]
    DatabaseDoesNotExist,

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

pub fn print_drop_databases_output_status(output: &DropDatabasesResponse) {
    for (database_name, result) in output {
        match result {
            Ok(()) => {
                println!(
                    "Database '{}' dropped successfully.",
                    database_name.as_str()
                );
            }
            Err(err) => {
                eprintln!("{}", err.to_error_message(database_name));
                eprintln!("Skipping...");
            }
        }
        println!();
    }
}

pub fn print_drop_databases_output_status_json(output: &DropDatabasesResponse) {
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

impl DropDatabaseError {
    #[must_use]
    pub fn to_error_message(&self, database_name: &MySQLDatabase) -> String {
        match self {
            DropDatabaseError::ValidationError(err) => {
                err.to_error_message(&DbOrUser::Database(database_name.clone()))
            }
            DropDatabaseError::DatabaseDoesNotExist => {
                format!("Database {database_name} does not exist.")
            }
            DropDatabaseError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            DropDatabaseError::ValidationError(err) => err.error_type(),
            DropDatabaseError::DatabaseDoesNotExist => "database-does-not-exist".to_string(),
            DropDatabaseError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request: DropDatabasesRequest = vec!["db1".into(), "db2".into(), "db3".into()];

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: DropDatabasesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response: DropDatabasesResponse = BTreeMap::from([
            ("db1".into(), Ok(())),
            ("db2".into(), Err(DropDatabaseError::DatabaseDoesNotExist)),
            (
                "db3".into(),
                Err(DropDatabaseError::MySqlError("Some MySQL error".into())),
            ),
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: DropDatabasesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
