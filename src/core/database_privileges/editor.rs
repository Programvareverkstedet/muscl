//! This module contains serialization and deserialization logic for
//! editing database privileges in a text editor.

use super::base::{
    DATABASE_PRIVILEGE_FIELDS, DatabasePrivilegeRow, db_priv_field_human_readable_name,
};
use crate::core::{
    common::{rev_yn, yn},
    types::MySQLDatabase,
};
use anyhow::{Context, anyhow};
use itertools::Itertools;
use std::cmp::max;

/// Generates a single row of the privileges table for the editor.
#[must_use]
pub fn format_privileges_line_for_editor(
    privs: &DatabasePrivilegeRow,
    database_name_len: usize,
    username_len: usize,
) -> String {
    DATABASE_PRIVILEGE_FIELDS
        .into_iter()
        .map(|field| match field {
            "Db" => format!("{:width$}", privs.db, width = database_name_len),
            "User" => format!("{:width$}", privs.user, width = username_len),
            privilege => format!(
                "{:width$}",
                // SAFETY: unwrap is safe here because the field names are static
                yn(privs.get_privilege_by_name(privilege).unwrap()),
                width = db_priv_field_human_readable_name(privilege).len()
            ),
        })
        .join(" ")
        .trim()
        .to_string()
}

const EDITOR_COMMENT: &str = r"
# Welcome to the privilege editor.
# Each line defines what privileges a single user has on a single database.
# The first two columns respectively represent the database name and the user, and the remaining columns are the privileges.
# If the user should have a certain privilege, write 'Y', otherwise write 'N'.
#
# Lines starting with '#' are comments and will be ignored.
";

/// Generates the content for the privilege editor.
///
/// The unix user is used in case there are no privileges to edit,
/// so that the user can see an example line based on their username.
pub fn generate_editor_content_from_privilege_data(
    privilege_data: &[DatabasePrivilegeRow],
    unix_user: &str,
    database_name: Option<&MySQLDatabase>,
) -> String {
    let example_user = format!("{unix_user}_user");
    let example_db = database_name
        .unwrap_or(&format!("{unix_user}_db").into())
        .to_string();

    // NOTE: `.max()`` fails when the iterator is empty.
    //       In this case, we know that the only fields in the
    //       editor will be the example user and example db name.
    //       Hence, it's put as the fallback value, despite not really
    //       being a "fallback" in the normal sense.
    let longest_username = max(
        privilege_data
            .iter()
            .map(|p| p.user.len())
            .max()
            .unwrap_or(example_user.len()),
        "User".len(),
    );

    let longest_database_name = max(
        privilege_data
            .iter()
            .map(|p| p.db.len())
            .max()
            .unwrap_or(example_db.len()),
        "Database".len(),
    );

    let mut header: Vec<_> = DATABASE_PRIVILEGE_FIELDS
        .into_iter()
        .map(db_priv_field_human_readable_name)
        .collect();

    // Pad the first two columns with spaces to align the privileges.
    header[0] = format!("{:width$}", header[0], width = longest_database_name);
    header[1] = format!("{:width$}", header[1], width = longest_username);

    let example_line = format_privileges_line_for_editor(
        &DatabasePrivilegeRow {
            db: example_db.into(),
            user: example_user.into(),
            select_priv: true,
            insert_priv: true,
            update_priv: true,
            delete_priv: true,
            create_priv: false,
            drop_priv: false,
            alter_priv: false,
            index_priv: false,
            create_tmp_table_priv: false,
            lock_tables_priv: false,
            references_priv: false,
            create_view_priv: false,
            show_view_priv: false,
            trigger_priv: false,
        },
        longest_database_name,
        longest_username,
    );

    format!(
        "{}\n{}\n{}",
        EDITOR_COMMENT,
        header.join(" "),
        if privilege_data.is_empty() {
            format!("# {example_line}")
        } else {
            privilege_data
                .iter()
                .map(|privs| {
                    format_privileges_line_for_editor(
                        privs,
                        longest_database_name,
                        longest_username,
                    )
                })
                .join("\n")
        }
    )
}

#[derive(Debug)]
enum PrivilegeRowParseResult {
    PrivilegeRow(DatabasePrivilegeRow),
    ParserError(anyhow::Error),
    TooFewFields(usize),
    TooManyFields(usize),
    Header,
    Comment,
    Empty,
}

