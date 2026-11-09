// SPDX-License-Identifier: Apache-2.0

//! Layer 3, per crate: the keys a crate uses exist, keys of other crates are
//! public, and its catalogue files parse and are all listed.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use fluent_syntax::ast::Entry;

use super::catalogue::CatalogueCheck;
use super::source::{crate_sources, ftl_files, key_literals};
use crate::{FALLBACK_LOCALE, Resources, public_keys};

/// The per-crate key check:
///
/// ```ignore
/// kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &my_crate::l10n::RESOURCES).check();
/// ```
///
/// - Each key literal passed to `tr!`, `text!` or `tr_or` in the crate's
///   non-test source exists in the en-US resources linked into the test
///   binary: the crate's own and its dependencies'.
/// - A key of another crate is in that crate's `## public` group.
/// - Every `.ftl` of the crate parses, and every one under `l10n/` is listed
///   in its `resources!`.
pub fn keys(manifest_dir: impl AsRef<Path>, resources: &'static Resources) -> Keys {
    Keys {
        dir: manifest_dir.as_ref().to_path_buf(),
        resources,
        linked: None,
    }
}

/// A configured key check; see [`keys`].
pub struct Keys {
    dir: PathBuf,
    resources: &'static Resources,
    linked: Option<Vec<&'static Resources>>,
}

/// What the key check found.
#[derive(Clone, Debug, Default)]
pub struct KeysReport {
    pub errors: Vec<String>,
}

impl fmt::Display for KeysReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for error in &self.errors {
            writeln!(f, "error: {error}")?;
        }
        Ok(())
    }
}

impl Keys {
    /// Checks against `linked` instead of every crate's resources linked into
    /// the binary.
    pub fn linked(mut self, linked: Vec<&'static Resources>) -> Self {
        self.linked = Some(linked);
        self
    }

    pub fn report(&self) -> KeysReport {
        let mut errors = Vec::new();
        let mut linked = self.linked.clone().unwrap_or_else(Resources::registered);
        if !linked.iter().any(|r| std::ptr::eq(*r, self.resources)) {
            linked.push(self.resources);
        }

        // Who owns each en-US message, and whether it is public.
        let mut owners: HashMap<String, (&'static str, bool)> = HashMap::new();
        for resources in &linked {
            for file in resources
                .files
                .iter()
                .filter(|f| f.locale == FALLBACK_LOCALE)
            {
                let public: BTreeSet<String> = public_keys(file.source).into_iter().collect();
                let resource = match fluent_syntax::parser::parse(file.source) {
                    Ok(resource) | Err((resource, _)) => resource,
                };
                for entry in &resource.body {
                    if let Entry::Message(message) = entry {
                        let id = message.id.name.to_string();
                        let is_public = public.contains(&id);
                        owners
                            .entry(id)
                            .or_insert((resources.crate_name, is_public));
                    }
                }
            }
        }

        for file in self.resources.files {
            if let Err((_, parse_errors)) = fluent_syntax::parser::parse(file.source) {
                for error in parse_errors {
                    errors.push(format!(
                        "{}: {}: doesn't parse: {error:?}",
                        file.locale, file.path
                    ));
                }
            }
        }

        let listed: BTreeSet<PathBuf> = self.listed_paths(&mut errors);
        for found in ftl_files(&self.dir.join("l10n")) {
            let canonical = found.canonicalize().unwrap_or(found.clone());
            if !listed.contains(&canonical) {
                let rel =
                    CatalogueCheck::slash_path(found.strip_prefix(&self.dir).unwrap_or(&found));
                errors.push(format!("{rel} isn't listed in the crate's resources!"));
            }
        }

        let (sources, mut source_errors) = crate_sources(&self.dir);
        errors.append(&mut source_errors);
        for source in &sources {
            for key in key_literals(&source.syntax) {
                let at = format!("{}:{}", source.rel, key.line);
                match owners.get(&key.value) {
                    None => errors.push(format!(
                        "{at}: `{}` isn't in any en-US resource linked into this test",
                        key.value
                    )),
                    Some((owner, public)) if *owner != self.resources.crate_name && !public => {
                        errors.push(format!(
                            "{at}: `{}` belongs to {owner}, and isn't in its `## public` group",
                            key.value
                        ))
                    }
                    Some(_) => {}
                }
            }
        }
        KeysReport { errors }
    }

    /// The files `resources!` lists, canonical. Their paths are relative to
    /// the calling file, which `file!()` names relative to the workspace root.
    fn listed_paths(&self, errors: &mut Vec<String>) -> BTreeSet<PathBuf> {
        let declared = Path::new(self.resources.declared_in);
        let calling_file = if declared.is_absolute() {
            Some(declared.to_path_buf())
        } else {
            self.dir
                .ancestors()
                .map(|a| a.join(declared))
                .find(|p| p.is_file())
        };
        let Some(calling_file) = calling_file else {
            errors.push(format!(
                "can't find {}, which calls resources!",
                self.resources.declared_in
            ));
            return BTreeSet::new();
        };
        let base = calling_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        self.resources
            .files
            .iter()
            .filter_map(|file| {
                let path = base.join(file.path);
                match path.canonicalize() {
                    Ok(path) => Some(path),
                    Err(error) => {
                        errors.push(format!("{}: {error}", path.display()));
                        None
                    }
                }
            })
            .collect()
    }

    #[track_caller]
    pub fn check(&self) {
        let report = self.report();
        assert!(report.errors.is_empty(), "{report}");
    }
}
