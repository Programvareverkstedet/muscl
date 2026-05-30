use std::collections::BTreeMap;

use itertools::Itertools;
use prettytable::Table;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::{
    core::{
        protocol::request_validation::ValidationError,
        types::{DbOrUser, MySQLDatabase},
    },
    server::sql::database_operations::DatabaseRow,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListDatabasesRequest {
    pub names: Option<Vec<MySQLDatabase>>,
    #[serde(default)]
    pub include_all_tables_and_users: bool,
}

impl ListDatabasesRequest {
    pub fn new(names: Option<Vec<MySQLDatabase>>, include_all_tables_and_users: bool) -> Self {
        Self {
            names,
            include_all_tables_and_users,
        }
    }
}

pub type ListDatabasesResponse = BTreeMap<MySQLDatabase, Result<DatabaseRow, ListDatabasesError>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ListDatabasesError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),

    #[error("Database does not exist")]
    DatabaseDoesNotExist,

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

pub fn print_list_databases_output_status(
    output: &ListDatabasesResponse,
    display_size_as_bytes: bool,
) {
    let mut final_database_list: Vec<&DatabaseRow> = Vec::new();
    for (db_name, db_result) in output {
        match db_result {
            Ok(db_row) => final_database_list.push(db_row),
            Err(err) => {
                eprintln!("{}", err.to_error_message(db_name));
                eprintln!("Skipping...");
            }
        }
    }

    if final_database_list.is_empty() {
        println!("No databases to show.");
    } else {
        let mut table = Table::new();
        table.add_row(row![
            "Database",
            "Tables",
            "Users",
            "Collation",
            "Character Set",
            if display_size_as_bytes {
                "Size (Bytes)"
            } else {
                "Size"
            }
        ]);
        for db in final_database_list.iter().sorted_by_key(|db| &db.database) {
            table.add_row(row![
                db.database,
                db.tables.join("\n"),
                db.users.iter().map(|user| user.as_str()).join("\n"),
                db.collation.as_deref().unwrap_or("N/A"),
                db.character_set.as_deref().unwrap_or("N/A"),
                if display_size_as_bytes {
                    db.size_bytes.to_string()
                } else {
                    humansize::format_size(db.size_bytes, humansize::DECIMAL)
                }
            ]);
        }

        table.printstd();
    }
}

pub fn print_list_databases_output_status_json(output: &ListDatabasesResponse) {
    let value = output
        .iter()
        .map(|(name, result)| match result {
            Ok(row) => (
                name.to_string(),
                json!({
                  "status": "success",
                  "tables": row.tables,
                  "users": row.users,
                  "collation": row.collation,
                  "character_set": row.character_set,
                  "size_bytes": row.size_bytes,
                }),
            ),
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

impl ListDatabasesError {
    #[must_use]
    pub fn to_error_message(&self, database_name: &MySQLDatabase) -> String {
        match self {
            ListDatabasesError::ValidationError(err) => {
                err.to_error_message(&DbOrUser::Database(database_name.clone()))
            }
            ListDatabasesError::DatabaseDoesNotExist => {
                format!("Database '{database_name}' does not exist.")
            }
            ListDatabasesError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ListDatabasesError::ValidationError(err) => err.error_type(),
            ListDatabasesError::DatabaseDoesNotExist => "database-does-not-exist".to_string(),
            ListDatabasesError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request = ListDatabasesRequest::new(Some(vec!["db1".into(), "db2".into()]), true);
        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: ListDatabasesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_deserialize_request_without_include_all_tables_and_users_defaults_to_false() {
        let json = serde_json::json!({
            "names": ["db1", "db2"]
        })
        .to_string();

        let deserialized: ListDatabasesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized,
            ListDatabasesRequest::new(Some(vec!["db1".into(), "db2".into()]), false)
        );
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response: ListDatabasesResponse = vec![
            (
                "db1".into(),
                Ok(DatabaseRow {
                    database: "db1".into(),
                    tables: vec!["table1".to_string(), "table2".to_string()],
                    users: vec!["user1".into(), "user2".into()],
                    collation: Some("utf8mb4_general_ci".to_string()),
                    character_set: Some("utf8mb4".to_string()),
                    size_bytes: 1024,
                }),
            ),
            ("db2".into(), Err(ListDatabasesError::DatabaseDoesNotExist)),
        ]
        .into_iter()
        .collect();

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: ListDatabasesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
