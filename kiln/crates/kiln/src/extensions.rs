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
//! // The app's own themes, selected as `builtin/<id>`.
//! kiln::extensions::install_builtin_themes(cx, &[("notes-dusk", &dusk)]);
//! kiln::extensions::select_themes(cx, Some("builtin/notes-dusk"), None);
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
//!
//! Each file an extension's `contributes.keymaps` lists is a keymap layer
//! over the app's, under the user's `keymap.toml`, and each in
//! `contributes.menus` a menu layer ([`MenuBar::extend`]) over the app's
//! menus: the extensions by ID, then their files in order. Extensions add:
//! an unbind (`command = ""`), a menu item with `remove`, and one with the
//! ID of an item the menu already has are dropped with a warning, as is a
//! binding or item for a command the app doesn't have. A change to them
//! resolves the keymap and menus again; a file with errors keeps its last
//! good version while it's there. Their problems are the extensions', not
//! the app's: [`crate::Config::fail_fast`] never panics on them.
//!
//! A contributed file is read only while it's inside its extension's
//! directory, symlinks followed: one that comes to lead outside it keeps
//! its last good version, unread.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{App, Global};
use kiln_core::{
    ApiVersion, AppKind, CommandCatalog, Diagnostic, ExtensionApi, ExtensionSet, ItemKind,
    KeymapLayer, LicenseNotice, Manifest, MenuBar, Os, Severity,
    theme::{Appearance, ThemeData, ThemeFile},
};
use kiln_platform::config::{ConfigDiagnostics, ConfigSource, log_diagnostics_as, log_target};
use toml::de::{DeTable, DeValue};

use crate::config::{Installed, Layers};

/// Called with the set as loaded, and after each rescan that changes it.
pub(crate) type OnExtensions = Box<dyn FnMut(&ExtensionSet, &mut App)>;

/// Called with each licence notice ([`ExtensionSet::license_notices`]).
pub(crate) type OnNotice = Box<dyn FnMut(&LicenseNotice, &mut App)>;

/// The files of an extension the app reads, relative to its directory.
pub(crate) type FilesOf = Box<dyn Fn(&Manifest) -> Vec<PathBuf>>;

/// Each extension's listed files' canonical paths, sizes and times, by the
/// extension's directory.
type Fingerprints = BTreeMap<PathBuf, Vec<Option<(PathBuf, u64, Option<std::time::SystemTime>)>>>;

/// What [`crate::Config`] says of extensions, for [`crate::init`].
#[derive(Default)]
pub(crate) struct Setup {
    pub(crate) dir: Option<(PathBuf, String, ApiVersion)>,
    pub(crate) kinds: Vec<AppKind>,
    pub(crate) allowed_licenses: Option<Vec<String>>,
    pub(crate) on_extensions: Vec<OnExtensions>,
    pub(crate) on_notice: Vec<OnNotice>,
    pub(crate) files_of: Option<FilesOf>,
}

/// The extensions loaded now, in ID order: none without
/// [`crate::Config::extensions`].
pub fn loaded(cx: &App) -> Vec<Manifest> {
    cx.try_global::<Loaded>()
        .map(|loaded| loaded.0.clone())
        .unwrap_or_default()
}

/// A theme an extension or the app installed: [`themes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeInfo {
    /// `<extension id>/<file stem>`: `acme.solarized/solarized-dark`; or
    /// `builtin/<id>` for one of the app's ([`install_builtin_themes`]).
    pub id: String,
    /// The file's `name`, else its stem (or the ID it was installed by).
    pub title: String,
    pub appearance: Appearance,
}

/// The themes installed now, in ID order: the extensions' (none without
/// [`crate::Config::extensions`]) and the app's
/// ([`install_builtin_themes`]).
pub fn themes(cx: &App) -> Vec<ThemeInfo> {
    let Some(layers) = cx.try_global::<Layers>() else {
        return Vec::new();
    };
    let layers = layers.borrow();
    let mut themes: Vec<ThemeInfo> = (layers.installed.iter())
        .chain(&layers.builtin)
        .map(|(id, theme)| ThemeInfo {
            id: id.clone(),
            title: theme.title.clone(),
            appearance: theme.appearance,
        })
        .collect();
    themes.sort_by(|a, b| a.id.cmp(&b.id));
    themes
}

/// The namespace of the app's own themes' IDs: `builtin/<id>`. No
/// extension's ID is one segment, so none can use it.
pub const BUILTIN_THEMES: &str = "builtin";

