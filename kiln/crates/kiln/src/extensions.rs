// SPDX-License-Identifier: LicenseRef-Proprietary

//! Extensions the user installed, a directory each under the extensions
//! directory ([`crate::Config::extensions`]), loaded by [`crate::init`]
//! and reloaded as the config service hears of a change under it, like
//! the user's config files: a manifest with errors keeps its last good
//! version, and its errors are in [`kiln_platform::config::diagnostics`].
//!
//! ```ignore
//! kiln::Config::new("com.example.Notes", "Notes")
//!     .extensions(paths.extensions_dir().unwrap(), "notes", ApiVersion { major: 1, minor: 0 })
//!     .extension_kind("lsp", lsp_schema)
//!     .on_extensions(|set, cx| {
//!         for manifest in set.extensions() {
//!             // manifest.app.get("lsp") is the table as written.
//!         }
//!     })
//!     .on_license_notice(|notice, cx| show(notice.message("Notes"), cx));
//! // Anywhere, then:
//! let loaded = kiln::extensions::loaded(cx);
//! // The installed themes, and the two the pair starts from.
//! let themes = kiln::extensions::themes(cx);
//! kiln::extensions::select_themes(cx, Some("acme.solarized/solarized-dark"), None);
//! ```
//!
//! [`ExtensionSet`] is kiln-core's: what's loaded, its diagnostics, and the
//! licence notices.
//!
//! Each file an extension's `contributes.themes` lists is a theme, laid
//! over the bundled base for its appearance, with the app's tables
//! ([`crate::Config::theme_extensions`]). Its ID is `<extension
//! id>/<file stem>`, and its problems are the extension's diagnostics: a
//! file with errors keeps its last good version, if it had one, while
//! it's there. A change under the directory reads them all again, so an
//! edit to the theme in use shows at once.

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use gpui::{App, Global};
use kiln_core::{
    ApiVersion, AppKind, Diagnostic, ExtensionApi, ExtensionSet, LicenseNotice, Manifest, Severity,
    theme::{Appearance, ThemeData, ThemeFile},
};
use kiln_platform::config::{ConfigSource, log_target};

use crate::config::{Installed, Layers};

/// Called with the set as loaded, and after each rescan that changes it.
pub(crate) type OnExtensions = Box<dyn FnMut(&ExtensionSet, &mut App)>;

/// Called with each licence notice ([`ExtensionSet::license_notices`]).
pub(crate) type OnNotice = Box<dyn FnMut(&LicenseNotice, &mut App)>;

/// What [`crate::Config`] says of extensions, for [`crate::init`].
#[derive(Default)]
pub(crate) struct Setup {
    pub(crate) dir: Option<(PathBuf, String, ApiVersion)>,
    pub(crate) kinds: Vec<AppKind>,
    pub(crate) allowed_licenses: Option<Vec<String>>,
    pub(crate) on_extensions: Vec<OnExtensions>,
    pub(crate) on_notice: Vec<OnNotice>,
}

/// The extensions loaded now, in ID order: none without
/// [`crate::Config::extensions`].
pub fn loaded(cx: &App) -> Vec<Manifest> {
    cx.try_global::<Loaded>()
        .map(|loaded| loaded.0.clone())
        .unwrap_or_default()
}

/// A theme an extension installed: [`themes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeInfo {
    /// `<extension id>/<file stem>`: `acme.solarized/solarized-dark`.
    pub id: String,
    /// The file's `name`, else its stem.
    pub title: String,
    pub appearance: Appearance,
}

/// The themes the extensions loaded now install, in ID order: none
/// without [`crate::Config::extensions`].
pub fn themes(cx: &App) -> Vec<ThemeInfo> {
    let Some(layers) = cx.try_global::<Layers>() else {
        return Vec::new();
    };
    let layers = layers.borrow();
    layers
        .installed
        .iter()
        .map(|(id, theme)| ThemeInfo {
            id: id.clone(),
            title: theme.title.clone(),
            appearance: theme.appearance,
        })
        .collect()
}

/// The installed theme each slot of the dark and light pair starts from,
/// by ID ([`themes`]), under the user's theme file; `None` is the bundled
/// one. An ID nothing installs, or a theme of the other appearance, is a
/// warning in [`kiln_platform::config::diagnostics`] ("theme
/// acme.solarized/solarized-dark isn't installed"), and the slot has the
/// bundled one until a rescan installs it. The choice holds until the next
/// call: the app passes its settings' choice at start and on each change.
pub fn select_themes(cx: &mut App, dark: Option<&str>, light: Option<&str>) {
    Layers::get(cx).select(dark, light, cx);
}

/// The extensions loaded, as a GPUI global.
struct Loaded(Vec<Manifest>);

impl Global for Loaded {}

