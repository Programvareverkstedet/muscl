// TODO: merge all rows into a single collection.
//       they already contain which database they belong to.
//       no need to index by database name.

use std::collections::BTreeMap;

use itertools::Itertools;
use prettytable::{Cell, Row, Table};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::core::{
    common::yn,
    database_privileges::{
        DATABASE_PRIVILEGE_FIELDS, DatabasePrivilegeRow, db_priv_field_human_readable_name,
        db_priv_field_single_character_name,
    },
    protocol::request_validation::ValidationError,
    types::{DbOrUser, MySQLDatabase},
};

pub type ListPrivilegesRequest = Option<Vec<MySQLDatabase>>;

pub type ListPrivilegesResponse =
    BTreeMap<MySQLDatabase, Result<Vec<DatabasePrivilegeRow>, ListPrivilegesError>>;

pub fn print_list_privileges_output_status(output: &ListPrivilegesResponse, long_names: bool) {
    let mut final_privs_map: BTreeMap<MySQLDatabase, Vec<DatabasePrivilegeRow>> = BTreeMap::new();
    for (db_name, db_result) in output {
        match db_result {
            Ok(db_rows) => {
                final_privs_map.insert(db_name.clone(), db_rows.clone());
            }
            Err(err) => {
                eprintln!("{}", err.to_error_message(db_name));
                eprintln!("Skipping...");
            }
        }
    }

    if final_privs_map.is_empty() {
        println!("No privileges to show.");
    } else {
        let mut table = Table::new();

        table.add_row(Row::new(
            DATABASE_PRIVILEGE_FIELDS
                .into_iter()
                .map(|field| {
                    if field == "Db" || field == "User" {
                        db_priv_field_human_readable_name(field)
                    } else if long_names {
                        format!(
                            "{} ({})",
                            db_priv_field_human_readable_name(field),
                            db_priv_field_single_character_name(field),
                        )
                    } else {
                        db_priv_field_human_readable_name(field)
                    }
                })
                .map(|name| Cell::new(&name))
                .collect(),
        ));

        for row in final_privs_map
            .values()
            .flatten()
            .sorted_by_key(|row| (&row.db, &row.user))
        {
            table.add_row(row![
                row.db,
                row.user,
                c->yn(row.select_priv),
                c->yn(row.insert_priv),
                c->yn(row.update_priv),
                c->yn(row.delete_priv),
                c->yn(row.create_priv),
                c->yn(row.drop_priv),
                c->yn(row.alter_priv),
                c->yn(row.index_priv),
                c->yn(row.create_tmp_table_priv),
                c->yn(row.lock_tables_priv),
                c->yn(row.references_priv),
            ]);
        }
        // }

        table.printstd();
    }
}

pub fn print_list_privileges_output_status_json(output: &ListPrivilegesResponse) {
    let value = output
        .iter()
        .map(|(name, result)| match result {
            Ok(row) => (
                name.to_string(),
                json!({
                  "status": "success",
                  "value": row.iter().into_group_map_by(|priv_row| priv_row.user.clone()),
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

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ListPrivilegesError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),

    #[error("Database does not exist")]
    DatabaseDoesNotExist,

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

impl ListPrivilegesError {
    #[must_use]
    pub fn to_error_message(&self, database_name: &MySQLDatabase) -> String {
        match self {
            ListPrivilegesError::ValidationError(err) => {
                err.to_error_message(&DbOrUser::Database(database_name.clone()))
            }
            ListPrivilegesError::DatabaseDoesNotExist => {
                format!("Database '{database_name}' does not exist.")
            }
            ListPrivilegesError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ListPrivilegesError::ValidationError(err) => err.error_type(),
            ListPrivilegesError::DatabaseDoesNotExist => "database-does-not-exist".to_string(),
            ListPrivilegesError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request: ListPrivilegesRequest = Some(vec!["test_db1".into(), "test_db2".into()]);
        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: ListPrivilegesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response: ListPrivilegesResponse = BTreeMap::from([
            (
                "test_db1".into(),
                Ok(vec![DatabasePrivilegeRow {
                    db: "test_db1".into(),
                    user: "user1".into(),
                    select_priv: true,
                    insert_priv: false,
                    ..Default::default()
                }]),
            ),
            (
                "test_db2".into(),
                Err(ListPrivilegesError::DatabaseDoesNotExist),
            ),
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: ListPrivilegesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
