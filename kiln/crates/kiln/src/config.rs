// SPDX-License-Identifier: LicenseRef-Proprietary

//! The dark and light pair as layers, and the user's theme file, as a
//! source for kiln-platform's config service: the facade's, as only it
//! sees both the service, the extensions and kiln-ui's themes.
//!
//! Each slot of the pair is, lowest first: the bundled base, the
//! installed theme chosen for it ([`crate::extensions::select_themes`]),
//! then the user's `theme.toml`, if its appearance is the slot's.

use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, rc::Rc};

use gpui::{App, Global};
use kiln_core::{
    Diagnostic, Severity,
    theme::{Appearance, ThemeData, ThemeFile},
};
use kiln_platform::config::{ConfigDiagnostics, ConfigSource, log_diagnostics_as, log_target};
use kiln_ui::{GlobalTheme, Theme, ThemeMode, set_themes};

/// A theme an extension or the app installed, laid over the bundled base
/// for its appearance.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Installed {
    pub(crate) title: String,
    pub(crate) appearance: Appearance,
    pub(crate) data: ThemeData,
}

/// What the pair is made of: [`ThemeLayers::apply`] lays it into the
/// pair in use.
#[derive(Default)]
pub(crate) struct ThemeLayers {
    /// The extensions' themes, by ID (`<extension id>/<file stem>`).
    pub(crate) installed: BTreeMap<String, Installed>,
    /// The app's own themes, by ID (`builtin/<id>`): kept across rescans
    /// ([`crate::extensions::install_builtin_themes`]).
    pub(crate) builtin: BTreeMap<String, Installed>,
    /// Where they're installed, which a selection's warnings name.
    pub(crate) root: Option<PathBuf>,
    /// The installed theme's ID each slot starts from, dark then light.
    selected: [Option<String>; 2],
    /// The user's theme file, as last read without errors.
    user: Option<ThemeFile>,
    /// What the app found wrong with its choice, as it chose
    /// ([`crate::extensions::select_themes_noting`]).
    notes: Vec<Diagnostic>,
    /// What's wrong with the selection, the app's notes first:
    /// [`SelectionSource`]'s.
    diagnostics: Vec<Diagnostic>,
    /// The theme each slot held before the layers' went in, to put back
    /// once they have none for it.
    replaced: [Option<Theme>; 2],
}

/// The layers, shared by the config sources that feed them, as a GPUI
/// global.
#[derive(Clone, Default)]
pub(crate) struct Layers(Rc<RefCell<ThemeLayers>>);

impl Global for Layers {}

impl Layers {
    /// The app's layers: those [`crate::init`] set up, else new ones.
    pub(crate) fn get(cx: &mut App) -> Self {
        cx.default_global::<Layers>().clone()
    }

    pub(crate) fn borrow(&self) -> std::cell::Ref<'_, ThemeLayers> {
        self.0.borrow()
    }

    pub(crate) fn borrow_mut(&self) -> std::cell::RefMut<'_, ThemeLayers> {
        self.0.borrow_mut()
    }

    /// Chooses the installed theme each slot starts from, then applies the
    /// layers, publishing what's wrong with the choice, after the app's
    /// `notes` on it, in [`ConfigDiagnostics`].
    pub(crate) fn select(
        &self,
        dark: Option<&str>,
        light: Option<&str>,
        notes: Vec<Diagnostic>,
        cx: &mut App,
    ) {
        {
            let mut layers = self.borrow_mut();
            layers.selected = [dark.map(str::to_owned), light.map(str::to_owned)];
            layers.notes = notes;
        }
        self.apply_publishing(cx);
    }

    /// [`Layers::apply`], publishing what's wrong with the selection in
    /// [`ConfigDiagnostics`] in place of what was: for a change between
    /// the config service's batches.
    pub(crate) fn apply_publishing(&self, cx: &mut App) {
        let previous = self.borrow().diagnostics.clone();
        self.apply(cx);
        let diagnostics = self.borrow().diagnostics.clone();
        if diagnostics != previous {
            // The service publishes them with each batch; a choice comes
            // between batches, so it puts its own in place of the last.
            let mut all = cx
                .try_global::<ConfigDiagnostics>()
                .map(|all| all.0.clone())
                .unwrap_or_default();
            all.retain(|diagnostic| !previous.contains(diagnostic));
            all.extend(diagnostics);
            cx.set_global(ConfigDiagnostics(all));
        }
    }

    /// Lays each slot's layers into the pair in use, in the mode in use,
    /// after putting back what an earlier apply replaced; a slot with no
    /// layer over the base keeps what it had. Logs the selection's
    /// problems when they change. Sets the pair only if it changed: with
    /// nothing laid in, a theme set alone stays.
    pub(crate) fn apply(&self, cx: &mut App) {
        let global = cx.try_global::<GlobalTheme>();
        let mode = global
            .and_then(GlobalTheme::mode)
            .unwrap_or(ThemeMode::System);
        let current = global
            .and_then(GlobalTheme::themes)
            .map(|(dark, light)| (dark.clone(), light.clone()));
        let (mut dark, mut light) = current
            .clone()
            .unwrap_or_else(|| (Theme::dark(), Theme::light()));
        let mut layers = self.borrow_mut();
        let notes = layers.notes.clone();
        let previous = std::mem::replace(&mut layers.diagnostics, notes);
        // Whether a slot is, or was, the layers': else the pair, or a
        // theme set alone, stays as it is.
        let mut touched = false;
        for (ix, appearance) in [Appearance::Dark, Appearance::Light]
            .into_iter()
            .enumerate()
        {
            let slot = slot(appearance, &mut dark, &mut light);
            if let Some(replaced) = layers.replaced[ix].take() {
                *slot = replaced;
                touched = true;
            }
            if let Some(theme) = layers.theme_for(ix, appearance) {
                layers.replaced[ix] = Some(std::mem::replace(slot, theme));
                touched = true;
            }
        }
        let changed = layers.diagnostics != previous;
        let diagnostics = layers.diagnostics.clone();
        drop(layers);
        if changed {
            log_diagnostics_as(log_target(cx), &diagnostics);
        }
        if touched && current.as_ref() != Some(&(dark.clone(), light.clone())) {
            set_themes(cx, mode, dark, light);
        }
    }
}

