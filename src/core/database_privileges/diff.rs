//! This module contains datastructures and logic for comparing database privileges,
//! generating, validating and reducing diffs between two sets of database privileges.

use super::base::{DatabasePrivilegeRow, db_priv_field_human_readable_name};
use crate::core::types::{MySQLDatabase, MySQLUser};
use prettytable::Table;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap, hash_map::Entry},
    fmt,
};

/// This enum represents a change for a single privilege.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub enum DatabasePrivilegeChange {
    YesToNo,
    NoToYes,
}

impl DatabasePrivilegeChange {
    #[must_use]
    pub fn new(p1: bool, p2: bool) -> Option<DatabasePrivilegeChange> {
        match (p1, p2) {
            (true, false) => Some(DatabasePrivilegeChange::YesToNo),
            (false, true) => Some(DatabasePrivilegeChange::NoToYes),
            _ => None,
        }
    }
}

/// This struct encapsulates the before and after states of the
/// access privileges for a single user on a single database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Default)]
pub struct DatabasePrivilegeRowDiff {
    // TODO: don't store the db and user here, let the type be stored in a mapping
    pub db: MySQLDatabase,
    pub user: MySQLUser,
    pub select_priv: Option<DatabasePrivilegeChange>,
    pub insert_priv: Option<DatabasePrivilegeChange>,
    pub update_priv: Option<DatabasePrivilegeChange>,
    pub delete_priv: Option<DatabasePrivilegeChange>,
    pub create_priv: Option<DatabasePrivilegeChange>,
    pub drop_priv: Option<DatabasePrivilegeChange>,
    pub alter_priv: Option<DatabasePrivilegeChange>,
    pub index_priv: Option<DatabasePrivilegeChange>,
    pub create_tmp_table_priv: Option<DatabasePrivilegeChange>,
    pub lock_tables_priv: Option<DatabasePrivilegeChange>,
    pub references_priv: Option<DatabasePrivilegeChange>,
    pub create_view_priv: Option<DatabasePrivilegeChange>,
    pub show_view_priv: Option<DatabasePrivilegeChange>,
    pub trigger_priv: Option<DatabasePrivilegeChange>,
}

impl DatabasePrivilegeRowDiff {
    /// Calculates the difference between two [`DatabasePrivilegeRow`] instances.
    #[must_use]
    pub fn from_rows(
        row1: &DatabasePrivilegeRow,
        row2: &DatabasePrivilegeRow,
    ) -> DatabasePrivilegeRowDiff {
        debug_assert!(row1.db == row2.db && row1.user == row2.user);

        DatabasePrivilegeRowDiff {
            db: row1.db.clone(),
            user: row1.user.clone(),
            select_priv: DatabasePrivilegeChange::new(row1.select_priv, row2.select_priv),
            insert_priv: DatabasePrivilegeChange::new(row1.insert_priv, row2.insert_priv),
            update_priv: DatabasePrivilegeChange::new(row1.update_priv, row2.update_priv),
            delete_priv: DatabasePrivilegeChange::new(row1.delete_priv, row2.delete_priv),
            create_priv: DatabasePrivilegeChange::new(row1.create_priv, row2.create_priv),
            drop_priv: DatabasePrivilegeChange::new(row1.drop_priv, row2.drop_priv),
            alter_priv: DatabasePrivilegeChange::new(row1.alter_priv, row2.alter_priv),
            index_priv: DatabasePrivilegeChange::new(row1.index_priv, row2.index_priv),
            create_tmp_table_priv: DatabasePrivilegeChange::new(
                row1.create_tmp_table_priv,
                row2.create_tmp_table_priv,
            ),
            lock_tables_priv: DatabasePrivilegeChange::new(
                row1.lock_tables_priv,
                row2.lock_tables_priv,
            ),
            references_priv: DatabasePrivilegeChange::new(
                row1.references_priv,
                row2.references_priv,
            ),
            create_view_priv: DatabasePrivilegeChange::new(
                row1.create_view_priv,
                row2.create_view_priv,
            ),
            show_view_priv: DatabasePrivilegeChange::new(row1.show_view_priv, row2.show_view_priv),
            trigger_priv: DatabasePrivilegeChange::new(row1.trigger_priv, row2.trigger_priv),
        }
    }

