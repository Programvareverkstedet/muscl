// TODO: fix comment
//! Database privilege operations
//!
//! This module contains functions for querying, modifying,
//! displaying and comparing database privileges.
//!
//! A lot of the complexity comes from two core components:
//!
//! - The privilege editor that needs to be able to print
//!   an editable table of privileges and reparse the content
//!   after the user has made manual changes.
//!
//! - The comparison functionality that tells the user what
//!   changes will be made when applying a set of changes
//!   to the list of database privileges.

use std::collections::{BTreeMap, BTreeSet};

use indoc::indoc;
use itertools::Itertools;
use sqlx::{AssertSqlSafe, MySqlConnection, mysql::MySqlRow, prelude::*};

use crate::{
    core::{
        common::{UnixUser, rev_yn, yn},
        database_privileges::{
            DATABASE_PRIVILEGE_FIELDS, DatabasePrivilegeChange, DatabasePrivilegeRow,
            DatabasePrivilegesDiff,
        },
        protocol::{
            DiffDoesNotApplyError, ListAllPrivilegesError, ListAllPrivilegesResponse,
            ListPrivilegesError, ListPrivilegesResponse, ModifyDatabasePrivilegesError,
            ModifyPrivilegesResponse,
            request_validation::{GroupDenylist, validate_db_or_user_request},
        },
        types::{DbOrUser, MySQLDatabase, MySQLUser},
    },
    server::{
        common::{create_user_group_matching_regex, try_get_with_binary_fallback},
        sql::{
            database_operations::unsafe_database_exists, quote_identifier,
            user_operations::unsafe_user_exists,
        },
    },
};

// TODO: get by name instead of row tuple position

#[inline]
fn get_mysql_row_priv_field(row: &MySqlRow, position: usize) -> Result<bool, sqlx::Error> {
    let field = DATABASE_PRIVILEGE_FIELDS[position];
    let value = row.try_get(position)?;
    if let Some(val) = rev_yn(value) {
        Ok(val)
    } else {
        tracing::warn!(r#"Invalid value for privilege "{}": '{}'"#, field, value);
        Ok(false)
    }
}

impl FromRow<'_, MySqlRow> for DatabasePrivilegeRow {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            db: try_get_with_binary_fallback(row, "Db")?.into(),
            user: try_get_with_binary_fallback(row, "User")?.into(),
            select_priv: get_mysql_row_priv_field(row, 2)?,
            insert_priv: get_mysql_row_priv_field(row, 3)?,
            update_priv: get_mysql_row_priv_field(row, 4)?,
            delete_priv: get_mysql_row_priv_field(row, 5)?,
            create_priv: get_mysql_row_priv_field(row, 6)?,
            drop_priv: get_mysql_row_priv_field(row, 7)?,
            alter_priv: get_mysql_row_priv_field(row, 8)?,
            index_priv: get_mysql_row_priv_field(row, 9)?,
            create_tmp_table_priv: get_mysql_row_priv_field(row, 10)?,
            lock_tables_priv: get_mysql_row_priv_field(row, 11)?,
            references_priv: get_mysql_row_priv_field(row, 12)?,
        })
    }
}

// NOTE: this function is unsafe because it does no input validation.
/// Get all users + privileges for a single database.
async fn unsafe_get_database_privileges(
    database_name: &str,
    connection: &mut MySqlConnection,
) -> Result<Vec<DatabasePrivilegeRow>, sqlx::Error> {
    let statement = AssertSqlSafe(format!(
        "SELECT {} FROM `db` WHERE `Db` = ?",
        DATABASE_PRIVILEGE_FIELDS
            .iter()
            .map(|field| quote_identifier(field))
            .join(","),
    ));
    let result = sqlx::query_as::<_, DatabasePrivilegeRow>(statement)
        .bind(database_name)
        .fetch_all(connection)
        .await;

    if let Err(e) = &result {
        tracing::error!(
            "Failed to get database privileges for '{}': {}",
            &database_name,
            e
        );
    }

    result
}

