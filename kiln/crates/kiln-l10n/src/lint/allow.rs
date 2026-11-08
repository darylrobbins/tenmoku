// SPDX-License-Identifier: Apache-2.0

//! `l10n-allow.toml`: which files the literal lint checks, and its
//! exceptions.
//!
//! ```toml
//! # While a crate migrates, only the files listed here are checked. A card
//! # that migrates files adds their lines; the crate's last card replaces the
//! # list with `all = true`.
//! migrated = [
//!     "src/picker.rs",
//! ]
//!
//! # Whole files that are never checked.
//! [[file]]
//! path = "src/language.rs"
//! reason = "Language display names, which stay in their own language"
//!
//! # One literal in one file.
//! [[literal]]
//! file = "src/paths.rs"
//! text = "Application Support"
//! reason = "A macOS path component"
//! ```

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    all: Option<bool>,
    migrated: Option<Vec<String>>,
    #[serde(default)]
    file: Vec<RawFile>,
    #[serde(default)]
    literal: Vec<RawLiteral>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    path: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLiteral {
    file: String,
    text: String,
    reason: Option<String>,
}

/// Which files the lint checks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Scope {
    All,
    Migrated(BTreeSet<String>),
}

/// A parsed, validated allow-list.
#[derive(Clone, Debug)]
pub(crate) struct AllowList {
    pub(crate) scope: Scope,
    pub(crate) files: BTreeSet<String>,
    pub(crate) literals: Vec<(String, String)>,
}

impl AllowList {
    /// Every file checked, as `all = true` without exceptions.
    pub(crate) fn everything() -> Self {
        Self {
            scope: Scope::All,
            files: BTreeSet::new(),
            literals: Vec::new(),
        }
    }

    /// Reads `path` (named `name` in errors), checking every entry against
    /// `sources`, the crate's non-test files.
    pub(crate) fn read(
        path: &Path,
        name: &str,
        sources: &BTreeSet<String>,
    ) -> Result<Self, Vec<String>> {
        let text =
            std::fs::read_to_string(path).map_err(|error| vec![format!("{name}: {error}")])?;
        Self::parse(&text, name, sources)
    }

    pub(crate) fn parse(
        text: &str,
        name: &str,
        sources: &BTreeSet<String>,
    ) -> Result<Self, Vec<String>> {
        let raw: Raw = toml::from_str(text).map_err(|error| vec![format!("{name}: {error}")])?;
        let mut errors = Vec::new();
        let scope = match (raw.all, raw.migrated) {
            (Some(true), None) => Scope::All,
            (None, Some(files)) => {
                let mut migrated = BTreeSet::new();
                for file in files {
                    if !sources.contains(&file) {
                        errors.push(format!(
                            "{name}: migrated file `{file}` isn't non-test source of this crate"
                        ));
                    }
                    if !migrated.insert(file.clone()) {
                        errors.push(format!("{name}: migrated file `{file}` is listed twice"));
                    }
                }
                Scope::Migrated(migrated)
            }
            (Some(_), Some(_)) => {
                errors.push(format!("{name}: has both `all` and `migrated`; a crate's last card replaces the list with `all = true`"));
                Scope::All
            }
            (Some(false), None) => {
                errors.push(format!(
                    "{name}: `all = false`; list the migrated files in `migrated` instead"
                ));
                Scope::All
            }
            (None, None) => {
                errors.push(format!(
                    "{name}: sets neither `all = true` nor `migrated = [...]`"
                ));
                Scope::All
            }
        };
        let has_reason =
            |reason: &Option<String>| reason.as_deref().is_some_and(|r| !r.trim().is_empty());
        let mut files = BTreeSet::new();
        for entry in raw.file {
            if !has_reason(&entry.reason) {
                errors.push(format!("{name}: [[file]] `{}` has no reason", entry.path));
            }
            if !sources.contains(&entry.path) {
                errors.push(format!(
                    "{name}: [[file]] `{}` isn't non-test source of this crate",
                    entry.path
                ));
            }
            files.insert(entry.path);
        }
        let mut literals = Vec::new();
        for entry in raw.literal {
            if !has_reason(&entry.reason) {
                errors.push(format!(
                    "{name}: [[literal]] `{}` in `{}` has no reason",
                    entry.text, entry.file
                ));
            }
            if !sources.contains(&entry.file) {
                errors.push(format!(
                    "{name}: [[literal]] file `{}` isn't non-test source of this crate",
                    entry.file
                ));
            }
            literals.push((entry.file, entry.text));
        }
        if errors.is_empty() {
            Ok(Self {
                scope,
                files,
                literals,
            })
        } else {
            Err(errors)
        }
    }

    /// Whether the lint checks `file`.
    pub(crate) fn checks(&self, file: &str) -> bool {
        let in_scope = match &self.scope {
            Scope::All => true,
            Scope::Migrated(files) => files.contains(file),
        };
        in_scope && !self.files.contains(file)
    }
}