    /// Returns true if there are no changes in this diff.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.select_priv.is_none()
            && self.insert_priv.is_none()
            && self.update_priv.is_none()
            && self.delete_priv.is_none()
            && self.create_priv.is_none()
            && self.drop_priv.is_none()
            && self.alter_priv.is_none()
            && self.index_priv.is_none()
            && self.create_tmp_table_priv.is_none()
            && self.lock_tables_priv.is_none()
            && self.references_priv.is_none()
            && self.create_view_priv.is_none()
            && self.show_view_priv.is_none()
            && self.trigger_priv.is_none()
    }

    /// Retrieves the privilege change for a given privilege name.
    pub fn get_privilege_change_by_name(
        &self,
        privilege_name: &str,
    ) -> anyhow::Result<Option<DatabasePrivilegeChange>> {
        match privilege_name {
            "select_priv" => Ok(self.select_priv),
            "insert_priv" => Ok(self.insert_priv),
            "update_priv" => Ok(self.update_priv),
            "delete_priv" => Ok(self.delete_priv),
            "create_priv" => Ok(self.create_priv),
            "drop_priv" => Ok(self.drop_priv),
            "alter_priv" => Ok(self.alter_priv),
            "index_priv" => Ok(self.index_priv),
            "create_tmp_table_priv" => Ok(self.create_tmp_table_priv),
            "lock_tables_priv" => Ok(self.lock_tables_priv),
            "references_priv" => Ok(self.references_priv),
            "create_view_priv" => Ok(self.create_view_priv),
            "show_view_priv" => Ok(self.show_view_priv),
            "trigger_priv" => Ok(self.trigger_priv),
            _ => anyhow::bail!("Unknown privilege name: {privilege_name}"),
        }
    }

    /// Merges another diff into this one, combining them in a sequential manner.
    fn mappend(&mut self, other: &DatabasePrivilegeRowDiff) {
        debug_assert!(self.db == other.db && self.user == other.user);

        if other.select_priv.is_some() {
            self.select_priv = other.select_priv;
        }
        if other.insert_priv.is_some() {
            self.insert_priv = other.insert_priv;
        }
        if other.update_priv.is_some() {
            self.update_priv = other.update_priv;
        }
        if other.delete_priv.is_some() {
            self.delete_priv = other.delete_priv;
        }
        if other.create_priv.is_some() {
            self.create_priv = other.create_priv;
        }
        if other.drop_priv.is_some() {
            self.drop_priv = other.drop_priv;
        }
        if other.alter_priv.is_some() {
            self.alter_priv = other.alter_priv;
        }
        if other.index_priv.is_some() {
            self.index_priv = other.index_priv;
        }
        if other.create_tmp_table_priv.is_some() {
            self.create_tmp_table_priv = other.create_tmp_table_priv;
        }
        if other.lock_tables_priv.is_some() {
            self.lock_tables_priv = other.lock_tables_priv;
        }
        if other.references_priv.is_some() {
            self.references_priv = other.references_priv;
        }
        if other.create_view_priv.is_some() {
            self.create_view_priv = other.create_view_priv;
        }
        if other.show_view_priv.is_some() {
            self.show_view_priv = other.show_view_priv;
        }
        if other.trigger_priv.is_some() {
            self.trigger_priv = other.trigger_priv;
        }
    }

    /// Removes any no-op changes from the diff, based on the original privilege row.
    fn remove_noops(&mut self, from: &DatabasePrivilegeRow) {
        fn new_value(
            change: Option<&DatabasePrivilegeChange>,
            from_value: bool,
        ) -> Option<DatabasePrivilegeChange> {
            change.as_ref().and_then(|c| match c {
                DatabasePrivilegeChange::YesToNo if from_value => {
                    Some(DatabasePrivilegeChange::YesToNo)
                }
                DatabasePrivilegeChange::NoToYes if !from_value => {
                    Some(DatabasePrivilegeChange::NoToYes)
                }
                _ => None,
            })
        }

        self.select_priv = new_value(self.select_priv.as_ref(), from.select_priv);
        self.insert_priv = new_value(self.insert_priv.as_ref(), from.insert_priv);
        self.update_priv = new_value(self.update_priv.as_ref(), from.update_priv);
        self.delete_priv = new_value(self.delete_priv.as_ref(), from.delete_priv);
        self.create_priv = new_value(self.create_priv.as_ref(), from.create_priv);
        self.drop_priv = new_value(self.drop_priv.as_ref(), from.drop_priv);
        self.alter_priv = new_value(self.alter_priv.as_ref(), from.alter_priv);
        self.index_priv = new_value(self.index_priv.as_ref(), from.index_priv);
        self.create_tmp_table_priv = new_value(
            self.create_tmp_table_priv.as_ref(),
            from.create_tmp_table_priv,
        );
        self.lock_tables_priv = new_value(self.lock_tables_priv.as_ref(), from.lock_tables_priv);
        self.references_priv = new_value(self.references_priv.as_ref(), from.references_priv);
        self.create_view_priv = new_value(self.create_view_priv.as_ref(), from.create_view_priv);
        self.show_view_priv = new_value(self.show_view_priv.as_ref(), from.show_view_priv);
        self.trigger_priv = new_value(self.trigger_priv.as_ref(), from.trigger_priv);
    }

    fn apply(&self, base: &mut DatabasePrivilegeRow) {
        fn apply_change(change: Option<&DatabasePrivilegeChange>, target: &mut bool) {
            match change {
                Some(DatabasePrivilegeChange::YesToNo) => *target = false,
                Some(DatabasePrivilegeChange::NoToYes) => *target = true,
                None => {}
            }
        }

        apply_change(self.select_priv.as_ref(), &mut base.select_priv);
        apply_change(self.insert_priv.as_ref(), &mut base.insert_priv);
        apply_change(self.update_priv.as_ref(), &mut base.update_priv);
        apply_change(self.delete_priv.as_ref(), &mut base.delete_priv);
        apply_change(self.create_priv.as_ref(), &mut base.create_priv);
        apply_change(self.drop_priv.as_ref(), &mut base.drop_priv);
        apply_change(self.alter_priv.as_ref(), &mut base.alter_priv);
        apply_change(self.index_priv.as_ref(), &mut base.index_priv);
        apply_change(
            self.create_tmp_table_priv.as_ref(),
            &mut base.create_tmp_table_priv,
        );
        apply_change(self.lock_tables_priv.as_ref(), &mut base.lock_tables_priv);
        apply_change(self.references_priv.as_ref(), &mut base.references_priv);
        apply_change(self.create_view_priv.as_ref(), &mut base.create_view_priv);
        apply_change(self.show_view_priv.as_ref(), &mut base.show_view_priv);
        apply_change(self.trigger_priv.as_ref(), &mut base.trigger_priv);
    }
}