/// The ID [`themes`] lists, and [`select_themes`] takes, for the app's
/// theme installed as `id`: `builtin/<id>`.
pub fn builtin_theme_id(id: &str) -> String {
    format!("{BUILTIN_THEMES}/{id}")
}

/// Installs the app's own `themes`, each by an ID of its own and its file,
/// laid over the bundled base for its appearance like an extension's. Each
/// is listed by [`themes`] as `builtin/<id>` ([`builtin_theme_id`]), for
/// [`select_themes`] to choose, and stays across rescans of the extensions
/// directory. One of an ID already installed replaces it.
pub fn install_builtin_themes(cx: &mut App, themes: &[(&str, &ThemeFile)]) {
    let layers = Layers::get(cx);
    {
        let mut layers = layers.borrow_mut();
        for &(id, file) in themes {
            let theme = Installed {
                title: file.name.clone().unwrap_or_else(|| id.to_owned()),
                appearance: file.appearance.unwrap_or_default(),
                data: ThemeData::from_file(file),
            };
            layers.builtin.insert(builtin_theme_id(id), theme);
        }
    }
    layers.apply(cx);
}

/// The installed theme each slot of the dark and light pair starts from,
/// by ID ([`themes`]), under the user's theme file; `None` is the bundled
/// one. An ID nothing installs, or a theme of the other appearance, is a
/// warning in [`kiln_platform::config::diagnostics`] ("theme
/// acme.solarized/solarized-dark isn't installed"), and the slot has the
/// bundled one until a rescan installs it. The choice holds until the next
/// call: the app passes its settings' choice at start and on each change.
pub fn select_themes(cx: &mut App, dark: Option<&str>, light: Option<&str>) {
    select_themes_noting(cx, dark, light, Vec::new());
}

/// [`select_themes`], with what the app found wrong in the settings it
/// made the choice from: `notes` go in
/// [`kiln_platform::config::diagnostics`] before the choice's own, and
/// stay there, through the config service's batches, until the next call.
pub fn select_themes_noting(
    cx: &mut App,
    dark: Option<&str>,
    light: Option<&str>,
    notes: Vec<Diagnostic>,
) {
    Layers::get(cx).select(dark, light, notes, cx);
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
    /// The extensions' keymap and menu files, as last read.
    files: Files,
    on_extensions: Vec<OnExtensions>,
    /// The files of the extensions the app reads, as listed by
    /// `files_of`, as of the last time `on_extensions` was called.
    fingerprints: Fingerprints,
    files_of: Option<FilesOf>,
    on_notice: Vec<OnNotice>,
    /// The notices logged with no callback to give them to.
    logged: HashSet<LicenseNotice>,
}

