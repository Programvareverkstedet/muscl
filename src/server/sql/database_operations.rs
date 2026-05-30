use std::collections::BTreeMap;

use sqlx::AssertSqlSafe;
use sqlx::MySqlConnection;
use sqlx::prelude::*;

use serde::{Deserialize, Serialize};

use crate::core::protocol::CompleteDatabaseNameResponse;
use crate::core::protocol::request_validation::GroupDenylist;
use crate::core::protocol::request_validation::validate_db_or_user_request;
use crate::core::types::DbOrUser;
use crate::core::types::MySQLDatabase;
use crate::core::types::MySQLUser;
use crate::{
    core::{
        common::UnixUser,
        protocol::{
            CreateDatabaseError, CreateDatabasesResponse, DropDatabaseError, DropDatabasesResponse,
            ListAllDatabasesError, ListAllDatabasesResponse, ListDatabasesError,
            ListDatabasesResponse,
        },
    },
    server::{common::create_user_group_matching_regex, sql::quote_identifier},
};

const MAX_SHOW_DB_RELATED_ITEMS: usize = 5;

// NOTE: this function is unsafe because it does no input validation.
pub(super) async fn unsafe_database_exists(
    database_name: &str,
    connection: &mut MySqlConnection,
) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?")
            .bind(database_name)
            .fetch_optional(connection)
            .await;

    if let Err(err) = &result {
        tracing::error!(
            "Failed to check if database '{}' exists: {:?}",
            &database_name,
            err
        );
    }

    Ok(result?.is_some())
}

pub async fn complete_database_name(
    database_prefix: &str,
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> CompleteDatabaseNameResponse {
    let result = sqlx::query(
        r"
          SELECT CAST(`SCHEMA_NAME` AS CHAR(64)) AS `database`
          FROM `information_schema`.`SCHEMATA`
          WHERE `SCHEMA_NAME` NOT IN ('information_schema', 'performance_schema', 'mysql', 'sys')
            AND `SCHEMA_NAME` REGEXP ?
            AND `SCHEMA_NAME` LIKE ?
        ",
    )
    .bind(create_user_group_matching_regex(unix_user, group_denylist))
    .bind(format!("{database_prefix}%"))
    .fetch_all(connection)
    .await;

    match result {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|row| {
                let database: String = row.try_get("database").ok()?;
                Some(database.into())
            })
            .collect(),
        Err(err) => {
            tracing::error!(
                "Failed to complete database name for prefix '{}' and user '{}': {:?}",
                database_prefix,
                unix_user.username,
                err
            );
            vec![]
        }
    }
}

pub async fn create_databases(
    database_names: &[MySQLDatabase],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> CreateDatabasesResponse {
    let mut results = BTreeMap::new();

    for database_name in database_names.iter().cloned() {
        if let Err(err) = validate_db_or_user_request(
            &DbOrUser::Database(database_name.clone()),
            unix_user,
            group_denylist,
        )
        .map_err(CreateDatabaseError::ValidationError)
        {
            results.insert(database_name.clone(), Err(err));
            continue;
        }

        match unsafe_database_exists(&database_name, &mut *connection).await {
            Ok(true) => {
                results.insert(
                    database_name.clone(),
                    Err(CreateDatabaseError::DatabaseAlreadyExists),
                );
                continue;
            }
            Err(err) => {
                results.insert(
                    database_name.clone(),
                    Err(CreateDatabaseError::MySqlError(err.to_string())),
                );
                continue;
            }
            _ => {}
        }

        let statement = AssertSqlSafe(format!(
            "CREATE DATABASE {}",
            quote_identifier(&database_name)
        ));
        let result = sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .map(|_| ())
            .map_err(|err| CreateDatabaseError::MySqlError(err.to_string()));

        if let Err(err) = &result {
            tracing::error!("Failed to create database '{}': {:?}", &database_name, err);
        }

        results.insert(database_name, result);
    }

    results
}

pub async fn drop_databases(
    database_names: &[MySQLDatabase],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> DropDatabasesResponse {
    let mut results = BTreeMap::new();

    for database_name in database_names.iter().cloned() {
        if let Err(err) = validate_db_or_user_request(
            &DbOrUser::Database(database_name.clone()),
            unix_user,
            group_denylist,
        )
        .map_err(DropDatabaseError::ValidationError)
        {
            results.insert(database_name.clone(), Err(err));
            continue;
        }

        match unsafe_database_exists(&database_name, &mut *connection).await {
            Ok(false) => {
                results.insert(
                    database_name.clone(),
                    Err(DropDatabaseError::DatabaseDoesNotExist),
                );
                continue;
            }
            Err(err) => {
                results.insert(
                    database_name.clone(),
                    Err(DropDatabaseError::MySqlError(err.to_string())),
                );
                continue;
            }
            _ => {}
        }

        let statement = AssertSqlSafe(format!(
            "DROP DATABASE {}",
            quote_identifier(&database_name)
        ));
        let result = sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .map(|_| ())
            .map_err(|err| DropDatabaseError::MySqlError(err.to_string()));

        if let Err(err) = &result {
            tracing::error!("Failed to drop database '{}': {:?}", &database_name, err);
        }

        results.insert(database_name, result);
    }

    results
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseRow {
    pub database: MySQLDatabase,
    pub tables: Vec<String>,
    pub users: Vec<MySQLUser>,
    pub collation: Option<String>,
    pub character_set: Option<String>,
    pub size_bytes: u64,
}

impl FromRow<'_, sqlx::mysql::MySqlRow> for DatabaseRow {
    fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        Ok(DatabaseRow {
            database: row.try_get::<String, _>("database")?.into(),
            tables: {
                let s: Option<String> = row.try_get("tables")?;
                s.and_then(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.split(',').map(std::borrow::ToOwned::to_owned).collect())
                    }
                })
                .unwrap_or_default()
            },
            users: {
                let s: Option<String> = row.try_get("users")?;
                s.and_then(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        Some(s.split(',').map(|s| s.to_owned().into()).collect())
                    }
                })
                .unwrap_or_default()
            },
            collation: row.try_get::<Option<String>, _>("collation")?,
            character_set: row.try_get::<Option<String>, _>("character_set")?,
            size_bytes: row.try_get::<u64, _>("size_bytes")?,
        })
    }
}