/// The extensions directory, as a source for the config service: it
/// watches the directory's tree, and a batch naming it rescans it.
pub(crate) struct ExtensionSource {
    dir: PathBuf,
    set: ExtensionSet,
    /// The app's tables a theme file may have.
    theme_tables: &'static [&'static str],
    /// The extensions' themes by ID, as last read.
    themes: BTreeMap<String, Installed>,
    /// Their files' problems.
    theme_diagnostics: Vec<Diagnostic>,
    layers: Layers,
    on_extensions: Vec<OnExtensions>,
    on_notice: Vec<OnNotice>,
    /// The notices logged with no callback to give them to.
    logged: HashSet<LicenseNotice>,
}

impl ExtensionSource {
    /// Discovers the extensions in `setup`'s directory, if it names one,
    /// and reads their themes, which may have the app's `theme_tables`;
    /// IDs may not start with a namespace in `reserved`.
    pub(crate) fn load(
        setup: Setup,
        reserved: Vec<String>,
        theme_tables: &'static [&'static str],
        layers: Layers,
    ) -> Option<Self> {
        let (dir, app, version) = setup.dir?;
        let api = ExtensionApi {
            app,
            version,
            reserved,
            kinds: setup.kinds,
            allowed_licenses: setup.allowed_licenses,
        };
        let mut source = Self {
            set: ExtensionSet::discover(&dir, &api),
            dir,
            theme_tables,
            themes: BTreeMap::new(),
            theme_diagnostics: Vec::new(),
            layers,
            on_extensions: setup.on_extensions,
            on_notice: setup.on_notice,
            logged: HashSet::new(),
        };
        source.read_themes();
        Some(source)
    }

    /// Reads every loaded extension's themes again. Returns whether they
    /// or their diagnostics changed.
    fn read_themes(&mut self) -> bool {
        let mut themes = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for manifest in self.set.extensions() {
            for path in &manifest.themes {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let id = format!("{}/{stem}", manifest.id);
                if themes.contains_key(&id) {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        file: path.clone(),
                        key: None,
                        line: 1,
                        column: 1,
                        message: format!(
                            "`{id}` is the ID of another of {}'s themes, which loads instead",
                            manifest.id
                        ),
                    });
                    continue;
                }
                let (file, found) = ThemeFile::load(path, self.theme_tables);
                diagnostics.extend(found);
                let theme = match file {
                    Some(file) => Installed {
                        title: file
                            .name
                            .clone()
                            .unwrap_or_else(|| stem.clone().into_owned()),
                        appearance: file.appearance.unwrap_or_default(),
                        data: ThemeData::from_file(&file),
                    },
                    // Its last good version, while it's there.
                    None => match self.themes.get(&id) {
                        Some(last) if path.exists() => last.clone(),
                        _ => continue,
                    },
                };
                themes.insert(id, theme);
            }
        }
        let changed = themes != self.themes || diagnostics != self.theme_diagnostics;
        self.themes = themes;
        self.theme_diagnostics = diagnostics;
        changed
    }

    /// Gives the themes to the pair's layers, which lay them in.
    fn apply_themes(&mut self, cx: &mut App) {
        {
            let mut layers = self.layers.borrow_mut();
            layers.installed = self.themes.clone();
            layers.root = Some(self.set.root().to_owned());
        }
        self.layers.apply(cx);
    }

    /// Publishes the set, tells the app, then gives the licence notices
    /// it hasn't had to [`crate::Config::on_license_notice`]'s callbacks,
    /// acknowledging them in the state directory, the extensions
    /// directory's parent. Without a callback they only go to the log,
    /// once each while the app runs, and stay unacknowledged, so a later
    /// launch with one shows them.
    fn apply(&mut self, cx: &mut App) {
        cx.set_global(Loaded(self.set.extensions().to_vec()));
        for on_extensions in &mut self.on_extensions {
            on_extensions(&self.set, cx);
        }
        let Some(state_dir) = self.set.root().parent() else {
            return;
        };
        let app = cx
            .try_global::<crate::AppInfo>()
            .map_or_else(|| "The app".into(), |info| info.name.to_string());
        if self.on_notice.is_empty() {
            for notice in self.set.pending_license_notices(state_dir) {
                if self.logged.insert(notice.clone()) {
                    log::warn!(target: log_target(cx), "{}", notice.message(&app));
                }
            }
            return;
        }
        for notice in self.set.license_notices(state_dir) {
            log::info!(target: log_target(cx), "{}", notice.message(&app));
            for on_notice in &mut self.on_notice {
                on_notice(&notice, cx);
            }
        }
    }
}

impl ConfigSource for ExtensionSource {
    fn paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn trees(&self) -> Vec<PathBuf> {
        vec![self.dir.clone()]
    }

    fn install(&mut self, cx: &mut App) {
        self.apply_themes(cx);
        self.apply(cx);
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        if !changed.contains(&self.dir) {
            return;
        }
        let rescanned = self.set.rescan();
        // A theme file may change with no manifest changing.
        if self.read_themes() {
            log::info!(target: log_target(cx), "extension themes reloaded");
            self.apply_themes(cx);
        }
        if rescanned {
            log::info!(target: log_target(cx), "extensions reloaded");
            self.apply(cx);
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.set.diagnostics().to_vec();
        diagnostics.extend(self.theme_diagnostics.iter().cloned());
        diagnostics
    }
}