impl ExtensionSource {
    /// Discovers the extensions in `setup`'s directory, if it names one,
    /// and reads their themes, which may have the app's `theme_tables`,
    /// and their keymap and menu files into `files`; IDs may not start
    /// with a namespace in `reserved`.
    pub(crate) fn load(
        setup: Setup,
        reserved: Vec<String>,
        theme_tables: &'static [&'static str],
        layers: Layers,
        files: Files,
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
            files,
            on_extensions: setup.on_extensions,
            fingerprints: Fingerprints::new(),
            files_of: setup.files_of,
            on_notice: setup.on_notice,
            logged: HashSet::new(),
        };
        source.read_themes();
        source.read_files();
        Some(source)
    }

    /// Reads every loaded extension's themes again. Returns whether they
    /// or their diagnostics changed. Each theme's current path is
    /// resolved, following links, and must still be under the extension's
    /// directory, including for a theme listed by a kept last-good
    /// manifest; one that isn't is not read, gets an error naming the
    /// file, and keeps its last good theme.
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
                let (file, found) = match read_contained(manifest, path) {
                    Ok(text) => ThemeFile::parse(path, &text, self.theme_tables),
                    Err(unread) => {
                        diagnostics.push(unread.diagnostic);
                        let last = self.themes.get(&id).filter(|_| unread.keep);
                        if let Some(last) = last {
                            themes.insert(id, last.clone());
                        }
                        continue;
                    }
                };
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
                        Some(last) => last.clone(),
                        None => continue,
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

    /// Reads every loaded extension's keymap and menu files again. Returns
    /// whether they or their diagnostics changed.
    fn read_files(&mut self) -> bool {
        let mut keymaps = Vec::new();
        let mut menus = Vec::new();
        let mut diagnostics = Vec::new();
        for manifest in self.set.extensions() {
            for (paths, files) in [
                (&manifest.keymaps, &mut keymaps),
                (&manifest.menus, &mut menus),
            ] {
                for path in paths {
                    let text = match read_contained(manifest, path) {
                        Ok(text) => Some(text),
                        Err(unread) => {
                            diagnostics.push(unread.diagnostic);
                            if !unread.keep {
                                continue;
                            }
                            None
                        }
                    };
                    files.push(File {
                        path: path.clone(),
                        text,
                    });
                }
            }
        }
        let mut files = self.files.0.borrow_mut();
        let changed = keymaps != files.keymaps || menus != files.menus || diagnostics != files.read;
        files.keymaps = keymaps;
        files.menus = menus;
        files.read = diagnostics;
        // A file no extension lists now has no last good version to keep.
        let listed: HashSet<PathBuf> = (files.keymaps.iter().chain(&files.menus))
            .map(|file| file.path.clone())
            .collect();
        files.last_good.retain(|path, _| listed.contains(path));
        changed
    }

    /// The size and modification time of each file the app lists for each
    /// loaded extension ([`crate::Config::extension_files`]): a file that's
    /// missing, isn't a regular file, or leads outside the extension's
    /// directory has none. Reads no contents.
    ///
    /// Known limit: a rewrite that keeps a file's size and modification
    /// time, as a timestamp-preserving copy of same-length content does,
    /// isn't noticed; detecting it needs hashing the contents, off the main
    /// thread.
    fn fingerprint(&self) -> Fingerprints {
        let Some(files) = &self.files_of else {
            return Fingerprints::new();
        };
        let mut out = Fingerprints::new();
        for manifest in self.set.extensions() {
            let root = manifest.dir.canonicalize().ok();
            let stamps = files(manifest)
                .into_iter()
                .map(|path| {
                    let canonical = manifest.dir.join(path).canonicalize().ok()?;
                    canonical.starts_with(root.as_ref()?).then_some(())?;
                    let meta = fs::metadata(&canonical).ok().filter(|m| m.is_file())?;
                    Some((canonical, meta.len(), meta.modified().ok()))
                })
                .collect();
            out.insert(manifest.dir.clone(), stamps);
        }
        out
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
        self.fingerprints = self.fingerprint();
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
        // So may a keymap or menu file.
        if self.read_files() {
            log::info!(target: log_target(cx), "extension keymaps and menus reloaded");
            crate::contributions::resolve_again(cx);
        }
        // A file the app reads from the manifest, as a grammar's module,
        // may change with no manifest changing.
        let touched = !rescanned && self.fingerprint() != self.fingerprints;
        if rescanned || touched {
            log::info!(target: log_target(cx), "extensions reloaded");
            self.apply(cx);
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.set.diagnostics().to_vec();
        diagnostics.extend(self.theme_diagnostics.iter().cloned());
        let files = self.files.0.borrow();
        diagnostics.extend(files.read.iter().cloned());
        diagnostics.extend(files.laid.iter().cloned());
        diagnostics
    }
}

/// Why a contributed file wasn't read.
struct Unread {
    diagnostic: Diagnostic,
    /// Whether its last good version stays: it's there, but can't be read,
    /// or leads outside its extension's directory. A missing file's
    /// doesn't.
    keep: bool,
}

/// The text of `manifest`'s contributed file at `path`, if it's there and,
/// symlinks followed, inside the extension's directory. Checked as it's
/// read, not only when the manifest is: a file may come to lead outside
/// the directory while the manifest's last good version stays.
fn read_contained(manifest: &Manifest, path: &Path) -> Result<String, Unread> {
    let unread = |keep, message: String| Unread {
        diagnostic: Diagnostic {
            severity: Severity::Error,
            file: path.to_owned(),
            key: None,
            line: 1,
            column: 1,
            message,
        },
        keep,
    };
    let canonical = match path.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(unread(false, "no such file".into()));
        }
        Err(error) => return Err(unread(true, format!("can't be read: {error}"))),
    };
    let inside = (manifest.dir.canonicalize()).is_ok_and(|dir| canonical.starts_with(dir));
    if !inside {
        let message = format!(
            "leads to {}, outside {}'s directory, so it isn't read",
            canonical.display(),
            manifest.id
        );
        return Err(unread(true, message));
    }
    std::fs::read_to_string(&canonical).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => unread(false, "no such file".into()),
        _ => unread(true, format!("can't be read: {error}")),
    })
}

