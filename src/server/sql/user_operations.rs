use indoc::formatdoc;
use itertools::Itertools;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use sqlx::MySqlConnection;
use sqlx::prelude::*;

use crate::core::protocol::request_validation::GroupDenylist;
use crate::core::protocol::request_validation::validate_db_or_user_request;
use crate::core::types::DbOrUser;
use crate::{
    core::{
        common::UnixUser,
        database_privileges::DATABASE_PRIVILEGE_FIELDS,
        protocol::{
            CreateUserError, CreateUsersResponse, DropUserError, DropUsersResponse,
            ListAllUsersError, ListAllUsersResponse, ListUsersError, ListUsersResponse,
            LockUserError, LockUsersResponse, SetPasswordError, SetUserPasswordResponse,
            UnlockUserError, UnlockUsersResponse,
        },
        types::MySQLUser,
    },
    server::{
        common::{create_user_group_matching_regex, try_get_with_binary_fallback},
        sql::quote_literal,
    },
};

// NOTE: this function is unsafe because it does no input validation.
pub(super) async fn unsafe_user_exists(
    db_user: &str,
    connection: &mut MySqlConnection,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r"
          SELECT EXISTS(
            SELECT 1
            FROM `mysql`.`user`
            WHERE `User` = ?
          )
        ",
    )
    .bind(db_user)
    .fetch_one(connection)
    .await
    .map(|row| row.get::<bool, _>(0));

    if let Err(err) = &result {
        tracing::error!("Failed to check if database user exists: {:?}", err);
    }

    result
}

pub async fn complete_user_name(
    user_prefix: &str,
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> Vec<MySQLUser> {
    let result = sqlx::query(
        r"
          SELECT `User` AS `user`
          FROM `mysql`.`user`
          WHERE `User` REGEXP ?
            AND `User` LIKE ?
        ",
    )
    .bind(create_user_group_matching_regex(unix_user, group_denylist))
    .bind(format!("{user_prefix}%"))
    .fetch_all(connection)
    .await;

    match result {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|row| {
                let user: String = try_get_with_binary_fallback(&row, "user").ok()?;
                Some(user.into())
            })
            .collect(),
        Err(err) => {
            tracing::error!(
                "Failed to complete user name for prefix '{}' and user '{}': {:?}",
                user_prefix,
                unix_user.username,
                err
            );
            vec![]
        }
    }
}

