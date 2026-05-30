use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::server::sql::database_operations::DatabaseRow;

pub type ListAllDatabasesResponse = Result<Vec<DatabaseRow>, ListAllDatabasesError>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ListAllDatabasesError {
    #[error("MySQL error: {0}")]
    MySqlError(String),
}

impl ListAllDatabasesError {
    #[must_use]
    pub fn to_error_message(&self) -> String {
        match self {
            ListAllDatabasesError::MySqlError(err) => format!("MySQL error: {err}"),
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ListAllDatabasesError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_response() {
        let response: ListAllDatabasesResponse = Ok(vec![
            DatabaseRow {
                database: "db1".into(),
                tables: vec!["table1".into(), "table2".into()],
                table_count: 2,
                users: vec!["user1".into(), "user2".into()],
                user_count: 2,
                collation: Some("utf8mb4_general_ci".into()),
                character_set: Some("utf8mb4".into()),
                size_bytes: 1024,
            },
            DatabaseRow {
                database: "db2".into(),
                tables: vec!["table3".into(), "table4".into()],
                table_count: 2,
                users: vec!["user3".into(), "user4".into()],
                user_count: 2,
                collation: Some("utf8mb4_general_ci".into()),
                character_set: Some("utf8mb4".into()),
                size_bytes: 2048,
            },
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: ListAllDatabasesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