impl fmt::Display for DatabasePrivilegeRowDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn format_change(
            f: &mut fmt::Formatter<'_>,
            change: Option<DatabasePrivilegeChange>,
            field_name: &str,
        ) -> fmt::Result {
            if let Some(change) = change {
                match change {
                    DatabasePrivilegeChange::YesToNo => f.write_fmt(format_args!(
                        "{}: Y -> N\n",
                        db_priv_field_human_readable_name(field_name)
                    )),
                    DatabasePrivilegeChange::NoToYes => f.write_fmt(format_args!(
                        "{}: N -> Y\n",
                        db_priv_field_human_readable_name(field_name)
                    )),
                }
            } else {
                Ok(())
            }
        }

        format_change(f, self.select_priv, "select_priv")?;
        format_change(f, self.insert_priv, "insert_priv")?;
        format_change(f, self.update_priv, "update_priv")?;
        format_change(f, self.delete_priv, "delete_priv")?;
        format_change(f, self.create_priv, "create_priv")?;
        format_change(f, self.drop_priv, "drop_priv")?;
        format_change(f, self.alter_priv, "alter_priv")?;
        format_change(f, self.index_priv, "index_priv")?;
        format_change(f, self.create_tmp_table_priv, "create_tmp_table_priv")?;
        format_change(f, self.lock_tables_priv, "lock_tables_priv")?;
        format_change(f, self.references_priv, "references_priv")?;
        format_change(f, self.create_view_priv, "create_view_priv")?;
        format_change(f, self.show_view_priv, "show_view_priv")?;
        format_change(f, self.trigger_priv, "trigger_priv")?;

        Ok(())
    }
}

/// This enum encapsulates whether a [`DatabasePrivilegeRow`] was introduced, modified or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub enum DatabasePrivilegesDiff {
    New(DatabasePrivilegeRow),
    Modified(DatabasePrivilegeRowDiff),
    Deleted(DatabasePrivilegeRow),
    Noop { db: MySQLDatabase, user: MySQLUser },
}

impl DatabasePrivilegesDiff {
    #[must_use]
    pub fn get_database_name(&self) -> &MySQLDatabase {
        match self {
            DatabasePrivilegesDiff::New(p) => &p.db,
            DatabasePrivilegesDiff::Modified(p) => &p.db,
            DatabasePrivilegesDiff::Deleted(p) => &p.db,
            DatabasePrivilegesDiff::Noop { db, .. } => db,
        }
    }

