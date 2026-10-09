//! The selected colour theme, kept across reloads of the themes directory,
//! as [`crate::SettingsStore`] keeps the settings.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    diagnostic::{Diagnostic, Severity},
    theme::{Theme, ThemeSet, load_themes},
};

/// What a reload or a selection changed.
#[derive(Clone, Debug)]
pub struct ThemeEvent {
    /// The theme in use now.
    pub theme: Arc<Theme>,
    /// The theme in use before.
    pub previous: Arc<Theme>,
    /// The diagnostics now, as [`ThemeStore::diagnostics`] has them.
    pub diagnostics: Vec<Diagnostic>,
}

/// The theme selected by name from the bundled themes and a themes
/// directory (see [`crate::paths::themes_dir`]).
///
/// A name that isn't listed when it's selected gives the bundled fastedit
/// Dark ([`Theme::fastedit_dark`]), with a warning. Once a theme has loaded, a reload that leaves its name unlisted
/// keeps it, with a warning beside the read's own diagnostics: that's a
/// syntax error in a half-saved file, a renamed theme or a deleted file, and
/// none of them should flash fastedit Dark.
pub struct ThemeStore {
    dir: Option<PathBuf>,
    /// The selected name, as given.
    name: String,
    theme: Arc<Theme>,
    /// Whether `theme` is fastedit Dark standing in for an unlisted `name`.
    fallback: bool,
    /// The last read's diagnostics, then any selection warning.
    diagnostics: Vec<Diagnostic>,
    /// The `*.toml` files the last read found, sorted.
    paths: Vec<PathBuf>,
}

impl ThemeStore {
    /// Reads the themes in `dir`, which may be absent or missing, and
    /// selects the one called `name`, ignoring case.
    pub fn load(dir: Option<PathBuf>, name: &str) -> Self {
        let mut store = Self {
            dir,
            name: name.to_owned(),
            theme: Arc::new(Theme::fastedit_dark().clone()),
            fallback: true,
            diagnostics: Vec::new(),
            paths: Vec::new(),
        };
        store.read(false);
        store
    }

    /// The theme in use.
    pub fn theme(&self) -> &Arc<Theme> {
        &self.theme
    }

    /// The last read's diagnostics, then any warning about the selected
    /// name.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Every `*.toml` file the last read found, sorted, for a
    /// [`crate::ConfigWatcher`]. A file added since is found by the next
    /// read, which [`ThemeStore::select`] always does. While a reload keeps
    /// the last good theme, files that have gone since are kept too, so the
    /// selected theme's file is read again when it comes back.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }

    /// Re-reads the directory if any of `changed` is one of
    /// [`ThemeStore::paths`]. Returns what changed if the theme or the
    /// diagnostics did, and `None` if neither did or nothing was read.
    pub fn reload(&mut self, changed: &[PathBuf]) -> Option<ThemeEvent> {
        if !changed.iter().any(|path| self.paths.contains(path)) {
            return None;
        }
        self.read(true)
    }

    /// Re-reads the directory, so a theme added since the last read can be
    /// chosen, and selects the one called `name`, ignoring case. Returns
    /// what changed, or `None` if nothing did. Selecting the selected name
    /// again behaves as a reload: a theme that loaded before is kept.
    pub fn select(&mut self, name: &str) -> Option<ThemeEvent> {
        let same = name.to_lowercase() == self.name.to_lowercase();
        self.name = name.to_owned();
        self.read(same)
    }

    /// Reads the directory and selects `self.name` from it. If the name
    /// isn't listed, `keep` keeps a theme that loaded under it before.
    fn read(&mut self, keep: bool) -> Option<ThemeEvent> {
        let (set, mut diagnostics) = load_themes(self.dir.as_deref());
        // Listed after the read, so a file that appears meanwhile is watched
        // and read again when it next changes.
        let mut paths = theme_files(self.dir.as_deref());
        let (theme, fallback) = self.choose(&set, keep, &mut diagnostics);
        if keep && !fallback && set.get(&self.name).is_none() {
            // The kept theme's file may be gone only for a moment, as when an
            // editor saves by moving the old file away and writing a new
            // one, so files gone since the last read stay watched.
            paths.append(&mut self.paths);
            paths.sort();
            paths.dedup();
        }
        self.paths = paths;
        self.fallback = fallback;
        if *theme == *self.theme && diagnostics == self.diagnostics {
            return None;
        }
        let previous = std::mem::replace(&mut self.theme, theme);
        self.diagnostics = diagnostics;
        Some(ThemeEvent {
            theme: self.theme.clone(),
            previous,
            diagnostics: self.diagnostics.clone(),
        })
    }

    /// The theme for `self.name` from `set`, and whether it's the fallback,
    /// adding a warning to `diagnostics` if the name isn't listed.
    fn choose(
        &self,
        set: &ThemeSet,
        keep: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> (Arc<Theme>, bool) {
        if let Some(theme) = set.get(&self.name) {
            let theme = if *theme == *self.theme {
                self.theme.clone()
            } else {
                Arc::new(theme.clone())
            };
            return (theme, false);
        }
        if keep && !self.fallback {
            let message = format!(
                "no theme is named {:?} now, so the one last loaded stays in use",
                self.name
            );
            diagnostics.push(self.warning(message));
            return (self.theme.clone(), false);
        }
        let message = format!("no theme is named {:?}; using fastedit Dark", self.name);
        diagnostics.push(self.warning(message));
        let dark = Theme::fastedit_dark();
        let theme = if *self.theme == *dark {
            self.theme.clone()
        } else {
            Arc::new(dark.clone())
        };
        (theme, true)
    }

    /// A warning about the selected name, at the themes directory.
    fn warning(&self, message: String) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            file: self.dir.clone().unwrap_or_default(),
            key: None,
            line: 1,
            column: 1,
            message,
        }
    }
}

/// The `*.toml` files in `dir`, as [`load_themes`] picks them, sorted.
fn theme_files(dir: Option<&Path>) -> Vec<PathBuf> {
    let Some(entries) = dir.and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml") && path.is_file())
        .collect();
    files.sort();
    files
}
