// SPDX-License-Identifier: Apache-2.0

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
//!
//! A workspace folder's own extensions, `<root>/.<app>/extensions`
//! ([`kiln_core::AppPaths::project_extensions_dir`]), load beside the
//! user's for the root the app sets ([`set_workspace_root`]), once
//! [`crate::Config::workspace_extensions`] turns them on, and only while
//! the root is trusted ([`kiln_shell::trust`]): their directory is then
//! watched, and they're dropped when trust is withdrawn or the root
//! changes. Until it's trusted, they're [`skipped`] and contribute nothing.
//! They may contribute only the kinds the app allows
//! ([`kiln_core::ExtensionApi::only_kinds`]), never themes, keymaps or
//! menus, and give no licence notices. One with the ID of one of the
//! user's isn't loaded, with a warning: the user's copy is.
//! [`crate::Config::on_extensions`] gets the user's then the workspace's
//! ([`ExtensionSet::joined`], [`ExtensionSet::sourced`]).
//!
//! The app may disable installed extensions by ID ([`set_disabled`], the
//! user's `[extensions] disabled`): one disabled is still discovered and
//! [`loaded`], but contributes nothing. Its themes aren't installed (one
//! selected falls back as one that isn't installed does), its keymap and
//! menu files aren't laid in, and [`crate::Config::on_extensions`] gets
//! the set without it. Enabling it again applies it as a rescan would.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{App, AppContext as _, Entity, Global, Subscription, Task};
use kiln_core::{
    ApiVersion, AppKind, CommandCatalog, Diagnostic, ExtensionApi, ExtensionSet, ItemKind,
    KeymapLayer, LicenseNotice, Manifest, MenuBar, Os, Severity, TreeWatcher,
    extension::ExtensionSource as Source,
    theme::{Appearance, ThemeData, ThemeFile},
    trust::canonical,
};
use kiln_platform::config::{
    ConfigDiagnostics, ConfigSource, batch_channel, log_diagnostics_as, log_target,
};
use toml::de::{DeTable, DeValue};

use crate::config::{Installed, Layers};

/// Called with the set as loaded, and after each rescan that changes it.
pub(crate) type OnExtensions = Box<dyn FnMut(&ExtensionSet, &mut App)>;

/// Called with each licence notice ([`ExtensionSet::license_notices`]).
pub(crate) type OnNotice = Box<dyn FnMut(&LicenseNotice, &mut App)>;

/// The files of an extension the app reads, relative to its directory.
pub(crate) type FilesOf = Box<dyn Fn(&Manifest) -> Vec<PathBuf>>;

/// Each extension's listed files' canonical paths, sizes and times, by the
/// extension's directory: the cheap check, made on the UI thread.
type Fingerprints = BTreeMap<PathBuf, Vec<Option<(PathBuf, u64, Option<std::time::SystemTime>)>>>;

/// The hash of each listed file's contents, by canonical path.
type Hashes = BTreeMap<PathBuf, Option<u64>>;

/// Hashes the contents of the files `fingerprints` has. Reads them all, so
/// it's for a background thread; a file that can't be read has none.
fn hash_files(fingerprints: &Fingerprints) -> Hashes {
    use std::hash::Hasher;
    let paths = fingerprints.values().flatten().flatten().map(|(p, ..)| p);
    paths
        .map(|path| {
            let hash = fs::read(path).ok().map(|bytes| {
                // `DefaultHasher::new()` has fixed keys: equal bytes hash equal.
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                hasher.write(&bytes);
                hasher.finish()
            });
            (path.clone(), hash)
        })
        .collect()
}

/// What [`crate::Config`] says of extensions, for [`crate::init`].
#[derive(Default)]
pub(crate) struct Setup {
    pub(crate) dir: Option<(PathBuf, String, ApiVersion)>,
    pub(crate) kinds: Vec<AppKind>,
    pub(crate) allowed_licenses: Option<Vec<String>>,
    pub(crate) on_extensions: Vec<OnExtensions>,
    pub(crate) on_notice: Vec<OnNotice>,
    pub(crate) files_of: Option<FilesOf>,
    /// The kinds a workspace's extensions may contribute, if it may have
    /// any ([`crate::Config::workspace_extensions`]).
    pub(crate) workspace_kinds: Option<Vec<String>>,
}

