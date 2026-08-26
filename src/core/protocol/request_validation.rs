use std::{collections::HashSet, sync::OnceLock};

use indoc::indoc;
use nix::{libc::gid_t, unistd::Group};
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::{common::UnixUser, types::DbOrUser};

#[derive(Error, Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum NameValidationError {
    #[error("Name cannot be empty.")]
    EmptyString,

    #[error(
        "Name contains invalid characters. Only A-Z, a-z, 0-9, _ (underscore) and - (dash) are permitted."
    )]
    InvalidCharacters,

    #[error("Name is too long. Maximum length is 64 characters.")]
    TooLong,
}

impl NameValidationError {
    #[must_use]
    pub fn to_error_message(self, db_or_user: &DbOrUser) -> String {
        match self {
            NameValidationError::EmptyString => {
                format!("{} name can not be empty.", db_or_user.capitalized_noun())
            }
            NameValidationError::TooLong => format!(
                "{} is too long, maximum length is 64 characters.",
                db_or_user.capitalized_noun()
            ),
            NameValidationError::InvalidCharacters => format!(
                indoc! {r"
                  Invalid characters in {} name: '{}', only A-Z, a-z, 0-9, _ (underscore) and - (dash) are permitted.
                "},
                db_or_user.lowercased_noun(),
                db_or_user.name(),
            ),
        }
    }

    #[must_use]
    pub fn error_type(&self) -> &'static str {
        match self {
            NameValidationError::EmptyString => "empty-string",
            NameValidationError::InvalidCharacters => "invalid-characters",
            NameValidationError::TooLong => "too-long",
        }
    }
}

#[derive(Error, Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum AuthorizationError {
    #[error("Illegal prefix, user is not authorized to manage this resource")]
    IllegalPrefix,

    // TODO: I don't think this should ever happen?
    #[error("Name cannot be empty")]
    StringEmpty,

    #[error("Group was found in denylist")]
    DenylistError,
}

impl AuthorizationError {
    #[must_use]
    pub fn to_error_message(self, db_or_user: &DbOrUser) -> String {
        match self {
            AuthorizationError::IllegalPrefix => format!(
                "Illegal {} name prefix: you are not allowed to manage databases or users prefixed with '{}'",
                db_or_user.lowercased_noun(),
                db_or_user.prefix(),
            )
            .to_owned(),
            // TODO: This error message could be clearer
            AuthorizationError::StringEmpty => {
                format!("{} name can not be empty.", db_or_user.capitalized_noun())
            }
            AuthorizationError::DenylistError => {
                format!("'{}' is denied by the group denylist", db_or_user.name())
            }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> &'static str {
        match self {
            AuthorizationError::IllegalPrefix => "illegal-prefix",
            AuthorizationError::StringEmpty => "string-empty",
            AuthorizationError::DenylistError => "denylist-error",
        }
    }
}

#[derive(Error, Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum ValidationError {
    #[error("Name validation error: {0}")]
    NameValidationError(NameValidationError),

    #[error("Authorization error: {0}")]
    AuthorizationError(AuthorizationError),
    // AuthorizationHandlerError(String),
}

impl ValidationError {
    #[must_use]
    pub fn to_error_message(&self, db_or_user: &DbOrUser) -> String {
        match self {
            ValidationError::NameValidationError(err) => err.to_error_message(db_or_user),
            ValidationError::AuthorizationError(err) => err.to_error_message(db_or_user),
            // AuthorizationError::AuthorizationHandlerError(msg) => {
            //     format!(
            //         "Authorization handler error for '{}': {}",
            //         db_or_user.name(),
            //         msg
            //     )
            // }
        }
    }

    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            ValidationError::NameValidationError(err) => {
                format!("name-validation-error/{}", err.error_type())
            }
            ValidationError::AuthorizationError(err) => {
                format!("authorization-error/{}", err.error_type())
            } // AuthorizationError::AuthorizationHandlerError(_) => {
              //     "authorization-handler-error".to_string()
              // }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupNamePattern(String);

impl GroupNamePattern {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self(pattern.into()).normalize()
    }