pub async fn create_database_users(
    db_users: &[MySQLUser],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> CreateUsersResponse {
    let mut results = BTreeMap::new();

    for db_user in db_users.iter().cloned() {
        if let Err(err) =
            validate_db_or_user_request(&DbOrUser::User(db_user.clone()), unix_user, group_denylist)
                .map_err(CreateUserError::ValidationError)
        {
            results.insert(db_user, Err(err));
            continue;
        }

        match unsafe_user_exists(&db_user, &mut *connection).await {
            Ok(true) => {
                results.insert(db_user, Err(CreateUserError::UserAlreadyExists));
                continue;
            }
            Err(err) => {
                results.insert(db_user, Err(CreateUserError::MySqlError(err.to_string())));
                continue;
            }
            _ => {}
        }

        let result = sqlx::query(format!("CREATE USER {}@'%'", quote_literal(&db_user),).as_str())
            .execute(&mut *connection)
            .await
            .map(|_| ())
            .map_err(|err| CreateUserError::MySqlError(err.to_string()));

        if let Err(err) = &result {
            tracing::error!("Failed to create database user '{}': {:?}", &db_user, err);
        }

        results.insert(db_user, result);
    }

    results
}

pub async fn drop_database_users(
    db_users: &[MySQLUser],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> DropUsersResponse {
    let mut results = BTreeMap::new();

    for db_user in db_users.iter().cloned() {
        if let Err(err) =
            validate_db_or_user_request(&DbOrUser::User(db_user.clone()), unix_user, group_denylist)
                .map_err(DropUserError::ValidationError)
        {
            results.insert(db_user, Err(err));
            continue;
        }

        match unsafe_user_exists(&db_user, &mut *connection).await {
            Ok(false) => {
                results.insert(db_user, Err(DropUserError::UserDoesNotExist));
                continue;
            }
            Err(err) => {
                results.insert(db_user, Err(DropUserError::MySqlError(err.to_string())));
                continue;
            }
            _ => {}
        }

        let result = sqlx::query(format!("DROP USER {}@'%'", quote_literal(&db_user),).as_str())
            .execute(&mut *connection)
            .await
            .map(|_| ())
            .map_err(|err| DropUserError::MySqlError(err.to_string()));

        if let Err(err) = &result {
            tracing::error!("Failed to drop database user '{}': {:?}", &db_user, err);
        }

        results.insert(db_user, result);
    }

    results
}

pub async fn set_password_for_database_user(
    db_user: &MySQLUser,
    password: &str,
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> SetUserPasswordResponse {
    validate_db_or_user_request(&DbOrUser::User(db_user.clone()), unix_user, group_denylist)
        .map_err(SetPasswordError::ValidationError)?;

    match unsafe_user_exists(db_user, &mut *connection).await {
        Ok(false) => return Err(SetPasswordError::UserDoesNotExist),
        Err(err) => return Err(SetPasswordError::MySqlError(err.to_string())),
        _ => {}
    }

    let result = sqlx::query(
        format!(
            "ALTER USER {}@'%' IDENTIFIED BY {}",
            quote_literal(db_user),
            quote_literal(password).as_str(),
        )
        .as_str(),
    )
    .execute(&mut *connection)
    .await
    .map(|_| ())
    .map_err(|err| SetPasswordError::MySqlError(err.to_string()));

    if result.is_err() {
        tracing::error!(
            "Failed to set password for database user '{}': <REDACTED>",
            &db_user,
        );
    }

    result
}

const DATABASE_USER_LOCK_STATUS_QUERY_MARIADB: &str = r#"
    SELECT COALESCE(
        JSON_EXTRACT(`mysql`.`global_priv`.`priv`, "$.account_locked"),
        'false'
    ) != 'false'
    FROM `mysql`.`global_priv`
    WHERE `User` = ?
    AND `Host` = '%'
"#;

const DATABASE_USER_LOCK_STATUS_QUERY_MYSQL: &str = r"
    SELECT `mysql`.`user`.`account_locked` = 'Y'
    FROM `mysql`.`user`
    WHERE `User` = ?
    AND `Host` = '%'
";

// NOTE: this function is unsafe because it does no input validation.
async fn database_user_is_locked_unsafe(
    db_user: &str,
    connection: &mut MySqlConnection,
    db_is_mariadb: bool,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(if db_is_mariadb {
        DATABASE_USER_LOCK_STATUS_QUERY_MARIADB
    } else {
        DATABASE_USER_LOCK_STATUS_QUERY_MYSQL
    })
    .bind(db_user)
    .fetch_one(connection)
    .await
    .map(|row| row.try_get(0))
    .and_then(|res| res);

    if let Err(err) = &result {
        tracing::error!(
            "Failed to check if database user is locked '{}': {:?}",
            &db_user,
            err
        );
    }

    result
}

pub async fn lock_database_users(
    db_users: &[MySQLUser],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> LockUsersResponse {
    let mut results = BTreeMap::new();

    for db_user in db_users.iter().cloned() {
        if let Err(err) =
            validate_db_or_user_request(&DbOrUser::User(db_user.clone()), unix_user, group_denylist)
                .map_err(LockUserError::ValidationError)
        {
            results.insert(db_user, Err(err));
            continue;
        }

        match unsafe_user_exists(&db_user, &mut *connection).await {
            Ok(true) => {}
            Ok(false) => {
                results.insert(db_user, Err(LockUserError::UserDoesNotExist));
                continue;
            }
            Err(err) => {
                results.insert(db_user, Err(LockUserError::MySqlError(err.to_string())));
                continue;
            }
        }

        match database_user_is_locked_unsafe(&db_user, &mut *connection, db_is_mariadb).await {
            Ok(false) => {}
            Ok(true) => {
                results.insert(db_user, Err(LockUserError::UserIsAlreadyLocked));
                continue;
            }
            Err(err) => {
                results.insert(db_user, Err(LockUserError::MySqlError(err.to_string())));
                continue;
            }
        }

        let result = sqlx::query(
            format!("ALTER USER {}@'%' ACCOUNT LOCK", quote_literal(&db_user),).as_str(),
        )
        .execute(&mut *connection)
        .await
        .map(|_| ())
        .map_err(|err| LockUserError::MySqlError(err.to_string()));

        if let Err(err) = &result {
            tracing::error!("Failed to lock database user '{}': {:?}", &db_user, err);
        }

        results.insert(db_user, result);
    }

    results
}

pub async fn unlock_database_users(
    db_users: &[MySQLUser],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> UnlockUsersResponse {
    let mut results = BTreeMap::new();

    for db_user in db_users.iter().cloned() {
        if let Err(err) =
            validate_db_or_user_request(&DbOrUser::User(db_user.clone()), unix_user, group_denylist)
                .map_err(UnlockUserError::ValidationError)
        {
            results.insert(db_user, Err(err));
            continue;
        }

        match unsafe_user_exists(&db_user, &mut *connection).await {
            Ok(false) => {
                results.insert(db_user, Err(UnlockUserError::UserDoesNotExist));
                continue;
            }
            Err(err) => {
                results.insert(db_user, Err(UnlockUserError::MySqlError(err.to_string())));
                continue;
            }
            _ => {}
        }

        match database_user_is_locked_unsafe(&db_user, &mut *connection, db_is_mariadb).await {
            Ok(false) => {
                results.insert(db_user, Err(UnlockUserError::UserIsAlreadyUnlocked));
                continue;
            }
            Err(err) => {
                results.insert(db_user, Err(UnlockUserError::MySqlError(err.to_string())));
                continue;
            }
            _ => {}
        }

        let result = sqlx::query(
            format!("ALTER USER {}@'%' ACCOUNT UNLOCK", quote_literal(&db_user),).as_str(),
        )
        .execute(&mut *connection)
        .await
        .map(|_| ())
        .map_err(|err| UnlockUserError::MySqlError(err.to_string()));

        if let Err(err) = &result {
            tracing::error!("Failed to unlock database user '{}': {:?}", &db_user, err);
        }

        results.insert(db_user, result);
    }

    results
}

/// This struct contains information about a database user.
/// This can be extended if we need more information in the future.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseUser {
    pub user: MySQLUser,
    #[serde(skip)]
    pub host: String,
    pub has_password: bool,
    pub is_locked: bool,
    pub databases: Vec<String>,
}

impl FromRow<'_, sqlx::mysql::MySqlRow> for DatabaseUser {
    fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            user: try_get_with_binary_fallback(row, "User")?.into(),
            host: try_get_with_binary_fallback(row, "Host")?,
            has_password: row.try_get("has_password")?,
            is_locked: row.try_get("account_locked")?,
            databases: Vec::new(),
        })
    }
}

