use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::database_privileges::DatabasePrivilegeRow;

pub type ListAllPrivilegesResponse = Result<Vec<DatabasePrivilegeRow>, ListAllPrivilegesError>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ListAllPrivilegesError {
    #[error("MySQL error: {0}")]
    MySqlError(String),
}

impl ListAllPrivilegesError {
    #[must_use]
    pub fn to_error_message(&self) -> String {
        match self {
            ListAllPrivilegesError::MySqlError(err) => format!("MySQL error: {err}"),
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ListAllPrivilegesError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_response() {
        let response: ListAllPrivilegesResponse = Ok(vec![
            DatabasePrivilegeRow {
                user: "user1".into(),
                db: "db1".into(),
                select_priv: true,
                insert_priv: false,
                ..Default::default()
            },
            DatabasePrivilegeRow {
                user: "user2".into(),
                db: "db2".into(),
                select_priv: false,
                insert_priv: true,
                ..Default::default()
            },
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let deserialized: ListAllPrivilegesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
