pub mod database_operations;
pub mod database_privilege_operations;
pub mod user_operations;

#[inline]
#[must_use]
pub fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\\', r"\\").replace('\'', "''"))
}

#[inline]
#[must_use]
pub fn quote_identifier(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

/// Escape the `LIKE`-style wildcards (`_` and `%`), as well as the escape
/// character itself (`\`), in a database name.
pub fn escape_db_name_wildcards(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(c, '\\' | '_' | '%') {
            result.push('\\');
        }
        result.push(c);
    }
    result
}

/// Inverse of [`escape_db_name_wildcards`].
pub fn unescape_db_name_wildcards(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(next) = chars.next()
        {
            result.push(next);
        } else {
            result.push(c);
        }
    }
    result
}

/// SQL expression which does the same as [`unescape_db_name_wildcards`] for a
/// column containing values from the `Db` column of `mysql.db`.
pub fn unescape_db_name_wildcards_sql(column: &str) -> String {
    format!(r"REPLACE(REPLACE(REPLACE({column}, '\\%', '%'), '\\_', '_'), '\\\\', '\\')")
}

/// Check whether the `Db` column of `mysql.db` is interpreted as a pattern
/// by the database server.
///
/// If so, it should be escaped with [`escape_db_name_wildcards`].
pub async fn db_column_uses_wildcards(
    connection: &mut sqlx::MySqlConnection,
    db_is_mariadb: bool,
) -> bool {
    if db_is_mariadb {
        return true;
    }

    // NOTE: `partial_revokes` was added in MySQL 8.0.16.
    match sqlx::query_scalar::<_, i64>("SELECT CAST(@@GLOBAL.partial_revokes AS SIGNED)")
        .fetch_one(connection)
        .await
    {
        Ok(partial_revokes) => partial_revokes == 0,
        Err(err) => {
            tracing::debug!(
                "Failed to query partial_revokes, assuming wildcards are in use: {}",
                err
            );
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_db_name_wildcards() {
        assert_eq!(escape_db_name_wildcards("alice_db"), r"alice\_db");
        assert_eq!(escape_db_name_wildcards("alice-db"), "alice-db");
        assert_eq!(escape_db_name_wildcards(r"a_%\b"), r"a\_\%\\b");
    }

    #[test]
    fn test_unescape_db_name_wildcards() {
        for name in ["alice_db", "alice-db", r"a_%\b", "bob_____", ""] {
            assert_eq!(
                unescape_db_name_wildcards(&escape_db_name_wildcards(name)),
                name
            );
        }

        // Legacy unescaped values are left as-is
        assert_eq!(unescape_db_name_wildcards("alice_db"), "alice_db");
    }

    #[test]
    fn test_quote_literal() {
        let payload = "' OR 1=1 --";
        assert_eq!(quote_literal(payload), "''' OR 1=1 --'");
    }

    #[test]
    fn test_quote_literal_escapes_backslash_before_quote() {
        let payload = r"\'; SELECT SLEEP(5); -- ";
        assert_eq!(quote_literal(payload), r"'\\''; SELECT SLEEP(5); -- '");
    }

    #[test]
    fn test_quote_identifier() {
        let payload = "` OR 1=1 --";
        assert_eq!(quote_identifier(payload), r#"``` OR 1=1 --`"#);
    }
}
