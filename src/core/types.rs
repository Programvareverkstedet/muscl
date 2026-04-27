use std::{
    ffi::OsString,
    fmt,
    ops::{Deref, DerefMut},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct MySQLUser(String);

impl FromStr for MySQLUser {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MySQLUser(s.to_string()))
    }
}

impl Deref for MySQLUser {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MySQLUser {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl fmt::Display for MySQLUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for MySQLUser {
    fn from(s: &str) -> Self {
        MySQLUser(s.to_string())
    }
}

impl From<String> for MySQLUser {
    fn from(s: String) -> Self {
        MySQLUser(s)
    }
}

impl From<MySQLUser> for OsString {
    fn from(val: MySQLUser) -> Self {
        val.0.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct MySQLDatabase(String);

impl FromStr for MySQLDatabase {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MySQLDatabase(s.to_string()))
    }
}

impl Deref for MySQLDatabase {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MySQLDatabase {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl fmt::Display for MySQLDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for MySQLDatabase {
    fn from(s: &str) -> Self {
        MySQLDatabase(s.to_string())
    }
}

impl From<String> for MySQLDatabase {
    fn from(s: String) -> Self {
        MySQLDatabase(s)
    }
}

impl From<MySQLDatabase> for OsString {
    fn from(val: MySQLDatabase) -> Self {
        val.0.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DbOrUser {
    Database(MySQLDatabase),
    User(MySQLUser),
}

impl Serialize for DbOrUser {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            DbOrUser::Database(db) => ("d:".to_string() + &db.to_string()).serialize(serializer),
            DbOrUser::User(user) => ("u:".to_string() + &user.to_string()).serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for DbOrUser {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Some(rest) = s.strip_prefix("d:") {
            Ok(DbOrUser::Database(MySQLDatabase(rest.to_string())))
        } else if let Some(rest) = s.strip_prefix("u:") {
            Ok(DbOrUser::User(MySQLUser(rest.to_string())))
        } else {
            Err(serde::de::Error::custom(format!(
                "Invalid DbOrUser format: {}",
                s
            )))
        }
    }
}

impl DbOrUser {
    #[must_use]
    pub fn lowercased_noun(&self) -> &'static str {
        match self {
            DbOrUser::Database(_) => "database",
            DbOrUser::User(_) => "user",
        }
    }

    #[must_use]
    pub fn capitalized_noun(&self) -> &'static str {
        match self {
            DbOrUser::Database(_) => "Database",
            DbOrUser::User(_) => "User",
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            DbOrUser::Database(db) => db.as_str(),
            DbOrUser::User(user) => user.as_str(),
        }
    }

    #[must_use]
    pub fn prefix(&self) -> &str {
        match self {
            DbOrUser::Database(db) => db.split('_').next().unwrap_or("?"),
            DbOrUser::User(user) => user.split('_').next().unwrap_or("?"),
        }
    }
}