    #[must_use]
    pub fn get_user_name(&self) -> &MySQLUser {
        match self {
            DatabasePrivilegesDiff::New(p) => &p.user,
            DatabasePrivilegesDiff::Modified(p) => &p.user,
            DatabasePrivilegesDiff::Deleted(p) => &p.user,
            DatabasePrivilegesDiff::Noop { user, .. } => user,
        }
    }

    /// Merges another [`DatabasePrivilegesDiff`] into this one, combining them in a sequential manner.
    /// For example, if this diff represents a creation and the other represents a modification,
    /// the result will be a creation with the modifications applied.
    pub fn mappend(&mut self, other: &DatabasePrivilegesDiff) -> anyhow::Result<()> {
        debug_assert!(
            self.get_database_name() == other.get_database_name()
                && self.get_user_name() == other.get_user_name()
        );

        if matches!(self, DatabasePrivilegesDiff::Deleted(_))
            && (matches!(other, DatabasePrivilegesDiff::Modified(_)))
        {
            anyhow::bail!("Cannot modify a deleted database privilege row");
        }

        if matches!(self, DatabasePrivilegesDiff::New(_))
            && (matches!(other, DatabasePrivilegesDiff::New(_)))
        {
            anyhow::bail!("Cannot create an already existing database privilege row");
        }

        if matches!(self, DatabasePrivilegesDiff::Modified(_))
            && (matches!(other, DatabasePrivilegesDiff::New(_)))
        {
            anyhow::bail!("Cannot create an already existing database privilege row");
        }

        if matches!(self, DatabasePrivilegesDiff::Noop { .. }) {
            other.clone_into(self);
            return Ok(());
        } else if matches!(other, DatabasePrivilegesDiff::Noop { .. }) {
            return Ok(());
        }

        match (&self, other) {
            (DatabasePrivilegesDiff::New(_), DatabasePrivilegesDiff::Modified(modified)) => {
                let inner_row = match self {
                    DatabasePrivilegesDiff::New(r) => r,
                    _ => unreachable!(),
                };
                modified.apply(inner_row);
            }
            (DatabasePrivilegesDiff::Modified(_), DatabasePrivilegesDiff::Modified(modified)) => {
                let inner_diff = match self {
                    DatabasePrivilegesDiff::Modified(r) => r,
                    _ => unreachable!(),
                };
                inner_diff.mappend(modified);

                if inner_diff.is_empty() {
                    let db = inner_diff.db.clone();
                    let user = inner_diff.user.clone();
                    *self = DatabasePrivilegesDiff::Noop { db, user };
                }
            }
            (DatabasePrivilegesDiff::Modified(_), DatabasePrivilegesDiff::Deleted(deleted)) => {
                *self = DatabasePrivilegesDiff::Deleted(deleted.to_owned());
            }
            (DatabasePrivilegesDiff::New(_), DatabasePrivilegesDiff::Deleted(_)) => {
                let db = self.get_database_name().to_owned();
                let user = self.get_user_name().to_owned();
                *self = DatabasePrivilegesDiff::Noop { db, user };
            }
            _ => {}
        }

        Ok(())
    }
}

pub type DatabasePrivilegeState<'a> = &'a [DatabasePrivilegeRow];

/// This function calculates the differences between two sets of database privileges.
/// It returns a set of [`DatabasePrivilegesDiff`] that can be used to display or
/// apply a set of privilege modifications to the database.
#[must_use]
pub fn diff_privileges(
    from: DatabasePrivilegeState<'_>,
    to: &[DatabasePrivilegeRow],
) -> BTreeSet<DatabasePrivilegesDiff> {
    let from_lookup_table: HashMap<(MySQLDatabase, MySQLUser), DatabasePrivilegeRow> = from
        .iter()
        .cloned()
        .map(|p| ((p.db.clone(), p.user.clone()), p))
        .collect();

    let to_lookup_table: HashMap<(MySQLDatabase, MySQLUser), DatabasePrivilegeRow> = to
        .iter()
        .cloned()
        .map(|p| ((p.db.clone(), p.user.clone()), p))
        .collect();

    let mut result = BTreeSet::new();

    for p in to {
        if let Some(old_p) = from_lookup_table.get(&(p.db.clone(), p.user.clone())) {
            let diff = DatabasePrivilegeRowDiff::from_rows(old_p, p);
            if !diff.is_empty() {
                result.insert(DatabasePrivilegesDiff::Modified(diff));
            }
        } else {
            result.insert(DatabasePrivilegesDiff::New(p.to_owned()));
        }
    }

    for p in from {
        if !to_lookup_table.contains_key(&(p.db.clone(), p.user.clone())) {
            result.insert(DatabasePrivilegesDiff::Deleted(p.to_owned()));
        }
    }

    result
}