/// A contributed keymap or menu file, as read: `None` if it's there but
/// can't be read now, so its last good version stays.
#[derive(Clone, Debug, PartialEq)]
struct File {
    path: PathBuf,
    text: Option<String>,
}

#[derive(Default)]
struct FileLayers {
    /// The loaded extensions' keymap files, by extension ID then in the
    /// order each lists them.
    keymaps: Vec<File>,
    /// Their menu files, the same way.
    menus: Vec<File>,
    /// What reading them found.
    read: Vec<Diagnostic>,
    /// What laying them in found, as last resolved.
    laid: Vec<Diagnostic>,
    /// Each file's text as last laid in without errors.
    last_good: HashMap<PathBuf, String>,
}

/// The extensions' keymap and menu files, shared by [`ExtensionSource`],
/// which reads them, and the keymap and menus' resolution, which lays
/// them in ([`keymap_layers`], [`extend_menus`]), as a GPUI global.
#[derive(Clone, Default)]
pub(crate) struct Files(Rc<RefCell<FileLayers>>);

impl Global for Files {}

impl Files {
    /// The app's files: those [`crate::init`] set up, else new ones.
    pub(crate) fn get(cx: &mut App) -> Self {
        cx.default_global::<Files>().clone()
    }
}

/// `file` parsed with `parse`: its text's layer, with the text, and
/// diagnostics, which with no errors becomes its last good version; else,
/// or if it can't be read, its last good version's layer, with no text,
/// as what's dropped from it was reported when it was read.
fn parse_kept<'a, T>(
    file: &'a File,
    last_good: &mut HashMap<PathBuf, String>,
    parse: impl Fn(&str) -> (Option<T>, Vec<Diagnostic>),
) -> (Option<(T, Option<&'a str>)>, Vec<Diagnostic>) {
    let kept = |last_good: &HashMap<PathBuf, String>| {
        let text = last_good.get(&file.path)?;
        Some((parse(text).0?, None))
    };
    let Some(text) = &file.text else {
        return (kept(last_good), Vec::new());
    };
    match parse(text) {
        (Some(layer), found) => {
            last_good.insert(file.path.clone(), text.clone());
            (Some((layer, Some(text.as_str()))), found)
        }
        (None, found) => (kept(last_good), found),
    }
}

/// The extensions' keymap layers on `os`, their bindings checked against
/// `catalog`, to lay over the app's, and their diagnostics: an unbind is
/// dropped with a warning, as a binding to a command `catalog` doesn't
/// know is.
pub(crate) fn keymap_layers(
    cx: &App,
    os: Os,
    catalog: &dyn CommandCatalog,
) -> (Vec<KeymapLayer>, Vec<Diagnostic>) {
    let Some(files) = cx.try_global::<Files>() else {
        return (Vec::new(), Vec::new());
    };
    let mut files = files.0.borrow_mut();
    let files = &mut *files;
    let mut layers = Vec::new();
    let mut diagnostics = Vec::new();
    for file in &files.keymaps {
        let (layer, found) = parse_kept(file, &mut files.last_good, |text| {
            KeymapLayer::parse_for(&file.path, text, Some(catalog), os)
        });
        diagnostics.extend(found);
        let Some((mut layer, current)) = layer else {
            continue;
        };
        layer.entries.retain(|entry| {
            if entry.command.is_some() {
                return true;
            }
            if current.is_some() {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    file: entry.location.file.clone(),
                    key: None,
                    line: entry.location.line,
                    column: entry.location.column,
                    message: format!(
                        "an extension can't unbind keys: `command = \"\"` for {} is ignored",
                        entry.keys
                    ),
                });
            }
            false
        });
        layers.push(layer);
    }
    (layers, diagnostics)
}