fn list_database_query(include_all_tables_and_users: bool) -> AssertSqlSafe<String> {
    let limit_clause = if include_all_tables_and_users {
        "".to_string()
    } else {
        format!(" LIMIT {}", MAX_SHOW_DB_RELATED_ITEMS)
    };

    AssertSqlSafe(format!(
        r"
            SELECT
                CAST(s.SCHEMA_NAME AS CHAR(64)) AS `database`,
                t.tables,
                u.users,
                s.DEFAULT_COLLATION_NAME AS `collation`,
                s.DEFAULT_CHARACTER_SET_NAME AS `character_set`,
                CAST(COALESCE(sz.size_bytes, 0) AS UNSIGNED) AS size_bytes
            FROM information_schema.SCHEMATA s

            LEFT JOIN (
                SELECT
                    x.TABLE_SCHEMA,
                    GROUP_CONCAT(x.TABLE_NAME ORDER BY x.TABLE_NAME SEPARATOR ',') AS tables
                FROM (
                    SELECT
                        TABLE_SCHEMA,
                        TABLE_NAME
                    FROM information_schema.TABLES
                    WHERE TABLE_SCHEMA = ?
                    ORDER BY TABLE_NAME{limit_clause}
                ) x
                GROUP BY x.TABLE_SCHEMA
            ) t
                ON t.TABLE_SCHEMA = s.SCHEMA_NAME

            LEFT JOIN (
                SELECT
                    x.DB,
                    GROUP_CONCAT(DISTINCT x.User ORDER BY x.User SEPARATOR ',') AS users
                FROM (
                    SELECT
                        DB,
                        User
                    FROM mysql.db
                    WHERE DB = ?
                    ORDER BY User{limit_clause}
                ) x
                GROUP BY x.DB
            ) u
                ON u.DB = s.SCHEMA_NAME

            LEFT JOIN (
                SELECT
                    TABLE_SCHEMA,
                    SUM(DATA_LENGTH + INDEX_LENGTH) AS size_bytes
                FROM information_schema.TABLES
                WHERE TABLE_SCHEMA = ?
                GROUP BY TABLE_SCHEMA
            ) sz
                ON sz.TABLE_SCHEMA = s.SCHEMA_NAME

            WHERE s.SCHEMA_NAME REGEXP ?
            AND s.SCHEMA_NAME NOT IN (
                'information_schema',
                'performance_schema',
                'mysql',
                'sys'
            )
        "
    ))
}