    /// Collapses runs of consecutive `*`/`?` wildcards into a canonical form.
    ///
    /// - `***` -> `*`
    /// - `*?*?` -> `??*`
    fn normalize(&self) -> Self {
        let mut result = String::with_capacity(self.0.len());
        let mut chars = self.0.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '*' || c == '?' {
                let mut question_marks = usize::from(c == '?');
                let mut has_star = c == '*';

                while let Some(&next) = chars.peek() {
                    match next {
                        '?' => question_marks += 1,
                        '*' => has_star = true,
                        _ => break,
                    }
                    chars.next();
                }

                result.extend(std::iter::repeat_n('?', question_marks));
                if has_star {
                    result.push('*');
                }
            } else {
                result.push(c);
            }
        }

        Self(result)
    }

    pub fn to_regex(&self) -> Result<Regex, regex::Error> {
        let mut regex_str = String::from("^");
        for c in self.0.chars() {
            match c {
                '*' => regex_str.push_str(".*"),
                '?' => regex_str.push('.'),
                _ => regex_str.push_str(&regex::escape(&c.to_string())),
            }
        }
        regex_str.push('$');
        Regex::new(&regex_str)
    }
}

#[derive(Debug, Default)]
pub struct GroupDenylist {
    gids: HashSet<gid_t>,
    name_patterns: Vec<GroupNamePattern>,
    compiled_name_patterns: OnceLock<Vec<Regex>>,
}

impl Clone for GroupDenylist {
    fn clone(&self) -> Self {
        Self {
            gids: self.gids.clone(),
            name_patterns: self.name_patterns.clone(),
            compiled_name_patterns: OnceLock::new(),
        }
    }
}

impl GroupDenylist {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_gid(&mut self, gid: gid_t) {
        self.gids.insert(gid);
    }

    pub fn insert_name_pattern(&mut self, pattern: GroupNamePattern) {
        self.name_patterns.push(pattern);
        self.compiled_name_patterns = OnceLock::new();
    }

    pub fn is_empty(&self) -> bool {
        self.gids.is_empty() && self.name_patterns.is_empty()
    }

    pub fn len(&self) -> usize {
        self.gids.len() + self.name_patterns.len()
    }

    fn compiled_name_pattern_regexes(&self) -> &[Regex] {
        self.compiled_name_patterns.get_or_init(|| {
            self.name_patterns
                .iter()
                .filter_map(|pattern| pattern.to_regex().ok())
                .collect()
        })
    }

    pub fn matches(&self, group: &Group) -> bool {
        self.gids.contains(&group.gid.as_raw())
            || self
                .compiled_name_pattern_regexes()
                .iter()
                .any(|regex| regex.is_match(&group.name))
    }
}

const MAX_NAME_LENGTH: usize = 64;

pub fn validate_name(name: &str) -> Result<(), NameValidationError> {
    if name.is_empty() {
        Err(NameValidationError::EmptyString)
    } else if name.len() > MAX_NAME_LENGTH {
        Err(NameValidationError::TooLong)
    } else if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Err(NameValidationError::InvalidCharacters)
    } else {
        Ok(())
    }
}

pub fn validate_authorization_by_unix_user(
    name: &str,
    user: &UnixUser,
) -> Result<(), AuthorizationError> {
    let prefixes = std::iter::once(user.username.clone())
        .chain(user.groups.iter().cloned())
        .collect::<Vec<String>>();

    validate_authorization_by_prefixes(name, &prefixes)
}

/// Core logic for validating the ownership of a database name.
/// This function checks if the given name matches any of the given prefixes.
/// These prefixes will in most cases be the user's unix username and any
/// unix groups the user is a member of.
pub fn validate_authorization_by_prefixes(
    name: &str,
    prefixes: &[String],
) -> Result<(), AuthorizationError> {
    if name.is_empty() {
        return Err(AuthorizationError::StringEmpty);
    }

    if prefixes
        .iter()
        .filter(|p| name.starts_with(&((*p).clone() + "_")))
        .collect::<Vec<_>>()
        .is_empty()
    {
        return Err(AuthorizationError::IllegalPrefix);
    }

    Ok(())
}

pub fn validate_authorization_by_group_denylist(
    name: &str,
    user: &UnixUser,
    group_denylist: &GroupDenylist,
) -> Result<(), AuthorizationError> {
    // NOTE: if the username matches, we allow it regardless of denylist
    if user.username == name {
        return Ok(());
    }

    let user_group = Group::from_name(name).ok().flatten();

    if let Some(group) = user_group
        && group_denylist.matches(&group)
    {
        Err(AuthorizationError::DenylistError)
    } else {
        Ok(())
    }
}