impl ThemeLayers {
    /// Slot `ix`'s theme, if anything is laid over its bundled base,
    /// noting what's wrong with its selection.
    fn theme_for(&mut self, ix: usize, appearance: Appearance) -> Option<Theme> {
        let selected = self.selected[ix].as_deref().and_then(|id| {
            let found = self.installed.get(id).or_else(|| self.builtin.get(id));
            let message = match found {
                Some(theme) if theme.appearance == appearance => return Some(&theme.data),
                Some(theme) => format!(
                    "theme {id} is {}, so it can't be the {} theme",
                    name(theme.appearance),
                    name(appearance)
                ),
                None => format!("theme {id} isn't installed"),
            };
            self.diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                file: self.root.clone().unwrap_or_default(),
                key: None,
                line: 1,
                column: 1,
                message,
            });
            None
        });
        let user = self
            .user
            .as_ref()
            .filter(|file| file.appearance.unwrap_or_default() == appearance);
        let data = match (user, selected) {
            (None, None) => return None,
            (None, Some(selected)) => selected.clone(),
            (Some(user), selected) => {
                ThemeData::from_file_over(user, selected.unwrap_or(ThemeData::base(appearance)))
            }
        };
        Some(Theme::new(data))
    }
}

/// `dark` or `light`.
fn name(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Dark => "dark",
        Appearance::Light => "light",
    }
}

/// The pair's theme for `appearance`.
fn slot<'a>(appearance: Appearance, dark: &'a mut Theme, light: &'a mut Theme) -> &'a mut Theme {
    match appearance {
        Appearance::Dark => dark,
        Appearance::Light => light,
    }
}

/// What's wrong with the themes chosen, as a source of the service's
/// diagnostics: it reads no files.
pub(crate) struct SelectionSource(pub(crate) Layers);

impl ConfigSource for SelectionSource {
    fn paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn reload(&mut self, _: &[PathBuf], _: &mut App) {}

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.0.borrow().diagnostics.clone()
    }
}

/// The user's theme file ([`crate::Config::user_theme`]). Laid over the
/// theme chosen for its appearance's slot (the bundled base, unless an
/// installed one is), it takes that slot of the dark and light pair, so
/// the theme mode still picks between them. A file with errors keeps the
/// last good version; a missing file puts back the theme it replaced.
pub(crate) struct UserTheme {
    path: PathBuf,
    /// The app's tables the file may have ([`crate::Config::theme_extensions`]).
    extensions: &'static [&'static str],
    layers: Layers,
    /// The file last read without errors, while it exists.
    file: Option<ThemeFile>,
    diagnostics: Vec<Diagnostic>,
}

impl UserTheme {
    pub(crate) fn load(path: PathBuf, extensions: &'static [&'static str], layers: Layers) -> Self {
        let mut theme = Self {
            path,
            extensions,
            layers,
            file: None,
            diagnostics: Vec::new(),
        };
        theme.read();
        theme
    }

    /// Re-reads the file. Returns whether it changed.
    fn read(&mut self) -> bool {
        // Only a file that's there is the user's theme: no file is none,
        // with nothing to report.
        if !self.path.exists() {
            self.diagnostics.clear();
            return self.file.take().is_some();
        }
        let (file, diagnostics) = ThemeFile::load(&self.path, self.extensions);
        self.diagnostics = diagnostics;
        let Some(file) = file else {
            return false;
        };
        if self.file.as_ref() == Some(&file) {
            return false;
        }
        self.file = Some(file);
        true
    }

    /// Lays the file into its slot of the pair in use.
    fn apply(&mut self, cx: &mut App) {
        self.layers.borrow_mut().user = self.file.clone();
        self.layers.apply(cx);
    }
}

impl ConfigSource for UserTheme {
    fn paths(&self) -> Vec<PathBuf> {
        vec![self.path.clone()]
    }

    fn user_files(&self) -> Vec<PathBuf> {
        self.paths()
    }

    fn install(&mut self, cx: &mut App) {
        if self.file.is_some() {
            self.apply(cx);
        }
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        if changed.contains(&self.path) && self.read() {
            log::info!(target: log_target(cx), "theme reloaded");
            self.apply(cx);
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.clone()
    }
}