/// The extensions loaded now: the user's in ID order, then the
/// workspace's ([`set_workspace_root`]) in theirs. None without
/// [`crate::Config::extensions`].
pub fn loaded(cx: &App) -> Vec<Manifest> {
    loaded_from(cx)
        .into_iter()
        .map(|(_, manifest)| manifest)
        .collect()
}

/// [`loaded`], each with where it's installed.
pub fn loaded_from(cx: &App) -> Vec<(Source, Manifest)> {
    cx.try_global::<Loaded>()
        .map(|loaded| loaded.0.clone())
        .unwrap_or_default()
}

/// A workspace extension that isn't loaded: [`skipped`].
#[derive(Clone, Debug, PartialEq)]
pub struct Skipped {
    /// Its manifest, read but not loaded.
    pub manifest: Manifest,
    /// Why: "/work/project isn't trusted".
    pub reason: String,
}

/// The workspace's extensions that aren't loaded because its root isn't
/// trusted, in ID order: each whose manifest reads. None once the root is
/// trusted, or without one.
pub fn skipped(cx: &App) -> Vec<Skipped> {
    cx.try_global::<SkippedGlobal>()
        .map(|skipped| skipped.0.clone())
        .unwrap_or_default()
}

/// Sets the workspace folder whose extensions load beside the user's, if
/// [`crate::Config::workspace_extensions`] turned them on: they load
/// while `root` is trusted, from then on as its trust changes, and those
/// of the root set before are dropped. `None` drops them. Not to be called
/// from [`crate::Config::on_extensions`]'s callback.
pub fn set_workspace_root(cx: &mut App, root: Option<PathBuf>) {
    let Some(source) = cx.try_global::<Running>().map(|running| running.0.clone()) else {
        return;
    };
    {
        let mut source = source.borrow_mut();
        let Some(workspace) = source.workspace.as_mut() else {
            return;
        };
        let app = workspace.api.app.clone();
        let root = root.map(|root| {
            let root = canonical(&root);
            let dir = root.join(format!(".{app}")).join("extensions");
            (root, dir)
        });
        if workspace.root == root {
            return;
        }
        workspace.root = root;
        workspace.trusted = None;
    }
    follow_trust(cx);
    ExtensionSource::settle_workspace(&source, cx);
}

/// Has the workspace's extensions follow the trust decisions, once.
fn follow_trust(cx: &mut App) {
    if cx.has_global::<FollowingTrust>() {
        return;
    }
    let follower = cx.new(|cx| TrustFollower {
        _observer: kiln_shell::trust::observe(cx, |_, cx| {
            if let Some(source) = cx.try_global::<Running>().map(|running| running.0.clone()) {
                ExtensionSource::settle_workspace(&source, cx);
            }
        }),
    });
    cx.set_global(FollowingTrust(follower));
}

/// Holds the subscription to the trust decisions.
struct TrustFollower {
    _observer: Subscription,
}