pub fn validate_db_or_user_request(
    db_or_user: &DbOrUser,
    unix_user: &UnixUser,
    group_denylist: &GroupDenylist,
) -> Result<(), ValidationError> {
    validate_name(db_or_user.name()).map_err(ValidationError::NameValidationError)?;

    validate_authorization_by_unix_user(db_or_user.name(), unix_user)
        .map_err(ValidationError::AuthorizationError)?;

    validate_authorization_by_group_denylist(db_or_user.name(), unix_user, group_denylist)
        .map_err(ValidationError::AuthorizationError)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name() {
        assert_eq!(validate_name(""), Err(NameValidationError::EmptyString));
        assert_eq!(validate_name("abcdefghijklmnopqrstuvwxyz"), Ok(()));
        assert_eq!(validate_name("ABCDEFGHIJKLMNOPQRSTUVWXYZ"), Ok(()));
        assert_eq!(validate_name("0123456789_-"), Ok(()));

        for c in "\n\t\r !@#$%^&*()+=[]{}|;:,.<>?/".chars() {
            assert_eq!(
                validate_name(&c.to_string()),
                Err(NameValidationError::InvalidCharacters)
            );
        }

        assert_eq!(validate_name(&"a".repeat(MAX_NAME_LENGTH)), Ok(()));

        assert_eq!(
            validate_name(&"a".repeat(MAX_NAME_LENGTH + 1)),
            Err(NameValidationError::TooLong)
        );
    }

    #[test]
    fn test_group_name_pattern_normalize() {
        assert_eq!(GroupNamePattern::new("*").0, "*");
        assert_eq!(GroupNamePattern::new("?").0, "?");
        assert_eq!(GroupNamePattern::new("**").0, "*");
        assert_eq!(GroupNamePattern::new("***").0, "*");
        assert_eq!(GroupNamePattern::new("??").0, "??");
        assert_eq!(GroupNamePattern::new("?*").0, "?*");
        assert_eq!(GroupNamePattern::new("*?").0, "?*");
        assert_eq!(GroupNamePattern::new("*?*?*").0, "??*");
        assert_eq!(GroupNamePattern::new("admin**?*svc").0, "admin?*svc");
        assert_eq!(GroupNamePattern::new("admin").0, "admin");
        assert_eq!(GroupNamePattern::new("").0, "");
    }

    #[test]
    fn test_group_name_pattern_only_wildcards() {
        let star = GroupNamePattern::new("*");
        assert!(star.to_regex().unwrap().is_match(""));
        assert!(star.to_regex().unwrap().is_match("a"));
        assert!(star.to_regex().unwrap().is_match("anything"));

        let question_mark = GroupNamePattern::new("?");
        assert!(!question_mark.to_regex().unwrap().is_match(""));
        assert!(question_mark.to_regex().unwrap().is_match("a"));
        assert!(!question_mark.to_regex().unwrap().is_match("ab"));

        let double_question_mark = GroupNamePattern::new("??");
        assert!(!double_question_mark.to_regex().unwrap().is_match("a"));
        assert!(double_question_mark.to_regex().unwrap().is_match("ab"));
        assert!(!double_question_mark.to_regex().unwrap().is_match("abc"));

        let collapsed_stars = GroupNamePattern::new("***");
        assert!(collapsed_stars.to_regex().unwrap().is_match(""));
        assert!(collapsed_stars.to_regex().unwrap().is_match("anything"));
    }

    #[test]
    fn test_validate_authorization_by_prefixes() {
        let prefixes = vec!["user".to_string(), "group".to_string()];

        assert_eq!(
            validate_authorization_by_prefixes("", &prefixes),
            Err(AuthorizationError::StringEmpty)
        );

        assert_eq!(
            validate_authorization_by_prefixes("user_testdb", &prefixes),
            Ok(())
        );
        assert_eq!(
            validate_authorization_by_prefixes("group_testdb", &prefixes),
            Ok(())
        );
        assert_eq!(
            validate_authorization_by_prefixes("group_test_db", &prefixes),
            Ok(())
        );
        assert_eq!(
            validate_authorization_by_prefixes("group_test-db", &prefixes),
            Ok(())
        );

        assert_eq!(
            validate_authorization_by_prefixes("nonexistent_testdb", &prefixes),
            Err(AuthorizationError::IllegalPrefix)
        );
    }
}
