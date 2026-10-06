//! Loading the default and user keymaps, and reloading the user's when it
//! changes.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_config::Diagnostic;

use crate::keymap::{CommandCatalog, Keymap, KeymapLayer};

/// The user keymap's name, in the user config directory.
pub const KEYMAP_FILE: &str = "keymap.toml";

/// The path diagnostics and bindings name for the built-in default keymap.
pub const DEFAULT_KEYMAP_PATH: &str = "<default keymap>";

/// `<config_dir>/keymap.toml`: see [`fe_config::config_dir`].
pub fn user_keymap_path() -> Option<PathBuf> {
    fe_config::config_dir().map(|dir| dir.join(KEYMAP_FILE))
}

/// The keymap from the built-in `default_source` and the user's file, if
/// any, and their diagnostics. A file with errors is left out; a missing
/// user file is no bindings. No catalog is consulted: see
/// [`KeymapStore::load`].
pub fn load_keymap(default_source: &str, user: Option<&Path>) -> (Keymap, Vec<Diagnostic>) {
    let store = KeymapStore::load(default_source, user.map(Path::to_owned), None);
    (Keymap::clone(&store.keymap), store.diagnostics)
}

/// Reads the keymap file at `path` through [`fe_config::load_layer`]: the
/// layer if it has no errors, and the diagnostics either way. A missing file
/// is an empty layer.
pub fn load_keymap_layer(
    path: &Path,
    catalog: Option<&dyn CommandCatalog>,
) -> (Option<KeymapLayer>, Vec<Diagnostic>) {
    let layer = fe_config::load_layer(path);
    if layer.has_errors() {
        return (None, layer.diagnostics);
    }
    KeymapLayer::parse(path, &layer.source, catalog)
}

/// What a reload changed.
#[derive(Clone, Debug)]
pub struct KeymapEvent {
    /// The effective keymap now, for [`crate::Matcher::set_keymap`].
    pub keymap: Arc<Keymap>,
    /// The effective keymap before the reload.
    pub previous: Arc<Keymap>,
    /// Every layer's diagnostics now.
    pub diagnostics: Vec<Diagnostic>,
}

/// The effective keymap: the built-in defaults, then the user's file.
///
/// A file with an error keeps its layer's last good version, which is empty
/// until it has loaded cleanly once, so a typo never drops the user's
/// bindings. Warnings, such as an unknown command, don't reject a file.
pub struct KeymapStore {
    default: KeymapLayer,
    default_diagnostics: Vec<Diagnostic>,
    user: Option<UserLayer>,
    catalog: Option<Box<dyn CommandCatalog>>,
    keymap: Arc<Keymap>,
    diagnostics: Vec<Diagnostic>,
}

struct UserLayer {
    path: PathBuf,
    /// The layer last loaded without errors.
    good: KeymapLayer,
    diagnostics: Vec<Diagnostic>,
}

impl UserLayer {
    /// Re-reads the file, keeping the last good layer if it has errors.
    fn read(&mut self, catalog: Option<&dyn CommandCatalog>) {
        let (layer, diagnostics) = load_keymap_layer(&self.path, catalog);
        if let Some(layer) = layer {
            self.good = layer;
        }
        self.diagnostics = diagnostics;
    }
}

impl KeymapStore {
    /// Loads the built-in `default_source` and the user's keymap file at
    /// `user`, which may be missing. With a `catalog`, such as the command
    /// [`fe_command::Registry`], bindings to unknown commands are dropped
    /// with a warning and bad args are errors.
    pub fn load(
        default_source: &str,
        user: Option<PathBuf>,
        catalog: Option<Box<dyn CommandCatalog>>,
    ) -> Self {
        let (default, default_diagnostics) = KeymapLayer::parse(
            Path::new(DEFAULT_KEYMAP_PATH),
            default_source,
            catalog.as_deref(),
        );
        let user = user.map(|path| {
            let mut layer = UserLayer {
                good: KeymapLayer {
                    path: path.clone(),
                    entries: Vec::new(),
                },
                path,
                diagnostics: Vec::new(),
            };
            layer.read(catalog.as_deref());
            layer
        });
        let mut store = Self {
            default: default.unwrap_or_default(),
            default_diagnostics,
            user,
            catalog,
            keymap: Arc::default(),
            diagnostics: Vec::new(),
        };
        (store.keymap, store.diagnostics) = store.effective();
        store
    }

    pub fn keymap(&self) -> &Arc<Keymap> {
        &self.keymap
    }

    /// Every layer's diagnostics, the defaults' first.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The user's keymap file, for a [`fe_config::ConfigWatcher`].
    pub fn paths(&self) -> Vec<PathBuf> {
        self.user.iter().map(|layer| layer.path.clone()).collect()
    }

    /// Re-reads the user's file if it's in `changed`. Returns what changed
    /// if the effective keymap or the diagnostics did, and `None` if neither
    /// did.
    pub fn reload(&mut self, changed: &[PathBuf]) -> Option<KeymapEvent> {
        let user = self.user.as_mut()?;
        if !changed.contains(&user.path) {
            return None;
        }
        user.read(self.catalog.as_deref());
        let (keymap, diagnostics) = self.effective();
        if keymap == self.keymap && diagnostics == self.diagnostics {
            return None;
        }
        let previous = std::mem::replace(&mut self.keymap, keymap);
        self.diagnostics = diagnostics;
        Some(KeymapEvent {
            keymap: self.keymap.clone(),
            previous,
            diagnostics: self.diagnostics.clone(),
        })
    }

    /// The layers' good versions combined, and their diagnostics.
    fn effective(&self) -> (Arc<Keymap>, Vec<Diagnostic>) {
        let user = self.user.as_ref();
        let layers = std::iter::once(&self.default).chain(user.map(|user| &user.good));
        let keymap = Arc::new(Keymap::from_layers(layers));
        let diagnostics = self
            .default_diagnostics
            .iter()
            .chain(user.into_iter().flat_map(|user| &user.diagnostics))
            .cloned()
            .collect();
        (keymap, diagnostics)
    }
}