struct FollowingTrust(#[allow(dead_code, reason = "held, not read")] Entity<TrustFollower>);

impl Global for FollowingTrust {}

/// The extension source, for [`set_workspace_root`] and the trust
/// decisions to reach.
pub(crate) struct Running(pub(crate) Rc<RefCell<ExtensionSource>>);

impl Global for Running {}

struct SkippedGlobal(Vec<Skipped>);

impl Global for SkippedGlobal {}

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
struct Loaded(Vec<(Source, Manifest)>);

impl Global for Loaded {}

/// Disables the installed extensions with these IDs, and enables the rest
/// ([`crate::extensions`]): each one disabled stays [`loaded`], and its
/// themes, keymap and menu files and app kinds go, with no restart, as
/// those of one enabled again come back. An ID no extension has is no
/// error: it's disabled if one with it is installed. Set before
/// [`crate::init`], they never load. Not to be called from
/// [`crate::Config::on_extensions`]'s callback.
pub fn set_disabled<S: Into<String>>(cx: &mut App, ids: impl IntoIterator<Item = S>) {
    let ids: BTreeSet<String> = ids.into_iter().map(Into::into).collect();
    if cx
        .try_global::<Disabled>()
        .is_some_and(|disabled| disabled.0 == ids)
    {
        return;
    }
    cx.set_global(Disabled(ids.clone()));
    if let Some(source) = cx.try_global::<Running>().map(|running| running.0.clone()) {
        ExtensionSource::disable(&source, ids, cx);
    }
}

/// The IDs [`set_disabled`] last disabled, in order, whether or not an
/// extension has them.
pub fn disabled(cx: &App) -> Vec<String> {
    (cx.try_global::<Disabled>())
        .map(|disabled| disabled.0.iter().cloned().collect())
        .unwrap_or_default()
}

/// Calls `f` after each change to what [`loaded_from`], [`skipped`] and
/// [`disabled`] give, while the subscription is held: a view listing them
/// follows them so.
pub fn observe(cx: &mut App, f: impl Fn(&mut App) + 'static) -> Subscription {
    let f = Rc::new(f);
    let on = || {
        let f = f.clone();
        move |cx: &mut App| f(cx)
    };
    let loaded = cx.observe_global::<Loaded>(on());
    let skipped = cx.observe_global::<SkippedGlobal>(on());
    let disabled = cx.observe_global::<Disabled>(on());
    Subscription::join(Subscription::join(loaded, skipped), disabled)
}

/// The IDs disabled ([`set_disabled`]), as a GPUI global.
#[derive(Default)]
pub(crate) struct Disabled(pub(crate) BTreeSet<String>);

impl Global for Disabled {}

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
    /// The hash of each of those files' contents, by canonical path, once
    /// [`hash_files`] has read them; none until then.
    hashes: Option<Hashes>,
    /// Whether a reload found a change the extensions applied don't have
    /// yet, its hashing still in flight.
    dirty: bool,
    /// Which fingerprint the hashing in flight is for: a result for an
    /// older one is dropped.
    generation: u64,
    files_of: Option<FilesOf>,
    on_notice: Vec<OnNotice>,
    /// The notices logged with no callback to give them to.
    logged: HashSet<LicenseNotice>,
    /// The workspace's extensions, if the app takes them.
    workspace: Option<Workspace>,
    /// The IDs of the extensions that contribute nothing ([`set_disabled`]).
    disabled: BTreeSet<String>,
    /// Whether the config service has installed it: until then, nothing's
    /// applied.
    installed: bool,
}

/// A workspace folder's extensions: [`set_workspace_root`].
struct Workspace {
    /// The user's, with only the kinds a workspace may contribute.
    api: ExtensionApi,
    /// Whether its directory is watched by a watcher of its own: not when
    /// the app's batches are given ([`crate::Config::watch_with`]), which
    /// name it instead.
    watch: bool,
    /// The root, canonical, and its extensions directory.
    root: Option<(PathBuf, PathBuf)>,
    /// Whether the root was trusted when last settled: `None` until it is.
    trusted: Option<bool>,
    /// Its extensions, while the root is trusted.
    set: Option<ExtensionSet>,
    /// The watcher on its directory, and the task applying its batches,
    /// while it's loaded.
    watching: Option<(TreeWatcher, Task<()>)>,
}

impl Workspace {
    /// The directory, while its extensions are loaded.
    fn loaded_dir(&self) -> Option<&Path> {
        self.set.as_ref()?;
        self.root.as_ref().map(|(_, dir)| dir.as_path())
    }
}

