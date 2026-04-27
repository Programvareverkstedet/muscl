use std::collections::BTreeMap;

use prettytable::Table;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::{
    core::{
        protocol::request_validation::ValidationError,
        types::{DbOrUser, MySQLUser},
    },
    server::sql::user_operations::DatabaseUser,
};

pub type ListUsersRequest = Option<Vec<MySQLUser>>;

pub type ListUsersResponse = BTreeMap<MySQLUser, Result<DatabaseUser, ListUsersError>>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ListUsersError {
    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),

    #[error("User does not exist")]
    UserDoesNotExist,

    #[error("MySQL error: {0}")]
    MySqlError(String),
}

pub fn print_list_users_output_status(output: &ListUsersResponse) {
    let mut final_user_list: Vec<&DatabaseUser> = Vec::new();
    for (db_name, db_result) in output {
        match db_result {
            Ok(db_row) => final_user_list.push(db_row),
            Err(err) => {
                eprintln!("{}", err.to_error_message(db_name));
                eprintln!("Skipping...");
            }
        }
    }

    if final_user_list.is_empty() {
        println!("No users to show.");
    } else {
        let mut table = Table::new();
        table.add_row(row![
            "User",
            "Password is set",
            "Locked",
            "Databases where user has privileges"
        ]);
        for user in final_user_list {
            table.add_row(row![
                user.user,
                user.has_password,
                user.is_locked,
                user.databases.join("\n")
            ]);
        }
        table.printstd();
    }
}

pub fn print_list_users_output_status_json(output: &ListUsersResponse) {
    let value = output
        .iter()
        .map(|(name, result)| match result {
            Ok(row) => (
                name.to_string(),
                json!({
                  "status": "success",
                  "value": {
                    "user": row.user,
                    "has_password": row.has_password,
                    "is_locked": row.is_locked,
                    "databases": row.databases,
                  }
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

impl ListUsersError {
    #[must_use]
    pub fn to_error_message(&self, username: &MySQLUser) -> String {
        match self {
            ListUsersError::ValidationError(err) => {
                err.to_error_message(&DbOrUser::User(username.clone()))
            }
            ListUsersError::UserDoesNotExist => {
                format!("User '{username}' does not exist.")
            }
            ListUsersError::MySqlError(err) => {
                format!("MySQL error: {err}")
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ListUsersError::ValidationError(err) => err.error_type(),
            ListUsersError::UserDoesNotExist => "user-does-not-exist".to_string(),
            ListUsersError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_request() {
        let request: ListUsersRequest = Some(vec!["test_user1".into(), "test_user2".into()]);

        let json = serde_json::to_string_pretty(&request).unwrap();
        println!("Serialized request:\n{}", json);

        let deserialized: ListUsersRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn test_serialize_deserialize_response() {
        let response_ok: ListUsersResponse = BTreeMap::from([
            (
                "test_user1".into(),
                Ok(DatabaseUser {
                    user: "test_user1".into(),
                    host: "%".into(),
                    has_password: true,
                    is_locked: false,
                    databases: vec!["db1".into(), "db2".into()],
                }),
            ),
            ("test_user2".into(), Err(ListUsersError::UserDoesNotExist)),
        ]);

        let json = serde_json::to_string_pretty(&response_ok).unwrap();
        println!("Serialized response:\n{}", json);

        let mut deserialized: ListUsersResponse = serde_json::from_str(&json).unwrap();
        deserialized
            .get_mut(&"test_user1".into())
            .unwrap()
            .as_mut()
            .unwrap()
            .host = "%".into();
        assert_eq!(response_ok, deserialized);
    }
}