/// Converts a set of [`DatabasePrivilegeRowDiff`] into a set of [`DatabasePrivilegesDiff`],
/// representing either creating new privilege rows, or modifying the existing ones.
///
/// This is particularly useful for processing CLI arguments.
pub fn create_or_modify_privilege_rows(
    from: DatabasePrivilegeState<'_>,
    to: &BTreeSet<DatabasePrivilegeRowDiff>,
) -> anyhow::Result<BTreeSet<DatabasePrivilegesDiff>> {
    let from_lookup_table: HashMap<(MySQLDatabase, MySQLUser), DatabasePrivilegeRow> = from
        .iter()
        .cloned()
        .map(|p| ((p.db.clone(), p.user.clone()), p))
        .collect();

    let mut result = BTreeSet::new();

    for diff in to {
        if let Some(old_p) = from_lookup_table.get(&(diff.db.clone(), diff.user.clone())) {
            let mut modified_diff = diff.to_owned();
            modified_diff.remove_noops(old_p);
            if !modified_diff.is_empty() {
                result.insert(DatabasePrivilegesDiff::Modified(modified_diff));
            }
        } else {
            let mut new_row = DatabasePrivilegeRow {
                db: diff.db.clone(),
                user: diff.user.clone(),
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
            };
            diff.apply(&mut new_row);
            result.insert(DatabasePrivilegesDiff::New(new_row));
        }
    }

    Ok(result)
}

/// Reduces a set of [`DatabasePrivilegesDiff`] by removing any modifications that would be no-ops.
/// For example, if a privilege is changed from Yes to No, but it was already No, that change
/// is removed from the diff.
///
/// The `from` parameter is used to determine the current state of the privileges.
/// The `to` parameter is the set of diffs to be reduced.
pub fn reduce_privilege_diffs(
    from: DatabasePrivilegeState<'_>,
    to: BTreeSet<DatabasePrivilegesDiff>,
) -> anyhow::Result<BTreeSet<DatabasePrivilegesDiff>> {
    let from_lookup_table: HashMap<(MySQLDatabase, MySQLUser), DatabasePrivilegeRow> = from
        .iter()
        .cloned()
        .map(|p| ((p.db.clone(), p.user.clone()), p))
        .collect();

    let mut result: HashMap<(MySQLDatabase, MySQLUser), DatabasePrivilegesDiff> = from_lookup_table
        .iter()
        .map(|((db, user), _)| {
            (
                (db.to_owned(), user.to_owned()),
                DatabasePrivilegesDiff::Noop {
                    db: db.to_owned(),
                    user: user.to_owned(),
                },
            )
        })
        .collect();

    for diff in to {
        let entry = result.entry((
            diff.get_database_name().to_owned(),
            diff.get_user_name().to_owned(),
        ));
        match entry {
            Entry::Occupied(mut occupied_entry) => {
                let existing_diff = occupied_entry.get_mut();
                existing_diff.mappend(&diff)?;
            }
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(diff.clone());
            }
        }
    }

    for (key, diff) in &mut result {
        if let Some(from_row) = from_lookup_table.get(key)
            && let DatabasePrivilegesDiff::Modified(modified_diff) = diff
        {
            modified_diff.remove_noops(from_row);
            if modified_diff.is_empty() {
                let db = modified_diff.db.clone();
                let user = modified_diff.user.clone();
                *diff = DatabasePrivilegesDiff::Noop { db, user };
            }
        }
    }

    Ok(result
        .into_values()
        .filter(|diff| !matches!(diff, DatabasePrivilegesDiff::Noop { .. }))
        .collect::<BTreeSet<DatabasePrivilegesDiff>>())
}