pub async fn list_databases(
    database_names: &[MySQLDatabase],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
    include_all_tables_and_users: bool,
) -> ListDatabasesResponse {
    let mut results = BTreeMap::new();

    for database_name in database_names.iter().cloned() {
        if let Err(err) = validate_db_or_user_request(
            &DbOrUser::Database(database_name.clone()),
            unix_user,
            group_denylist,
        )
        .map_err(ListDatabasesError::ValidationError)
        {
            results.insert(database_name.clone(), Err(err));
            continue;
        }

        let query = list_database_query(include_all_tables_and_users);

        let result = sqlx::query_as::<_, DatabaseRow>(query)
            .bind(database_name.to_string())
            .bind(database_name.to_string())
            .bind(database_name.to_string())
            .bind(database_name.to_string())
            .fetch_optional(&mut *connection)
            .await
            .map_err(|err| ListDatabasesError::MySqlError(err.to_string()))
            .and_then(|database| {
                database.map_or_else(|| Err(ListDatabasesError::DatabaseDoesNotExist), Ok)
            });

        if let Err(err) = &result {
            tracing::error!("Failed to list database '{}': {:?}", &database_name, err);
        }

        // TODO: should we assert that the users are also owned by the unix_user from the request?

        results.insert(database_name, result);
    }

    results
}

fn list_all_databases_for_user_query(include_all_tables_and_users: bool) -> AssertSqlSafe<String> {
    let limit_clause = if include_all_tables_and_users {
        "".to_string()
    } else {
        format!(" LIMIT {}", MAX_SHOW_DB_RELATED_ITEMS)
    };

    AssertSqlSafe(format!(
        r"
            SELECT
                CAST(s.SCHEMA_NAME AS CHAR(64)) AS `database`,
                t.tables,
                u.users,
                s.DEFAULT_COLLATION_NAME AS `collation`,
                s.DEFAULT_CHARACTER_SET_NAME AS `character_set`,
                CAST(COALESCE(sz.size_bytes, 0) AS UNSIGNED) AS size_bytes
            FROM information_schema.SCHEMATA s

            LEFT JOIN (
                SELECT
                    x.TABLE_SCHEMA,
                    GROUP_CONCAT(x.TABLE_NAME ORDER BY x.TABLE_NAME SEPARATOR ',') AS tables
                FROM (
                    SELECT
                        TABLE_SCHEMA,
                        TABLE_NAME
                    FROM information_schema.TABLES
                    WHERE TABLE_SCHEMA REGEXP ?
                    ORDER BY TABLE_NAME{limit_clause}
                ) x
                GROUP BY x.TABLE_SCHEMA
            ) t
                ON t.TABLE_SCHEMA = s.SCHEMA_NAME

            LEFT JOIN (
                SELECT
                    x.DB,
                    GROUP_CONCAT(DISTINCT x.User ORDER BY x.User SEPARATOR ',') AS users
                FROM (
                    SELECT
                        DB,
                        User
                    FROM mysql.db
                    WHERE DB REGEXP ?
                    ORDER BY User{limit_clause}
                ) x
                GROUP BY x.DB
            ) u
                ON u.DB = s.SCHEMA_NAME

            LEFT JOIN (
                SELECT
                    TABLE_SCHEMA,
                    SUM(DATA_LENGTH + INDEX_LENGTH) AS size_bytes
                FROM information_schema.TABLES
                WHERE TABLE_SCHEMA REGEXP ?
                GROUP BY TABLE_SCHEMA
            ) sz
                ON sz.TABLE_SCHEMA = s.SCHEMA_NAME

            WHERE s.SCHEMA_NAME REGEXP ?
            AND s.SCHEMA_NAME NOT IN (
                'information_schema',
                'performance_schema',
                'mysql',
                'sys'
            )

            ORDER BY s.SCHEMA_NAME
        "
    ))
}

pub async fn list_all_databases_for_user(
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
    include_all_tables_and_users: bool,
) -> ListAllDatabasesResponse {
    let query = list_all_databases_for_user_query(include_all_tables_and_users);
    let user_group_regex = create_user_group_matching_regex(unix_user, group_denylist);

    let result = sqlx::query_as::<_, DatabaseRow>(query)
        .bind(&user_group_regex)
        .bind(&user_group_regex)
        .bind(&user_group_regex)
        .bind(&user_group_regex)
        .fetch_all(connection)
        .await
        .map_err(|err| ListAllDatabasesError::MySqlError(err.to_string()));

    // TODO: should we assert that the users are also owned by the unix_user from the request?

    if let Err(err) = &result {
        tracing::error!(
            "Failed to list databases for user '{}': {:?}",
            unix_user.username,
            err
        );
    }

    result
}