/// Merges the extensions' menu layers on `os` into `bar`, their items
/// checked against `catalog` and their shortcuts resolved in `frames` (the
/// app's [`crate::Config::menu_shortcut_context`]), and returns their
/// diagnostics. An item that removes one, or has the ID of one its menu
/// already has, is dropped with a warning, as one for a command `catalog`
/// doesn't know is: extensions add menu items, they don't take the app's
/// away.
pub(crate) fn extend_menus(
    cx: &App,
    bar: &mut MenuBar,
    os: Os,
    catalog: &dyn CommandCatalog,
    frames: &[String],
) -> Vec<Diagnostic> {
    let Some(files) = cx.try_global::<Files>() else {
        return Vec::new();
    };
    let mut files = files.0.borrow_mut();
    let files = &mut *files;
    let frames: Vec<&str> = frames.iter().map(String::as_str).collect();
    let mut diagnostics = Vec::new();
    for file in &files.menus {
        let (layer, found) = parse_kept(file, &mut files.last_good, |text| {
            MenuBar::parse_in(&file.path, text, Some(catalog), os, &frames)
        });
        diagnostics.extend(found);
        let Some((mut layer, current)) = layer else {
            continue;
        };
        for menu in &mut layer.menus {
            let own = bar.menu(&menu.name);
            menu.items.retain(|item| {
                let message = match (&item.kind, item.id()) {
                    (ItemKind::Remove(id), _) => format!(
                        "an extension can't remove menu items: the removal of {id:?} \
                         from {:?} is ignored",
                        menu.name
                    ),
                    (_, Some(id)) if own.is_some_and(|own| own.item(id).is_some()) => format!(
                        "{:?} already has an item {id:?}; an extension can't replace it, \
                         so this one is ignored",
                        menu.name
                    ),
                    _ => return true,
                };
                if let Some(text) = current {
                    let id = item.id().unwrap_or_default();
                    let (key, offset) = item_at(text, &menu.name, id).unzip();
                    let (line, column) = line_column(text, offset.unwrap_or_default());
                    diagnostics.push(Diagnostic {
                        severity: Severity::Warning,
                        file: file.path.clone(),
                        key,
                        line,
                        column,
                        message,
                    });
                }
                false
            });
        }
        bar.extend(layer);
    }
    diagnostics
}

/// The key ([`MenuBar::parse`]'s, as in `menus[0].items[2]`) and offset in
/// a menu file's `source` of the first item in `menu` with `id` (its own,
/// else its command's or system menu's), or naming it to remove.
fn item_at(source: &str, menu: &str, id: &str) -> Option<(String, usize)> {
    let document = DeTable::parse(source).ok()?;
    let field = |table: &DeTable<'_>, name: &str| match table.get(name).map(|value| value.get_ref())
    {
        Some(DeValue::String(text)) => Some(text.to_string()),
        _ => None,
    };
    let DeValue::Array(menus) = document.get_ref().get("menus")?.get_ref() else {
        return None;
    };
    for (menu_ix, value) in menus.iter().enumerate() {
        let DeValue::Table(table) = value.get_ref() else {
            continue;
        };
        if field(table, "name").as_deref() != Some(menu) {
            continue;
        }
        let Some(DeValue::Array(items)) = table.get("items").map(|items| items.get_ref()) else {
            continue;
        };
        for (item_ix, item) in items.iter().enumerate() {
            let DeValue::Table(fields) = item.get_ref() else {
                continue;
            };
            let item_id = field(fields, "id")
                .or_else(|| field(fields, "command"))
                .or_else(|| field(fields, "system"));
            if item_id.as_deref() == Some(id) {
                let key = format!("menus[{menu_ix}].items[{item_ix}]");
                return Some((key, item.span().start));
            }
        }
    }
    None
}

/// The 1-based line and column (in chars) of byte `offset` in `source`.
fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
}

/// Keeps `diagnostics`, what laying the extensions' files in found, as
/// theirs. If they changed and `publish`, logs them and puts them in place
/// of the last in [`ConfigDiagnostics`]: a resolution comes between the
/// config service's batches, which publish them with the rest.
pub(crate) fn set_laid(cx: &mut App, diagnostics: Vec<Diagnostic>, publish: bool) {
    let Some(files) = cx.try_global::<Files>().cloned() else {
        return;
    };
    let previous = std::mem::replace(&mut files.0.borrow_mut().laid, diagnostics.clone());
    if !publish || previous == diagnostics {
        return;
    }
    log_diagnostics_as(log_target(cx), &diagnostics);
    let mut all = cx
        .try_global::<ConfigDiagnostics>()
        .map(|all| all.0.clone())
        .unwrap_or_default();
    all.retain(|diagnostic| !previous.contains(diagnostic));
    all.extend(diagnostics);
    cx.set_global(ConfigDiagnostics(all));
}