// NOTE: this function is unsafe because it does no input validation.
/// Get all users + privileges for a single database-user pair.
pub async fn unsafe_get_database_privileges_for_db_user_pair(
    database_name: &MySQLDatabase,
    user_name: &MySQLUser,
    connection: &mut MySqlConnection,
) -> Result<Option<DatabasePrivilegeRow>, sqlx::Error> {
    let statement = AssertSqlSafe(format!(
        "SELECT {} FROM `db` WHERE `Db` = ? AND `User` = ? AND `Host` = '%'",
        DATABASE_PRIVILEGE_FIELDS
            .iter()
            .map(|field| quote_identifier(field))
            .join(","),
    ));
    let result = sqlx::query_as::<_, DatabasePrivilegeRow>(statement)
        .bind(database_name.as_str())
        .bind(user_name.as_str())
        .fetch_optional(connection)
        .await;

    if let Err(e) = &result {
        tracing::error!(
            "Failed to get database privileges for '{}.{}': {}",
            &database_name,
            &user_name,
            e
        );
    }

    result
}

pub async fn get_databases_privilege_data(
    database_names: &[MySQLDatabase],
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> ListPrivilegesResponse {
    let mut results = BTreeMap::new();

    for database_name in database_names.iter().cloned() {
        if let Err(err) = validate_db_or_user_request(
            &DbOrUser::Database(database_name.to_owned()),
            unix_user,
            group_denylist,
        )
        .map_err(ListPrivilegesError::ValidationError)
        {
            results.insert(database_name, Err(err));
            continue;
        }

        match unsafe_database_exists(&database_name, connection).await {
            Ok(false) => {
                results.insert(
                    database_name.to_owned(),
                    Err(ListPrivilegesError::DatabaseDoesNotExist),
                );
                continue;
            }
            Err(e) => {
                results.insert(
                    database_name.to_owned(),
                    Err(ListPrivilegesError::MySqlError(e.to_string())),
                );
                continue;
            }
            Ok(true) => {}
        }

        let result = unsafe_get_database_privileges(&database_name, connection)
            .await
            .map_err(|e| ListPrivilegesError::MySqlError(e.to_string()));

        results.insert(database_name.to_owned(), result);
    }

    debug_assert!(database_names.len() == results.len());

    results
}

/// TODO: make this constant
fn get_all_db_privs_query() -> AssertSqlSafe<String> {
    AssertSqlSafe(format!(
        indoc! {r"
            SELECT {} FROM `db` WHERE `db` IN
            (SELECT DISTINCT CAST(`SCHEMA_NAME` AS CHAR(64)) AS `database`
              FROM `information_schema`.`SCHEMATA`
              WHERE `SCHEMA_NAME` NOT IN ('information_schema', 'performance_schema', 'mysql', 'sys')
                AND `SCHEMA_NAME` REGEXP ?)
        "},
        DATABASE_PRIVILEGE_FIELDS
            .iter()
            .map(|field| quote_identifier(field))
            .join(","),
    ))
}

/// Get all database + user + privileges pairs that are owned by the current user.
pub async fn get_all_database_privileges(
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> ListAllPrivilegesResponse {
    let result = sqlx::query_as::<_, DatabasePrivilegeRow>(get_all_db_privs_query())
        .bind(create_user_group_matching_regex(unix_user, group_denylist))
        .fetch_all(connection)
        .await
        .map_err(|e| ListAllPrivilegesError::MySqlError(e.to_string()));

    if let Err(e) = &result {
        tracing::error!("Failed to get all database privileges: {:?}", e);
    }

    result
}

// TODO: make these queries constant strings.
async fn unsafe_apply_privilege_diff(
    database_privilege_diff: &DatabasePrivilegesDiff,
    connection: &mut MySqlConnection,
) -> Result<(), sqlx::Error> {
    let result = match database_privilege_diff {
        DatabasePrivilegesDiff::New(p) => {
            let tables = DATABASE_PRIVILEGE_FIELDS
                .iter()
                .chain(&["Host"])
                .map(|field| quote_identifier(field))
                .join(",");

            let question_marks =
                std::iter::repeat_n("?", DATABASE_PRIVILEGE_FIELDS.len() + 1).join(",");

            let statement = AssertSqlSafe(format!(
                "INSERT INTO `db` ({tables}) VALUES ({question_marks})"
            ));
            sqlx::query(statement)
                .bind(p.db.to_string())
                .bind(p.user.to_string())
                .bind(yn(p.select_priv))
                .bind(yn(p.insert_priv))
                .bind(yn(p.update_priv))
                .bind(yn(p.delete_priv))
                .bind(yn(p.create_priv))
                .bind(yn(p.drop_priv))
                .bind(yn(p.alter_priv))
                .bind(yn(p.index_priv))
                .bind(yn(p.create_tmp_table_priv))
                .bind(yn(p.lock_tables_priv))
                .bind(yn(p.references_priv))
                .bind("%")
                .execute(connection)
                .await
                .map(|_| ())
        }
        DatabasePrivilegesDiff::Modified(p) => {
            let changes = DATABASE_PRIVILEGE_FIELDS
                .iter()
                .skip(2) // Skip Db and User fields
                .map(|field| {
                    format!(
                        "{} = COALESCE(?, {})",
                        quote_identifier(field),
                        quote_identifier(field)
                    )
                })
                .join(",");

            fn change_to_yn(change: DatabasePrivilegeChange) -> &'static str {
                match change {
                    DatabasePrivilegeChange::YesToNo => "N",
                    DatabasePrivilegeChange::NoToYes => "Y",
                }
            }

            let statement = AssertSqlSafe(format!(
                "UPDATE `db` SET {changes} WHERE `Db` = ? AND `User` = ? AND `Host` = ?"
            ));
            sqlx::query(statement)
                .bind(p.select_priv.map(change_to_yn))
                .bind(p.insert_priv.map(change_to_yn))
                .bind(p.update_priv.map(change_to_yn))
                .bind(p.delete_priv.map(change_to_yn))
                .bind(p.create_priv.map(change_to_yn))
                .bind(p.drop_priv.map(change_to_yn))
                .bind(p.alter_priv.map(change_to_yn))
                .bind(p.index_priv.map(change_to_yn))
                .bind(p.create_tmp_table_priv.map(change_to_yn))
                .bind(p.lock_tables_priv.map(change_to_yn))
                .bind(p.references_priv.map(change_to_yn))
                .bind(p.db.to_string())
                .bind(p.user.to_string())
                .bind("%")
                .execute(connection)
                .await
                .map(|_| ())
        }
        DatabasePrivilegesDiff::Deleted(p) => {
            sqlx::query("DELETE FROM `db` WHERE `Db` = ? AND `User` = ? AND `Host` = ?")
                .bind(p.db.to_string())
                .bind(p.user.to_string())
                .bind("%")
                .execute(connection)
                .await
                .map(|_| ())
        }
        DatabasePrivilegesDiff::Noop { .. } => Ok(()),
    };

    if let Err(e) = &result {
        tracing::error!("Failed to apply database privilege diff: {}", e);
    }

    result
}

async fn validate_diff(
    diff: &DatabasePrivilegesDiff,
    connection: &mut MySqlConnection,
) -> Result<(), ModifyDatabasePrivilegesError> {
    let privilege_row = unsafe_get_database_privileges_for_db_user_pair(
        diff.get_database_name(),
        diff.get_user_name(),
        connection,
    )
    .await;

    let privilege_row = match privilege_row {
        Ok(privilege_row) => privilege_row,
        Err(e) => return Err(ModifyDatabasePrivilegesError::MySqlError(e.to_string())),
    };

    match diff {
        DatabasePrivilegesDiff::New(_) => {
            if privilege_row.is_some() {
                Err(ModifyDatabasePrivilegesError::DiffDoesNotApply(
                    DiffDoesNotApplyError::RowAlreadyExists(
                        diff.get_database_name().to_owned(),
                        diff.get_user_name().to_owned(),
                    ),
                ))
            } else {
                Ok(())
            }
        }
        DatabasePrivilegesDiff::Modified(_) if privilege_row.is_none() => {
            Err(ModifyDatabasePrivilegesError::DiffDoesNotApply(
                DiffDoesNotApplyError::RowDoesNotExist(
                    diff.get_database_name().to_owned(),
                    diff.get_user_name().to_owned(),
                ),
            ))
        }
        DatabasePrivilegesDiff::Modified(row_diff) => {
            let row = privilege_row.unwrap();

            let error_exists = DATABASE_PRIVILEGE_FIELDS
                .iter()
                .skip(2) // Skip Db and User fields
                .any(
                    |field| match row_diff.get_privilege_change_by_name(field).unwrap() {
                        Some(DatabasePrivilegeChange::YesToNo) => {
                            !row.get_privilege_by_name(field).unwrap()
                        }
                        Some(DatabasePrivilegeChange::NoToYes) => {
                            row.get_privilege_by_name(field).unwrap()
                        }
                        None => false,
                    },
                );

            if error_exists {
                Err(ModifyDatabasePrivilegesError::DiffDoesNotApply(
                    DiffDoesNotApplyError::RowPrivilegeChangeDoesNotApply(row_diff.to_owned(), row),
                ))
            } else {
                Ok(())
            }
        }
        DatabasePrivilegesDiff::Deleted(_) => {
            if privilege_row.is_none() {
                Err(ModifyDatabasePrivilegesError::DiffDoesNotApply(
                    DiffDoesNotApplyError::RowDoesNotExist(
                        diff.get_database_name().to_owned(),
                        diff.get_user_name().to_owned(),
                    ),
                ))
            } else {
                Ok(())
            }
        }
        DatabasePrivilegesDiff::Noop { .. } => {
            tracing::warn!(
                "Server got sent a noop database privilege diff to validate, is the client buggy?"
            );
            Ok(())
        }
    }
}

/// Uses the result of [`diff_privileges`] to modify privileges in the database.
pub async fn apply_privilege_diffs(
    database_privilege_diffs: &BTreeSet<DatabasePrivilegesDiff>,
    unix_user: &UnixUser,
    connection: &mut MySqlConnection,
    _db_is_mariadb: bool,
    group_denylist: &GroupDenylist,
) -> ModifyPrivilegesResponse {
    let mut results: BTreeMap<(MySQLDatabase, MySQLUser), _> = BTreeMap::new();

    for diff in database_privilege_diffs {
        let key = (
            diff.get_database_name().to_owned(),
            diff.get_user_name().to_owned(),
        );
        if let Err(err) = validate_db_or_user_request(
            &DbOrUser::Database(diff.get_database_name().to_owned()),
            unix_user,
            group_denylist,
        )
        .map_err(ModifyDatabasePrivilegesError::UserValidationError)
        {
            results.insert(key, Err(err));
            continue;
        }

        if let Err(err) = validate_db_or_user_request(
            &DbOrUser::User(diff.get_user_name().to_owned()),
            unix_user,
            group_denylist,
        )
        .map_err(ModifyDatabasePrivilegesError::UserValidationError)
        {
            results.insert(key, Err(err));
            continue;
        }

        match unsafe_database_exists(diff.get_database_name(), connection).await {
            Ok(false) => {
                results.insert(
                    key,
                    Err(ModifyDatabasePrivilegesError::DatabaseDoesNotExist),
                );
                continue;
            }
            Err(e) => {
                results.insert(
                    key,
                    Err(ModifyDatabasePrivilegesError::MySqlError(e.to_string())),
                );
                continue;
            }
            Ok(true) => {}
        }

        match unsafe_user_exists(diff.get_user_name(), connection).await {
            Ok(false) => {
                results.insert(key, Err(ModifyDatabasePrivilegesError::UserDoesNotExist));
                continue;
            }
            Err(e) => {
                results.insert(
                    key,
                    Err(ModifyDatabasePrivilegesError::MySqlError(e.to_string())),
                );
                continue;
            }
            Ok(true) => {}
        }

        if let Err(err) = validate_diff(diff, connection).await {
            results.insert(key, Err(err));
            continue;
        }

        let result = unsafe_apply_privilege_diff(diff, connection)
            .await
            .map_err(|e| ModifyDatabasePrivilegesError::MySqlError(e.to_string()));

        results.insert(key, result);
    }

    if let Err(err) = connection.execute("FLUSH PRIVILEGES").await {
        tracing::error!("Failed to flush privileges: {}", err);
    }

    results
        .into_iter()
        .map(|((k1, k2), v)| (k1, (k2, v)))
        .into_group_map()
        .into_iter()
        .map(|(k1, pairs)| {
            let inner = pairs.into_iter().collect::<BTreeMap<_, _>>();
            (k1, inner)
        })
        .collect()
}