#[inline]
fn parse_privilege_cell_from_editor(yn: &str, name: &str) -> anyhow::Result<bool> {
    let human_readable_name = db_priv_field_human_readable_name(name);
    rev_yn(yn)
        .ok_or_else(|| anyhow!("Expected Y or N, found {yn}"))
        .context(format!("Could not parse '{human_readable_name}' privilege"))
}

#[inline]
fn editor_row_is_header(row: &str) -> bool {
    row.split_ascii_whitespace()
        .zip(DATABASE_PRIVILEGE_FIELDS.iter())
        .map(|(field, priv_name)| (field, db_priv_field_human_readable_name(priv_name)))
        .all(|(field, header_field)| field == header_field)
}

/// Parse a single row of the privileges table from the editor.
fn parse_privilege_row_from_editor(row: &str) -> PrivilegeRowParseResult {
    if row.starts_with('#') || row.starts_with("//") {
        return PrivilegeRowParseResult::Comment;
    }

    if row.trim().is_empty() {
        return PrivilegeRowParseResult::Empty;
    }

    let parts: Vec<&str> = row.trim().split_ascii_whitespace().collect();

    match parts.len() {
        n if (n < DATABASE_PRIVILEGE_FIELDS.len()) => {
            return PrivilegeRowParseResult::TooFewFields(n);
        }
        n if (n > DATABASE_PRIVILEGE_FIELDS.len()) => {
            return PrivilegeRowParseResult::TooManyFields(n);
        }
        _ => {}
    }

    if editor_row_is_header(row) {
        return PrivilegeRowParseResult::Header;
    }

    let row = DatabasePrivilegeRow {
        db: (*parts.first().unwrap()).into(),
        user: (*parts.get(1).unwrap()).into(),
        select_priv: match parse_privilege_cell_from_editor(
            parts.get(2).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[2],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        insert_priv: match parse_privilege_cell_from_editor(
            parts.get(3).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[3],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        update_priv: match parse_privilege_cell_from_editor(
            parts.get(4).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[4],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        delete_priv: match parse_privilege_cell_from_editor(
            parts.get(5).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[5],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        create_priv: match parse_privilege_cell_from_editor(
            parts.get(6).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[6],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        drop_priv: match parse_privilege_cell_from_editor(
            parts.get(7).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[7],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        alter_priv: match parse_privilege_cell_from_editor(
            parts.get(8).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[8],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        index_priv: match parse_privilege_cell_from_editor(
            parts.get(9).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[9],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        create_tmp_table_priv: match parse_privilege_cell_from_editor(
            parts.get(10).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[10],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        lock_tables_priv: match parse_privilege_cell_from_editor(
            parts.get(11).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[11],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        references_priv: match parse_privilege_cell_from_editor(
            parts.get(12).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[12],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        create_view_priv: match parse_privilege_cell_from_editor(
            parts.get(13).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[13],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        show_view_priv: match parse_privilege_cell_from_editor(
            parts.get(14).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[14],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
        trigger_priv: match parse_privilege_cell_from_editor(
            parts.get(15).unwrap(),
            DATABASE_PRIVILEGE_FIELDS[15],
        ) {
            Ok(p) => p,
            Err(e) => return PrivilegeRowParseResult::ParserError(e),
        },
    };

    PrivilegeRowParseResult::PrivilegeRow(row)
}

#[derive(Debug, Clone)]
pub struct PrivilegeLineError {
    pub line_number: usize,
    pub message: String,
}

pub fn parse_privilege_data_from_editor_content(
    content: &str,
) -> Result<Vec<DatabasePrivilegeRow>, Vec<PrivilegeLineError>> {
    let mut rows = Vec::new();
    let mut errors = Vec::new();

    for (line_number, line) in content.lines().map(str::trim).enumerate() {
        match parse_privilege_row_from_editor(line) {
            PrivilegeRowParseResult::PrivilegeRow(row) => rows.push(row),
            PrivilegeRowParseResult::ParserError(e) => errors.push(PrivilegeLineError {
                line_number,
                message: format!("{e:#}"),
            }),
            PrivilegeRowParseResult::TooFewFields(n) => errors.push(PrivilegeLineError {
                line_number,
                message: format!(
                    "Too few fields: expected {}, found {n}",
                    DATABASE_PRIVILEGE_FIELDS.len(),
                ),
            }),
            PrivilegeRowParseResult::TooManyFields(n) => errors.push(PrivilegeLineError {
                line_number,
                message: format!(
                    "Too many fields: expected {}, found {n}",
                    DATABASE_PRIVILEGE_FIELDS.len(),
                ),
            }),
            PrivilegeRowParseResult::Header
            | PrivilegeRowParseResult::Comment
            | PrivilegeRowParseResult::Empty => {}
        }
    }

    if errors.is_empty() {
        Ok(rows)
    } else {
        Err(errors)
    }
}

pub fn format_privilege_row_header_for(line: &str) -> String {
    let mut header: Vec<_> = DATABASE_PRIVILEGE_FIELDS
        .into_iter()
        .map(db_priv_field_human_readable_name)
        .collect();

    let splitline = line.split_ascii_whitespace().collect::<Vec<&str>>();
    let dbname = splitline.first().unwrap_or(&"");
    let username = splitline.get(1).unwrap_or(&"");

    header[0] = format!("{:width$}", header[0], width = dbname.len());
    header[1] = format!("{:width$}", header[1], width = username.len());

    header.join(" ")
}

const ERROR_MARKER_PREFIX: &str = "# ^ ERROR: ";
const ERROR_CONTINUATION_PREFIX: &str = "#          ";

/// Inline error messages into the editor content, so that the user can easily see what went wrong.
pub fn inline_errors_into_editor_content(content: &str, errors: &[PrivilegeLineError]) -> String {
    content
        .lines()
        .enumerate()
        .flat_map(|(line_number, line)| {
            let comments = errors
                .iter()
                .filter(move |e| e.line_number == line_number)
                .flat_map(|e| e.message.lines())
                .enumerate()
                .map(|(i, message_line)| {
                    if i == 0 {
                        format!("{ERROR_MARKER_PREFIX}{message_line}")
                    } else {
                        format!("{ERROR_CONTINUATION_PREFIX}{message_line}")
                    }
                });

            std::iter::once(line.to_string()).chain(comments)
        })
        .join("\n")
}

/// Remove any error annotations previously added by [`inline_errors_into_editor_content`].
pub fn strip_inlined_errors(content: &str) -> String {
    content
        .lines()
        .scan(false, |in_error_block, line| {
            *in_error_block = line.starts_with(ERROR_MARKER_PREFIX)
                || (*in_error_block && line.starts_with(ERROR_CONTINUATION_PREFIX));
            Some((line, *in_error_block))
        })
        .filter_map(|(line, in_error_block)| (!in_error_block).then_some(line))
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_generate_editor_content_from_privilege_data() {
        let permissions = vec![
            DatabasePrivilegeRow {
                db: "test_abcdef".into(),
                user: "test_abcdef".into(),
                select_priv: true,
                insert_priv: false,
                update_priv: true,
                delete_priv: false,
                create_priv: true,
                drop_priv: false,
                alter_priv: true,
                index_priv: false,
                create_tmp_table_priv: true,
                lock_tables_priv: false,
                references_priv: true,
                create_view_priv: false,
                show_view_priv: true,
                trigger_priv: false,
            },
            DatabasePrivilegeRow {
                db: "test_abcdefghijlkmno".into(),
                user: "test_abcdef".into(),
                select_priv: true,
                insert_priv: false,
                update_priv: true,
                delete_priv: false,
                create_priv: true,
                drop_priv: false,
                alter_priv: true,
                index_priv: false,
                create_tmp_table_priv: true,
                lock_tables_priv: false,
                references_priv: true,
                create_view_priv: false,
                show_view_priv: true,
                trigger_priv: false,
            },
        ];

        let content = generate_editor_content_from_privilege_data(&permissions, "test", None);

        let expected_lines = vec![
            "",
            "# Welcome to the privilege editor.",
            "# Each line defines what privileges a single user has on a single database.",
            "# The first two columns respectively represent the database name and the user, and the remaining columns are the privileges.",
            "# If the user should have a certain privilege, write 'Y', otherwise write 'N'.",
            "#",
            "# Lines starting with '#' are comments and will be ignored.",
            "",
            "Database             User        Select Insert Update Delete Create Drop Alter Index Temp Lock References CreateView ShowView Trigger",
            "test_abcdef          test_abcdef Y      N      Y      N      Y      N    Y     N     Y    N    Y          N          Y        N",
            "test_abcdefghijlkmno test_abcdef Y      N      Y      N      Y      N    Y     N     Y    N    Y          N          Y        N",
        ];

        let generated_lines: Vec<&str> = content.lines().collect();

        assert_eq!(generated_lines, expected_lines);
    }

    #[test]
    fn ensure_generated_and_parsed_editor_content_is_equal() {
        let permissions = vec![
            DatabasePrivilegeRow {
                db: "db".into(),
                user: "user".into(),
                select_priv: true,
                insert_priv: true,
                update_priv: true,
                delete_priv: true,
                create_priv: true,
                drop_priv: true,
                alter_priv: true,
                index_priv: true,
                create_tmp_table_priv: true,
                lock_tables_priv: true,
                references_priv: true,
                create_view_priv: true,
                show_view_priv: true,
                trigger_priv: true,
            },
            DatabasePrivilegeRow {
                db: "db".into(),
                user: "user".into(),
                select_priv: false,
                insert_priv: false,
                update_priv: false,
                delete_priv: false,
                create_priv: false,
                drop_priv: false,
                alter_priv: false,
                index_priv: false,
                create_tmp_table_priv: false,
                lock_tables_priv: false,
                references_priv: false,
                create_view_priv: false,
                show_view_priv: false,
                trigger_priv: false,
            },
        ];

        let content = generate_editor_content_from_privilege_data(&permissions, "user", None);

        let parsed_permissions = parse_privilege_data_from_editor_content(&content).unwrap();

        assert_eq!(permissions, parsed_permissions);
    }

    #[test]
    fn test_parse_privilege_data_from_editor_content_collects_all_errors() {
        let content = indoc! {"
            # This is a comment and should be ignored.

            db1 user1 Y Y Y Y Y Y Y Y Y Y Y Y Y Y

            # Another comment
            db2 user2 X Y Y Y Y Y Y Y Y Y Y Y Y Y
            db3 user3 too few fields

            db4 user4 Y N Y N Y N Y N Y N Y N Y N
            db5 user5 Y Y Y Y Y Y Y Y Y Y Y Y Y too many fields
        "};

        let errors = parse_privilege_data_from_editor_content(content).unwrap_err();

        assert_eq!(errors.len(), 3);

        assert_eq!(errors[0].line_number, 5);
        assert!(errors[0].message.contains("Select"));

        assert_eq!(errors[1].line_number, 6);
        assert!(errors[1].message.contains("Too few fields"));

        assert_eq!(errors[2].line_number, 9);
        assert!(errors[2].message.contains("Too many fields"));
    }

    #[test]
    fn test_inline_errors_into_editor_content() {
        let content = indoc! {"
            # A comment before anything else.

            db1 user1 Y Y Y Y Y Y Y Y Y Y Y Y Y Y
            db2 user2 X Y Y Y Y Y Y Y Y Y Y Y Y Y

            # A comment between the two invalid lines.
            db3 user3 too few fields
            db4 user4 Y N Y N Y N Y N Y N Y N Y N
        "};

        let errors = vec![
            PrivilegeLineError {
                line_number: 3,
                message: "Expected Y or N, found X".to_string(),
            },
            PrivilegeLineError {
                line_number: 6,
                message: "Expected 16 fields, found 5".to_string(),
            },
            PrivilegeLineError {
                line_number: 7,
                message: "Could not parse privilege row:\nExpected Y or N, found Q".to_string(),
            },
        ];

        let result = inline_errors_into_editor_content(content, &errors);

        let expected = indoc! {"
            # A comment before anything else.

            db1 user1 Y Y Y Y Y Y Y Y Y Y Y Y Y Y
            db2 user2 X Y Y Y Y Y Y Y Y Y Y Y Y Y
            # ^ ERROR: Expected Y or N, found X

            # A comment between the two invalid lines.
            db3 user3 too few fields
            # ^ ERROR: Expected 16 fields, found 5
            db4 user4 Y N Y N Y N Y N Y N Y N Y N
            # ^ ERROR: Could not parse privilege row:
            #          Expected Y or N, found Q
        "};

        assert_eq!(result, expected.trim_end());
    }

    #[test]
    fn test_strip_inlined_errors_recovers_original_content() {
        let content = indoc! {"
            # A comment before anything else.

            db1 user1 Y Y Y Y Y Y Y Y Y Y Y Y Y Y
            db2 user2 X Y Y Y Y Y Y Y Y Y Y Y Y Y

            # A comment between the two invalid lines.
            db3 user3 too few fields
            db4 user4 Y N Y N Y N Y N Y N Y N Y N
        "};
        let content = content.trim_end();

        let errors = vec![
            PrivilegeLineError {
                line_number: 3,
                message: "Expected Y or N, found X".to_string(),
            },
            PrivilegeLineError {
                line_number: 6,
                message: "Could not parse privilege row:\nExpected Y or N, found Q".to_string(),
            },
        ];

        let inlined = inline_errors_into_editor_content(content, &errors);
        assert_ne!(inlined, content);
        assert_eq!(strip_inlined_errors(&inlined), content);
    }
}
