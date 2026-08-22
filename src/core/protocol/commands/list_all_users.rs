use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::server::sql::user_operations::DatabaseUser;

pub type ListAllUsersResponse = Result<Vec<DatabaseUser>, ListAllUsersError>;

#[derive(Error, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ListAllUsersError {
    #[error("MySQL error: {0}")]
    MySqlError(String),
}

impl ListAllUsersError {
    #[must_use]
    pub fn to_error_message(&self) -> String {
        match self {
            ListAllUsersError::MySqlError(err) => format!("MySQL error: {err}"),
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ListAllUsersError::MySqlError(_) => "mysql-error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_response() {
        let response: ListAllUsersResponse = Ok(vec![
            DatabaseUser {
                user: "user1".into(),
                host: "%".into(),
                has_password: true,
                is_locked: false,
                databases: vec!["db1".into(), "db2".into()],
                database_count: 2,
            },
            DatabaseUser {
                user: "user2".into(),
                host: "%".into(),
                has_password: false,
                is_locked: true,
                databases: vec!["db3".into()],
                database_count: 1,
            },
        ]);

        let json = serde_json::to_string_pretty(&response).unwrap();
        println!("Serialized response:\n{}", json);

        let mut deserialized: ListAllUsersResponse = serde_json::from_str(&json).unwrap();
        deserialized.as_mut().unwrap()[0].host = "%".into();
        deserialized.as_mut().unwrap()[1].host = "%".into();

        assert_eq!(response, deserialized);
    }
}