impl ExtensionSource {
    /// Discovers the extensions in `setup`'s directory, if it names one,
    /// and reads their themes, which may have the app's `theme_tables`,
    /// and their keymap and menu files into `files`; IDs may not start
    /// with a namespace in `reserved`.
    /// A workspace's extensions, if `setup` takes them, are watched with
    /// a watcher of their own if `watch`. Those with an ID in `disabled`
    /// contribute nothing.
    pub(crate) fn load(
        setup: Setup,
        reserved: Vec<String>,
        theme_tables: &'static [&'static str],
        layers: Layers,
        files: Files,
        watch: bool,
        disabled: BTreeSet<String>,
    ) -> Option<Self> {
        let (dir, app, version) = setup.dir?;
        let api = ExtensionApi {
            app,
            version,
            reserved,
            kinds: setup.kinds,
            allowed_licenses: setup.allowed_licenses,
            only_kinds: None,
        };
        let workspace = setup.workspace_kinds.map(|kinds| Workspace {
            api: ExtensionApi {
                only_kinds: Some(kinds),
                ..api.clone()
            },
            watch,
            root: None,
            trusted: None,
            set: None,
            watching: None,
        });
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
            hashes: None,
            dirty: false,
            generation: 0,
            files_of: setup.files_of,
            on_notice: setup.on_notice,
            logged: HashSet::new(),
            workspace,
            disabled,
            installed: false,
        };
        source.read_themes();
        source.read_files();
        Some(source)
    }

    /// Reads every enabled extension's themes again. Returns whether they
    /// or their diagnostics changed. Each theme's current path is
    /// resolved, following links, and must still be under the extension's
    /// directory, including for a theme listed by a kept last-good
    /// manifest; one that isn't is not read, gets an error naming the
    /// file, and keeps its last good theme.
    fn read_themes(&mut self) -> bool {
        let mut themes = BTreeMap::new();
        let mut diagnostics = Vec::new();
        let enabled = (self.set.extensions().iter()).filter(|m| !self.disabled.contains(&m.id));
        for manifest in enabled {
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

    /// Reads every enabled extension's keymap and menu files again.
    /// Returns whether they or their diagnostics changed.
    fn read_files(&mut self) -> bool {
        let mut keymaps = Vec::new();
        let mut menus = Vec::new();
        let mut diagnostics = Vec::new();
        let enabled = (self.set.extensions().iter()).filter(|m| !self.disabled.contains(&m.id));
        for manifest in enabled {
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
        files.last_good.retain(|(_, path), _| listed.contains(path));
        changed
    }

    /// The size and modification time of each file the app lists for each
    /// enabled extension ([`crate::Config::extension_files`]): a file that's
    /// missing, isn't a regular file, or leads outside the extension's
    /// directory has none. Reads no contents: a rewrite that keeps a file's
    /// size and modification time is found by [`hash_files`], which
    /// [`ExtensionSource::check_contents`] runs off the main thread.
    fn fingerprint(&self) -> Fingerprints {
        let Some(files) = &self.files_of else {
            return Fingerprints::new();
        };
        let mut out = Fingerprints::new();
        for manifest in self.enabled().extensions() {
            let root = kiln_core::platform::canonicalize(&manifest.dir).ok();
            let stamps = files(manifest)
                .into_iter()
                .map(|path| {
                    let canonical =
                        kiln_core::platform::canonicalize(&manifest.dir.join(path)).ok()?;
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
        self.install_themes();
        self.layers.apply(cx);
    }

    /// Puts the themes in the pair's layers, for them to lay in.
    fn install_themes(&mut self) {
        let mut layers = self.layers.borrow_mut();
        layers.installed = self.themes.clone();
        layers.root = Some(self.set.root().to_owned());
    }

    /// Publishes the set, tells the app of those enabled, then gives the
    /// licence notices it hasn't had to
    /// [`crate::Config::on_license_notice`]'s callbacks, acknowledging them
    /// in the state directory, the extensions directory's parent. Without a callback they only go to the log,
    /// once each while the app runs, and stay unacknowledged, so a later
    /// launch with one shows them.
    fn apply(&mut self, cx: &mut App) {
        publish_loaded(&self.joined(), cx);
        self.fingerprints = self.fingerprint();
        self.hashes = None;
        self.dirty = false;
        let enabled = self.enabled();
        for on_extensions in &mut self.on_extensions {
            on_extensions(&enabled, cx);
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

impl ExtensionSource {
    /// The user's extensions, with the workspace's joined after while
    /// they're loaded.
    fn joined(&self) -> ExtensionSet {
        match self.workspace.as_ref().and_then(|ws| ws.set.as_ref()) {
            Some(workspace) => self.set.joined(workspace),
            None => self.set.clone(),
        }
    }

    /// [`ExtensionSource::joined`] without those disabled: what the app's
    /// told of.
    fn enabled(&self) -> ExtensionSet {
        self.joined()
            .filtered(|manifest| !self.disabled.contains(&manifest.id))
    }

    /// Disables the extensions with these IDs, and enables the rest: their
    /// themes, keymaps and menus are read again and applied, and the app
    /// told, as after a rescan. The selection's and the extensions'
    /// diagnostics are published, as this comes between the config
    /// service's batches. Before the service installs the source, they're
    /// only read again: installing applies them.
    fn disable(this: &Rc<RefCell<Self>>, ids: BTreeSet<String>, cx: &mut App) {
        let previous = this.borrow().diagnostics();
        let stamps = {
            let mut source = this.borrow_mut();
            if source.disabled == ids {
                return;
            }
            source.disabled = ids;
            if !source.installed {
                source.read_themes();
                if source.read_files() {
                    crate::contributions::resolve_again(cx);
                }
                return;
            }
            if source.read_themes() {
                source.install_themes();
                source.layers.apply_publishing(cx);
            }
            if source.read_files() {
                crate::contributions::resolve_again(cx);
            }
            if source.files_of.is_none() {
                source.apply(cx);
                None
            } else {
                // Told once the files are hashed, as after a rescan.
                source.dirty = true;
                Some(source.fingerprint())
            }
        };
        if let Some(stamps) = stamps {
            Self::hash(this, stamps, cx);
        }
        let diagnostics = this.borrow().diagnostics();
        replace_diagnostics(cx, &previous, diagnostics);
    }

    /// The extensions' problems now: the user's, the workspace's while
    /// they're loaded, and their themes', keymaps' and menus'.
    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.joined().diagnostics().to_vec();
        diagnostics.extend(self.theme_diagnostics.iter().cloned());
        let files = self.files.0.borrow();
        diagnostics.extend(files.read.iter().cloned());
        diagnostics.extend(files.laid.iter().cloned());
        diagnostics
    }

    /// Loads the workspace's extensions if its root is trusted now and
    /// they aren't loaded, and drops them if it isn't and they are, then
    /// applies the extensions and publishes their diagnostics, between
    /// the config service's batches. Nothing happens if the root and its
    /// trust are as last settled.
    fn settle_workspace(this: &Rc<RefCell<Self>>, cx: &mut App) {
        let previous = this.borrow().diagnostics();
        {
            let mut source = this.borrow_mut();
            let Some(workspace) = source.workspace.as_mut() else {
                return;
            };
            let trusted = (workspace.root.as_ref())
                .is_some_and(|(root, _)| kiln_shell::trust::decided(root, cx) == Some(true));
            if workspace.trusted == Some(trusted) {
                return;
            }
            workspace.trusted = Some(trusted);
            workspace.set = None;
            workspace.watching = None;
            let mut skipped = Vec::new();
            if let Some((root, dir)) = &workspace.root {
                let set = ExtensionSet::discover_from(dir, &workspace.api, Source::Workspace);
                if trusted {
                    log::info!(target: log_target(cx), "loading {}'s extensions", root.display());
                    workspace.set = Some(set);
                } else {
                    let reason = format!("{} isn't trusted", root.display());
                    skipped = (set.extensions().iter())
                        .map(|manifest| Skipped {
                            manifest: manifest.clone(),
                            reason: reason.clone(),
                        })
                        .collect();
                }
            }
            cx.set_global(SkippedGlobal(skipped));
        }
        Self::watch_workspace(this, cx);
        let stamps = {
            let mut source = this.borrow_mut();
            if source.files_of.is_none() {
                source.apply(cx);
                None
            } else {
                publish_loaded(&source.joined(), cx);
                source.dirty = true;
                Some(source.fingerprint())
            }
        };
        if let Some(stamps) = stamps {
            Self::hash(this, stamps, cx);
        }
        let diagnostics = this.borrow().diagnostics();
        replace_diagnostics(cx, &previous, diagnostics);
    }

    /// Watches the workspace's extensions directory while they're loaded,
    /// if they're watched on their own, rescanning it after each change
    /// under it.
    fn watch_workspace(this: &Rc<RefCell<Self>>, cx: &mut App) {
        let mut source = this.borrow_mut();
        let Some(workspace) = source.workspace.as_mut().filter(|ws| ws.watch) else {
            return;
        };
        let Some(dir) = workspace.loaded_dir().map(Path::to_owned) else {
            return;
        };
        let (sender, mut batches) = batch_channel();
        let watcher = match TreeWatcher::new(vec![dir.clone()], move |batch| sender.send(batch)) {
            Ok(watcher) => watcher,
            Err(error) => {
                log::warn!(
                    target: log_target(cx),
                    "{} won't reload when changed: {error}",
                    dir.display()
                );
                return;
            }
        };
        let weak = Rc::downgrade(this);
        let task = cx.spawn(async move |cx| {
            while let Some(batch) = batches.next().await {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                cx.update(|cx| {
                    let previous = this.borrow().diagnostics();
                    Self::reload(&this, &batch, cx);
                    let diagnostics = this.borrow().diagnostics();
                    replace_diagnostics(cx, &previous, diagnostics);
                });
            }
        });
        workspace.watching = Some((watcher, task));
    }

    /// Rescans the user's extensions directory if `changed` names it, and
    /// the workspace's if it names that and they're loaded, then applies
    /// what changed.
    fn reload(this: &Rc<RefCell<Self>>, changed: &[PathBuf], cx: &mut App) {
        let mut source = this.borrow_mut();
        let user = changed.contains(&source.dir);
        let workspace = (source.workspace.as_mut())
            .filter(|ws| {
                ws.loaded_dir()
                    .is_some_and(|dir| changed.iter().any(|c| c == dir))
            })
            .and_then(|ws| ws.set.as_mut());
        let Some(mut rescanned) = workspace
            .map(ExtensionSet::rescan)
            .or(user.then_some(false))
        else {
            return;
        };
        if user {
            rescanned |= source.set.rescan();
            // A theme file may change with no manifest changing.
            if source.read_themes() {
                log::info!(target: log_target(cx), "extension themes reloaded");
                source.apply_themes(cx);
            }
            // So may a keymap or menu file.
            if source.read_files() {
                log::info!(target: log_target(cx), "extension keymaps and menus reloaded");
                crate::contributions::resolve_again(cx);
            }
        }
        // A file the app reads from the manifest, as a grammar's module,
        // may change with no manifest changing. The contents are hashed off
        // the main thread, and the extensions applied when that's done, if
        // anything differs.
        if source.files_of.is_none() {
            if rescanned {
                log::info!(target: log_target(cx), "extensions reloaded");
                source.apply(cx);
            }
            return;
        }
        let stamps = source.fingerprint();
        source.dirty |= rescanned || stamps != source.fingerprints;
        drop(source);
        Self::hash(this, stamps, cx);
    }

    /// Hashes the contents of the files in `stamps` on a background thread,
    /// then applies the extensions, with `stamps` as their fingerprint, if
    /// they were loaded or a reload found a change in the manifests or the
    /// files' sizes and times (`dirty`), or the contents differ from the
    /// last applied's. Applying after the hashing, not before, means the
    /// hashes can't be of bytes the app never saw, and a rewrite after it
    /// is a change the next reload sees. Hashes for a check since replaced
    /// are dropped; `dirty` stays until the extensions are applied.
    fn hash(this: &Rc<RefCell<Self>>, stamps: Fingerprints, cx: &mut App) {
        let (task, generation) = {
            let mut source = this.borrow_mut();
            source.generation += 1;
            let generation = source.generation;
            let task = cx.background_executor().spawn({
                let stamps = stamps.clone();
                async move { hash_files(&stamps) }
            });
            (task, generation)
        };
        let this = this.clone();
        cx.spawn(async move |cx| {
            let hashes = task.await;
            cx.update(|cx| {
                let mut source = this.borrow_mut();
                if source.generation != generation {
                    return;
                }
                let differs = source.hashes.as_ref() != Some(&hashes);
                if !(source.dirty || differs) {
                    return;
                }
                log::info!(target: log_target(cx), "extensions reloaded");
                source.apply(cx);
                // What `apply` fingerprinted may be newer than what was
                // hashed: keep the older, so a change between is seen.
                source.fingerprints = stamps.clone();
                source.hashes = Some(hashes);
            });
        })
        .detach();
    }
}

/// The source as the config service holds it, shared with the background
/// hashing that comes back to it.
pub(crate) struct Shared(pub(crate) Rc<RefCell<ExtensionSource>>);

impl ConfigSource for Shared {
    fn paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn trees(&self) -> Vec<PathBuf> {
        vec![self.0.borrow().dir.clone()]
    }

    fn install(&mut self, cx: &mut App) {
        let stamps = {
            let mut source = self.0.borrow_mut();
            source.installed = true;
            source.apply_themes(cx);
            if source.files_of.is_none() {
                source.apply(cx);
                return;
            }
            // The app is told once the files are hashed, so the hashes can't
            // be of bytes it never saw: a rewrite after it is a change.
            publish_loaded(&source.joined(), cx);
            source.dirty = true;
            source.fingerprint()
        };
        ExtensionSource::hash(&self.0, stamps, cx);
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        ExtensionSource::reload(&self.0, changed, cx);
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.0.borrow().diagnostics()
    }
}

/// Publishes `joined`'s extensions as [`loaded_from`]'s.
fn publish_loaded(joined: &ExtensionSet, cx: &mut App) {
    let loaded = joined.sourced();
    let loaded = loaded.map(|(source, manifest)| (source, manifest.clone()));
    cx.set_global(Loaded(loaded.collect()));
}

/// Logs `diagnostics` and puts them in place of `previous` in
/// [`ConfigDiagnostics`], if they differ: for a change between the config
/// service's batches, which publish them with the rest.
fn replace_diagnostics(cx: &mut App, previous: &[Diagnostic], diagnostics: Vec<Diagnostic>) {
    if previous == diagnostics {
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
    let canonical = match kiln_core::platform::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(unread(false, "no such file".into()));
        }
        Err(error) => return Err(unread(true, format!("can't be read: {error}"))),
    };
    let inside = (kiln_core::platform::canonicalize(&manifest.dir))
        .is_ok_and(|dir| canonical.starts_with(dir));
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
    /// Each file's text as last laid in without errors on each platform:
    /// a text can parse on one and not another (`secondary-ctrl-k` holds
    /// ctrl twice off macOS), and the keymap is resolved for each.
    last_good: HashMap<(Os, PathBuf), String>,
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

/// `file` parsed with `parse` as on `os`: its text's layer, with the
/// text, and diagnostics, which with no errors becomes its last good
/// version there; else, or if it can't be read, its last good version's
/// layer there, with no text, as what's dropped from it was reported when
/// it was read.
fn parse_kept<'a, T>(
    file: &'a File,
    os: Os,
    last_good: &mut HashMap<(Os, PathBuf), String>,
    parse: impl Fn(&str) -> (Option<T>, Vec<Diagnostic>),
) -> (Option<(T, Option<&'a str>)>, Vec<Diagnostic>) {
    let key = (os, file.path.clone());
    let kept = |last_good: &HashMap<(Os, PathBuf), String>| {
        let text = last_good.get(&key)?;
        Some((parse(text).0?, None))
    };
    let Some(text) = &file.text else {
        return (kept(last_good), Vec::new());
    };
    match parse(text) {
        (Some(layer), found) => {
            last_good.insert(key.clone(), text.clone());
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
        let (layer, found) = parse_kept(file, os, &mut files.last_good, |text| {
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
        let (layer, found) = parse_kept(file, os, &mut files.last_good, |text| {
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
    if publish {
        replace_diagnostics(cx, &previous, diagnostics);
    }
}