const DB_USER_SELECT_STATEMENT_MARIADB: &str = r#"
SELECT
  `user`.`User`,
  `user`.`Host`,
  `user`.`Password` != '' OR `user`.`authentication_string` != '' AS `has_password`,
  COALESCE(
    JSON_EXTRACT(`global_priv`.`priv`, "$.account_locked"),
    'false'
  ) != 'false' AS `account_locked`
FROM `user`
JOIN `global_priv` ON
  `user`.`User` = `global_priv`.`User`
  AND `user`.`Host` = `global_priv`.`Host`
"#;

const DB_USER_SELECT_STATEMENT_MYSQL: &str = r"
SELECT
  `user`.`User`,
  `user`.`Host`,
  `user`.`authentication_string` != '' AS `has_password`,
  `user`.`account_locked` = 'Y' AS `account_locked`
FROM `user`
";

pub async fn list_database_users(
    db_users: &[MySQLUser],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> ListUsersResponse {
    let mut results = BTreeMap::new();

    for db_user in db_users.iter().cloned() {
        if let Err(err) =
            validate_db_or_user_request(&DbOrUser::User(db_user.clone()), unix_user, group_denylist)
                .map_err(ListUsersError::ValidationError)
        {
            results.insert(db_user, Err(err));
            continue;
        }

        let mut result = sqlx::query_as::<_, DatabaseUser>(
            &(if db_is_mariadb {
                DB_USER_SELECT_STATEMENT_MARIADB.to_string()
            } else {
                DB_USER_SELECT_STATEMENT_MYSQL.to_string()
            } + "WHERE `mysql`.`user`.`User` = ?"),
        )
        .bind(db_user.as_str())
        .fetch_optional(&mut *connection)
        .await;

        if let Err(err) = &result {
            tracing::error!("Failed to list database user '{}': {:?}", &db_user, err);
        }

        if let Ok(Some(user)) = result.as_mut()
            && let Err(err) = set_databases_where_user_has_privileges(user, &mut *connection).await
        {
            result = Err(err);
        }

        match result {
            Ok(Some(user)) => results.insert(db_user, Ok(user)),
            Ok(None) => results.insert(db_user, Err(ListUsersError::UserDoesNotExist)),
            Err(err) => results.insert(db_user, Err(ListUsersError::MySqlError(err.to_string()))),
        };
    }

    results
}

pub async fn list_all_database_users_for_unix_user(
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> ListAllUsersResponse {
    let mut result = sqlx::query_as::<_, DatabaseUser>(
        &(if db_is_mariadb {
            DB_USER_SELECT_STATEMENT_MARIADB.to_string()
        } else {
            DB_USER_SELECT_STATEMENT_MYSQL.to_string()
        } + "WHERE `user`.`User` REGEXP ?"),
    )
    .bind(create_user_group_matching_regex(unix_user, group_denylist))
    .fetch_all(&mut *connection)
    .await
    .map_err(|err| ListAllUsersError::MySqlError(err.to_string()));

    if let Err(err) = &result {
        tracing::error!("Failed to list all database users: {:?}", err);
    }

    if let Ok(users) = result.as_mut() {
        for user in users {
            if let Err(mysql_error) =
                set_databases_where_user_has_privileges(user, &mut *connection).await
            {
                return Err(ListAllUsersError::MySqlError(mysql_error.to_string()));
            }
        }
    }

    result
}

/// This function sets the `databases` field of the given `DatabaseUser`
/// where the user has any privileges.
pub async fn set_databases_where_user_has_privileges(
    db_user: &mut DatabaseUser,
    connection: &mut MySqlConnection,
) -> Result<(), sqlx::Error> {
    let database_list = sqlx::query(
        formatdoc!(
            r"
                SELECT `Db` AS `database`
                FROM `db`
                WHERE `User` = ? AND ({})
            ",
            DATABASE_PRIVILEGE_FIELDS
                .iter()
                .map(|field| format!("`{field}` = 'Y'"))
                .join(" OR "),
        )
        .as_str(),
    )
    .bind(db_user.user.as_str())
    .fetch_all(&mut *connection)
    .await;

    if let Err(err) = &database_list {
        tracing::error!(
            "Failed to list databases for user '{}': {:?}",
            &db_user.user,
            err
        );
    }

    db_user.databases = database_list.and_then(|rows| {
        rows.into_iter()
            .map(|row| try_get_with_binary_fallback(&row, "database"))
            .collect::<Result<Vec<String>, sqlx::Error>>()
    })?;

    Ok(())
}