/// Renders a set of [`DatabasePrivilegesDiff`] into a human-readable formatted table.
#[must_use]
pub fn display_privilege_diffs(diffs: &BTreeSet<DatabasePrivilegesDiff>) -> String {
    let mut table = Table::new();
    table.set_titles(row!["Database", "User", "Privilege diff",]);
    for row in diffs {
        match row {
            DatabasePrivilegesDiff::New(p) => {
                table.add_row(row![
                    p.db,
                    p.user,
                    "(Previously unprivileged)\n".to_string() + &p.to_string()
                ]);
            }
            DatabasePrivilegesDiff::Modified(p) => {
                table.add_row(row![p.db, p.user, p.to_string(),]);
            }
            DatabasePrivilegesDiff::Deleted(p) => {
                table.add_row(row![p.db, p.user, "Removed".to_string()]);
            }
            DatabasePrivilegesDiff::Noop { db, user } => {
                table.add_row(row![db, user, "No changes".to_string()]);
            }
        }
    }

    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_privilege_change_creation() {
        assert_eq!(
            DatabasePrivilegeChange::new(true, false),
            Some(DatabasePrivilegeChange::YesToNo),
        );
        assert_eq!(
            DatabasePrivilegeChange::new(false, true),
            Some(DatabasePrivilegeChange::NoToYes),
        );
        assert_eq!(DatabasePrivilegeChange::new(true, true), None);
        assert_eq!(DatabasePrivilegeChange::new(false, false), None);
    }

    #[test]
    fn test_database_privilege_row_diff_from_rows() {
        let row1 = DatabasePrivilegeRow {
            db: "db".into(),
            user: "user".into(),

            select_priv: true,
            insert_priv: false,
            update_priv: true,
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
        };
        let row2 = DatabasePrivilegeRow {
            db: "db".into(),
            user: "user".into(),

            select_priv: true,
            insert_priv: true,
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
        };

        let diff = DatabasePrivilegeRowDiff::from_rows(&row1, &row2);
        assert_eq!(
            diff,
            DatabasePrivilegeRowDiff {
                db: "db".into(),
                user: "user".into(),
                select_priv: None,
                insert_priv: Some(DatabasePrivilegeChange::NoToYes),
                update_priv: Some(DatabasePrivilegeChange::YesToNo),
                delete_priv: None,
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_database_privilege_row_diff_is_empty() {
        let empty_diff = DatabasePrivilegeRowDiff {
            db: "db".into(),
            user: "user".into(),
            ..Default::default()
        };

        assert!(empty_diff.is_empty());

        let non_empty_diff = DatabasePrivilegeRowDiff {
            db: "db".into(),
            user: "user".into(),
            select_priv: Some(DatabasePrivilegeChange::YesToNo),
            ..Default::default()
        };

        assert!(!non_empty_diff.is_empty());
    }

    // TODO: test in isolation:
    // DatabasePrivilegeRowDiff::mappend
    // DatabasePrivilegeRowDiff::remove_noops
    // DatabasePrivilegeRowDiff::apply
    //
    // DatabasePrivilegesDiff::mappend
    //
    // reduce_privilege_diffs

    #[test]
    fn test_diff_privileges() {
        let row_to_be_modified = DatabasePrivilegeRow {
            db: "db".into(),
            user: "user".into(),
            select_priv: true,
            insert_priv: true,
            update_priv: true,
            delete_priv: true,
            create_priv: true,
            drop_priv: true,
            alter_priv: true,
            index_priv: false,
            create_tmp_table_priv: true,
            lock_tables_priv: true,
            references_priv: false,
            create_view_priv: false,
            show_view_priv: false,
            trigger_priv: false,
        };

        let mut row_to_be_deleted = row_to_be_modified.to_owned();
        "user2".clone_into(&mut row_to_be_deleted.user);

        let from = vec![row_to_be_modified.to_owned(), row_to_be_deleted.to_owned()];

        let mut modified_row = row_to_be_modified.to_owned();
        modified_row.select_priv = false;
        modified_row.insert_priv = false;
        modified_row.index_priv = true;

        let mut new_row = row_to_be_modified.to_owned();
        "user3".clone_into(&mut new_row.user);

        let to = vec![modified_row.to_owned(), new_row.to_owned()];

        let diffs = diff_privileges(&from, &to);

        assert_eq!(
            diffs,
            BTreeSet::from_iter(vec![
                DatabasePrivilegesDiff::Deleted(row_to_be_deleted),
                DatabasePrivilegesDiff::Modified(DatabasePrivilegeRowDiff {
                    db: "db".into(),
                    user: "user".into(),
                    select_priv: Some(DatabasePrivilegeChange::YesToNo),
                    insert_priv: Some(DatabasePrivilegeChange::YesToNo),
                    index_priv: Some(DatabasePrivilegeChange::NoToYes),
                    ..Default::default()
                }),
                DatabasePrivilegesDiff::New(new_row),
            ])
        );
    }
}
