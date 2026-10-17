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
//! ```
//!
//! [`ExtensionSet`] is kiln-core's: what's loaded, its diagnostics, and the
//! licence notices.

use std::{collections::HashSet, path::PathBuf};

use gpui::{App, Global};
use kiln_core::{
    ApiVersion, AppKind, Diagnostic, ExtensionApi, ExtensionSet, LicenseNotice, Manifest,
};
use kiln_platform::config::{ConfigSource, log_target};

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

/// The extensions loaded, as a GPUI global.
struct Loaded(Vec<Manifest>);

impl Global for Loaded {}

/// The extensions directory, as a source for the config service: it
/// watches the directory's tree, and a batch naming it rescans it.
pub(crate) struct ExtensionSource {
    dir: PathBuf,
    set: ExtensionSet,
    on_extensions: Vec<OnExtensions>,
    on_notice: Vec<OnNotice>,
    /// The notices logged with no callback to give them to.
    logged: HashSet<LicenseNotice>,
}

impl ExtensionSource {
    /// Discovers the extensions in `setup`'s directory, if it names one;
    /// IDs may not start with a namespace in `reserved`.
    pub(crate) fn load(setup: Setup, reserved: Vec<String>) -> Option<Self> {
        let (dir, app, version) = setup.dir?;
        let api = ExtensionApi {
            app,
            version,
            reserved,
            kinds: setup.kinds,
            allowed_licenses: setup.allowed_licenses,
        };
        Some(Self {
            set: ExtensionSet::discover(&dir, &api),
            dir,
            on_extensions: setup.on_extensions,
            on_notice: setup.on_notice,
            logged: HashSet::new(),
        })
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
        self.apply(cx);
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        if changed.contains(&self.dir) && self.set.rescan() {
            log::info!(target: log_target(cx), "extensions reloaded");
            self.apply(cx);
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.set.diagnostics().to_vec()
    }
}
