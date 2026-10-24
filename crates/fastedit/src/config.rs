//! What [`kiln::init`] sets up for fastedit: [`shipped`] is everything the
//! app owns, [`config`] adds the user's files.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use fe_config::settings::LspServerSettings;
use fe_config::{Settings, SettingsStore};
use fe_gpui::fe_keymap;
use gpui::{App, AppContext as _};
use kiln::{Config, GlobalSettings, core::ApiVersion};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::theme;

/// The editor's, vim's and the workbench's commands, with vim as the key
/// owner ([`fe_gpui::init_with_keymap`]).
pub(crate) fn register_commands(cx: &mut App) {
    // The keymap comes after, once every command is registered.
    fe_gpui::init_with_keymap(cx, fe_keymap::Keymap::default());
    fe_workbench::init(cx);
}

/// What the app owns of its [`Config`], without the user's files:
///
/// - [`register_commands`];
/// - the built-in keymap and menus, checked against those commands
///   ([`fe_workbench::command_catalog`]), the menus' shortcuts resolved
///   with an editor focused in the workbench;
/// - `settings`, and the keymap's issues logged after a reload;
/// - kiln's dark and light themes in the settings' `theme.mode`, with
///   fastedit's tables ([`fe_config::theme::EXTENSIONS`]) in the user's
///   `theme.toml`.
///
/// Diagnostics and reloads are logged as fastedit's own.
pub(crate) fn shipped(settings: SettingsStore) -> Config {
    let app = fe_config::paths::app_id();
    let mode = theme::mode(settings.settings().theme.mode);
    Config::new(app.reverse_dns, app.display)
        .log_target("fastedit")
        .commands(register_commands)
        .catalog(fe_workbench::command_catalog())
        .keymap(fe_keymap::DEFAULT_KEYMAP_PATH, fe_keymap::DEFAULT)
        .on_keymap_reload(|cx| log_keymap_report(cx))
        .menus(fe_keymap::menu::DEFAULT_MENUS_PATH, fe_keymap::MENUS)
        .menu_shortcut_context(fe_keymap::MENU_SHORTCUT_CONTEXT)
        .settings_store(settings.into_inner())
        .theme_mode(mode)
        .theme_extensions(fe_config::theme::EXTENSIONS)
}

/// [`shipped`] with the user's `theme.toml` and `keymap.toml` (in the XDG
/// layout, as they've always been), each watched and reloaded when it
/// changes, `settings` from the user's file and the project's (which may
/// only turn language servers off), and the user's extensions
/// ([`with_extensions`]).
pub(crate) fn config(settings: SettingsStore) -> Config {
    let config = shipped(settings);
    let config = match fe_config::paths::app_paths().extensions_dir() {
        Some(dir) => with_extensions(config, dir),
        None => config,
    };
    let config = match fe_config::paths::user_theme_path() {
        Some(path) => config.user_theme(path),
        None => config,
    };
    match fe_keymap::user_keymap_path() {
        Some(path) => config.user_keymap(path),
        None => config,
    }
}

/// `config` with the user's extensions, a directory each under `dir`,
/// loaded and reloaded by kiln: their manifests name
/// `[engine] fastedit = "1.0"`, or `"1.1"` for snippets. Their themes are chosen by the settings'
/// `theme.dark` and `theme.light` ([`theme::init`]). Their language
/// servers, `[contributes.lsp.<name>]` tables of the keys
/// `[lsp.servers.<name>]` takes, go to every workbench
/// ([`fe_workbench::set_extension_servers`]) once the user consents to
/// each, under the user's `[lsp]` settings; a change to a file one's
/// arguments name loads them again, so the change is asked about. Their
/// tree-sitter grammars, `[[contributes.grammars]]`
/// ([`ExtensionGrammar`]), are registered with fe-syntax on a background
/// thread, for files opened after. Their snippets,
/// `[[contributes.snippets]]` ([`ExtensionSnippet`]), are offered as
/// completions in every workbench ([`fe_workbench::set_extension_snippets`]).
/// A trusted launch root's own extensions come after the user's, with
/// [`WORKSPACE_KINDS`] only: their servers' consent is keyed by their own
/// directories, so a grant to a user's server doesn't cover them. Those
/// the settings' `extensions.disabled` names contribute none of these
/// ([`DisabledExtensions`]).
pub(crate) fn with_extensions(config: Config, dir: PathBuf) -> Config {
    // 1.1 added snippets.
    let api = ApiVersion { major: 1, minor: 1 };
    let lsp = schemars::schema_for!(BTreeMap<String, LspServerSettings>).to_value();
    let grammars = schemars::schema_for!(Vec<ExtensionGrammar>).to_value();
    let snippets = schemars::schema_for!(Vec<ExtensionSnippet>).to_value();
    let mut last_good = LastGoodServers::new();
    config
        .extensions(dir, &fe_config::paths::app_id().short, api)
        .extension_kind("lsp", lsp)
        .extension_kind("grammars", grammars)
        .extension_kind("snippets", snippets)
        .workspace_extensions(WORKSPACE_KINDS)
        .extension_files(|manifest| {
            let mut files = grammar_files(manifest);
            files.extend(server_files(manifest));
            files
        })
        .on_extensions(move |set, cx| {
            let servers = extension_servers(set.extensions(), &mut last_good);
            fe_workbench::set_extension_servers(cx, servers);
            fe_workbench::set_extension_snippets(cx, extension_snippets(set.extensions()));
            let grammars = extension_grammars(set.extensions());
            // Newer than any batch still registering, which then stops. An
            // empty one still withdraws what extensions registered before.
            let batch = GRAMMAR_BATCH.fetch_add(1, Ordering::SeqCst) + 1;
            cx.background_spawn(async move { register_grammars(batch, grammars) })
                .detach();
        })
        .source(DisabledExtensions)
}

/// Has kiln disable the extensions the settings' `extensions.disabled`
/// names ([`kiln::extensions::set_disabled`]), from the config service's
/// start and whenever a reload changes it, as [`theme::init`] follows
/// `theme.dark`. Disabled, an extension's language servers stop, their
/// consent kept, its grammars aren't offered for files opened after and
/// its snippets aren't offered; enabled again, they're back.
struct DisabledExtensions;

impl kiln::platform::config::ConfigSource for DisabledExtensions {
    fn paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Before the extensions' own, as kiln installs the app's sources
    /// first: those disabled never contribute, nor reach the app.
    fn install(&mut self, cx: &mut App) {
        if let Some(global) = cx.try_global::<GlobalSettings<Settings>>() {
            let disabled = global.settings().extensions.disabled.clone();
            kiln::extensions::set_disabled(cx, disabled);
        }
        cx.observe_global::<GlobalSettings<Settings>>(|cx| {
            let global = cx.global::<GlobalSettings<Settings>>();
            let (now, before) = (&global.settings().extensions, &global.previous().extensions);
            if now.disabled != before.disabled {
                let disabled = now.disabled.clone();
                kiln::extensions::set_disabled(cx, disabled);
            }
        })
        .detach();
    }

    fn reload(&mut self, _: &[PathBuf], _: &mut App) {}
}

/// The contribution kinds a trusted folder's own extensions may have
/// (extensions record §3): never themes, keymaps or menus.
pub(crate) const WORKSPACE_KINDS: [&str; 3] = ["grammars", "lsp", "snippets"];

/// One of an extension's `[[contributes.grammars]]`: a tree-sitter grammar
/// compiled to WebAssembly (`tree-sitter build --wasm`) and its queries,
/// each path relative to the extension's directory. See
/// [`fe_syntax::GrammarSpec`] for what each key means.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExtensionGrammar {
    /// The language's name; a built-in language's is refused.
    #[schemars(length(min = 1))]
    language: String,
    /// The module, named `tree-sitter-<grammar>.wasm` as the tree-sitter CLI
    /// names it.
    wasm: String,
    /// File extensions, without the dot, it's chosen for.
    #[serde(default)]
    extensions: Vec<String>,
    /// Whole file names it's chosen for.
    #[serde(default)]
    file_names: Vec<String>,
    highlights: Option<String>,
    injections: Option<String>,
    /// Whether the first of several patterns capturing a node wins, as in
    /// tree-sitter's own highlighter, rather than the last.
    #[serde(default)]
    earlier_patterns_win: bool,
    /// The grammar's SPDX licence, when it isn't the extension's.
    license: Option<String>,
}

/// One of an extension's `[[contributes.snippets]]`, in the LSP snippet
/// grammar, as VS Code's snippet files have them.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExtensionSnippet {
    /// What's typed to offer it; an empty one drops it.
    prefix: String,
    /// What it inserts; an empty one drops it.
    body: SnippetBody,
    /// Shown beside its prefix.
    description: Option<String>,
    /// fastedit's names of the languages it's offered in: every one if
    /// none.
    #[serde(default)]
    languages: Vec<String>,
}

/// A snippet's body: its text, or its lines.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
enum SnippetBody {
    Text(String),
    Lines(Vec<String>),
}

/// The snippets `extensions` contribute, with their extension's ID and
/// name: one with an empty prefix or body, or a body that doesn't parse
/// ([`fe_workbench::snippet_body_error`]), is logged and skipped.
fn extension_snippets(extensions: &[kiln::core::Manifest]) -> Vec<fe_workbench::ExtensionSnippet> {
    let mut snippets = Vec::new();
    for manifest in extensions {
        let Some(table) = manifest.app.get("snippets") else {
            continue;
        };
        // The schema has checked it.
        let table: Vec<ExtensionSnippet> = match table.clone().try_into() {
            Ok(table) => table,
            Err(error) => {
                log::warn!("extension {}'s snippets: {error}", manifest.id);
                continue;
            }
        };
        for snippet in table {
            let body = match snippet.body {
                SnippetBody::Text(text) => text,
                SnippetBody::Lines(lines) => lines.join("\n"),
            };
            if snippet.prefix.is_empty() || body.is_empty() {
                log::warn!(
                    "extension {}'s snippet {:?} has an empty {}, so it's skipped",
                    manifest.id,
                    snippet.prefix,
                    if body.is_empty() { "body" } else { "prefix" }
                );
                continue;
            }
            if let Some(error) = fe_workbench::snippet_body_error(&body) {
                log::warn!(
                    "extension {}'s snippet {:?} is skipped: {error}",
                    manifest.id,
                    snippet.prefix
                );
                continue;
            }
            snippets.push(fe_workbench::ExtensionSnippet {
                extension_id: manifest.id.clone(),
                extension_name: manifest.name.clone(),
                prefix: snippet.prefix,
                body,
                description: snippet.description,
                languages: snippet.languages,
            });
        }
    }
    snippets
}

/// A grammar an extension contributes: its extension's ID and directory,
/// and the grammar, its paths resolved.
struct Grammar {
    id: String,
    dir: PathBuf,
    spec: fe_syntax::GrammarSpec,
}

/// The grammars `extensions` contribute: a grammar naming a file outside
/// its extension is logged and skipped.
fn extension_grammars(extensions: &[kiln::core::Manifest]) -> Vec<Grammar> {
    let mut specs = Vec::new();
    for manifest in extensions {
        let Some(grammars) = manifest.app.get("grammars") else {
            continue;
        };
        // The schema has checked it.
        let grammars: Vec<ExtensionGrammar> = match grammars.clone().try_into() {
            Ok(grammars) => grammars,
            Err(error) => {
                log::warn!("extension {}'s grammars: {error}", manifest.id);
                continue;
            }
        };
        for grammar in grammars {
            let file = |entry: &str| extension_file(&manifest.dir, entry);
            let spec = (|| {
                Ok::<_, String>(fe_syntax::GrammarSpec {
                    wasm: file(&grammar.wasm)?,
                    highlights: grammar.highlights.as_deref().map(file).transpose()?,
                    injections: grammar.injections.as_deref().map(file).transpose()?,
                    language: grammar.language.clone(),
                    extensions: grammar.extensions.clone(),
                    file_names: grammar.file_names.clone(),
                    earlier_patterns_win: grammar.earlier_patterns_win,
                })
            })();
            match spec {
                Ok(spec) => {
                    let license = grammar.license.as_ref().unwrap_or(&manifest.license.expr);
                    log::info!(
                        "extension {}'s grammar {} ({license})",
                        manifest.id,
                        grammar.language
                    );
                    specs.push(Grammar {
                        id: manifest.id.clone(),
                        dir: manifest.dir.clone(),
                        spec,
                    });
                }
                Err(error) => log::warn!(
                    "extension {}'s grammar {}: {error}",
                    manifest.id,
                    grammar.language
                ),
            }
        }
    }
    specs
}

/// The files `manifest`'s grammars name, so that a change to one alone
/// registers them again.
fn grammar_files(manifest: &kiln::core::Manifest) -> Vec<PathBuf> {
    let Some(grammars) = manifest.app.get("grammars") else {
        return Vec::new();
    };
    let Ok(grammars) = grammars.clone().try_into::<Vec<ExtensionGrammar>>() else {
        return Vec::new();
    };
    grammars
        .into_iter()
        .flat_map(|grammar| [Some(grammar.wasm), grammar.highlights, grammar.injections])
        .flatten()
        .map(PathBuf::from)
        .collect()
}

/// `entry` in extension `dir`'s manifest as the file it names, which must be
/// inside `dir` with symlinks followed, as kiln checks its own contributed
/// files.
fn extension_file(dir: &Path, entry: &str) -> Result<PathBuf, String> {
    let relative = Path::new(entry);
    if relative.has_root() || relative.is_absolute() {
        return Err(format!(
            "{entry:?} must be relative to the extension's directory"
        ));
    }
    let root = dir
        .canonicalize()
        .map_err(|error| format!("{}: {error}", dir.display()))?;
    let file = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("{entry:?}: {error}"))?;
    if !file.starts_with(&root) {
        return Err(format!("{entry:?} is outside the extension's directory"));
    }
    Ok(file)
}

/// The last batch of grammars [`with_extensions`] started registering.
static GRAMMAR_BATCH: AtomicU64 = AtomicU64::new(0);

/// Withdraws each language an extension's grammar registered unless that
/// extension, by its directory, still has a grammar of that name in `batch`
/// ([`fe_syntax::unregister_language`]): one of a workspace folder no
/// longer trusted goes, even if another extension names the same language.
/// Then registers each grammar of `batch` ([`fe_syntax::register_language`]),
/// logging those refused with their extension's ID: one refused keeps the
/// language its own extension registered before, if it did, and no other's.
/// Batches run one at a time, and one stops once a newer one starts, which
/// then withdraws what the stopped one registered and shouldn't have: the
/// extensions' latest grammars are the ones kept.
fn register_grammars(batch: u64, grammars: Vec<Grammar>) {
    /// Each language extensions' grammars registered, by name, with the
    /// directory of the extension it's from.
    static REGISTERED: Mutex<BTreeMap<String, PathBuf>> = Mutex::new(BTreeMap::new());
    static REGISTERING: Mutex<()> = Mutex::new(());
    let _registering = REGISTERING.lock().unwrap_or_else(PoisonError::into_inner);
    let stale = || GRAMMAR_BATCH.load(Ordering::SeqCst) != batch;
    if stale() {
        return;
    }
    let mut registered = REGISTERED.lock().unwrap_or_else(PoisonError::into_inner);
    let listed: BTreeSet<(&str, &Path)> = (grammars.iter())
        .map(|grammar| (grammar.spec.language.as_str(), grammar.dir.as_path()))
        .collect();
    registered.retain(|name, dir| {
        if listed.contains(&(name.as_str(), dir.as_path())) {
            return true;
        }
        log::info!("extension grammar {name} from {} withdrawn", dir.display());
        fe_syntax::unregister_language(name);
        false
    });
    for Grammar { id, dir, spec } in grammars {
        if stale() {
            return;
        }
        match fe_syntax::register_language(&spec) {
            Ok(_) => {
                registered.insert(spec.language.clone(), dir);
            }
            Err(error) => log::warn!("extension {id}'s grammar {}: {error}", spec.language),
        }
    }
}

/// Each extension's language servers' settings, by extension ID then
/// server name, as last read well.
type LastGoodServers = HashMap<String, BTreeMap<String, LspServerSettings>>;

/// The language servers `extensions` contribute, in their order: each
/// enabled table with a `command`, as written ([`fe_lsp::extension_spec`]),
/// with its extension. A table that doesn't read as settings keeps its
/// settings from `last_good`, if it had any, so a bad edit doesn't stop a
/// server that worked; `last_good` becomes what was read.
fn extension_servers(
    extensions: &[kiln::core::Manifest],
    last_good: &mut LastGoodServers,
) -> Vec<fe_workbench::ExtensionServer> {
    let mut servers = Vec::new();
    let mut read = LastGoodServers::new();
    for manifest in extensions {
        let Some(lsp) = manifest.app.get("lsp") else {
            continue;
        };
        let settings = server_settings(manifest, lsp, last_good.get(&manifest.id));
        for (name, settings) in settings.iter().filter(|(_, settings)| settings.enabled) {
            match fe_lsp::extension_spec(name, settings) {
                Some(spec) => servers.push(fe_workbench::ExtensionServer {
                    extension_id: manifest.id.clone(),
                    extension_name: manifest.name.clone(),
                    extension_version: {
                        let (major, minor, patch) = manifest.version;
                        format!("{major}.{minor}.{patch}")
                    },
                    extension_license: manifest.license.expr.clone(),
                    extension_dir: manifest.dir.clone(),
                    spec,
                }),
                None => log::warn!(
                    "extension {}'s language server {name} has no command, so it never starts",
                    manifest.id
                ),
            }
        }
        read.insert(manifest.id.clone(), settings);
    }
    *last_good = read;
    servers
}

/// `manifest`'s language servers' settings from its `lsp` table, each
/// server's read on its own: one that doesn't read is logged, and keeps
/// its settings in `previous` if it has some there.
fn server_settings(
    manifest: &kiln::core::Manifest,
    lsp: &toml::Value,
    previous: Option<&BTreeMap<String, LspServerSettings>>,
) -> BTreeMap<String, LspServerSettings> {
    let kept = |name: &str| previous.and_then(|previous| previous.get(name)).cloned();
    // The schema has checked it's a table.
    let Some(table) = lsp.as_table() else {
        log::warn!(
            "extension {}'s language servers aren't a table",
            manifest.id
        );
        return previous.cloned().unwrap_or_default();
    };
    table
        .iter()
        .filter_map(
            |(name, value)| match value.clone().try_into::<LspServerSettings>() {
                Ok(settings) => Some((name.clone(), settings)),
                Err(error) => {
                    let kept = kept(name);
                    log::warn!(
                        "extension {}'s language server {name}: {error}{}",
                        manifest.id,
                        if kept.is_some() {
                            "; it keeps its last good settings"
                        } else {
                            ""
                        }
                    );
                    kept.map(|settings| (name.clone(), settings))
                }
            },
        )
        .collect()
}

/// The files `manifest`'s language servers may run: each argument, and
/// each `--flag=value`'s value, and each command, that names a path. kiln
/// keeps those that are files inside the extension's directory, so that a
/// change to a script loads the servers again, and its consent is asked
/// for again.
fn server_files(manifest: &kiln::core::Manifest) -> Vec<PathBuf> {
    let Some(table) = manifest.app.get("lsp").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for value in table.values() {
        let Ok(settings) = value.clone().try_into::<LspServerSettings>() else {
            continue;
        };
        files.extend(settings.command.iter().map(PathBuf::from));
        for arg in settings.args.iter().flatten() {
            if let Some((_, value)) = arg.strip_prefix('-').and_then(|flag| flag.split_once('=')) {
                files.push(PathBuf::from(value));
            }
            files.push(PathBuf::from(arg));
        }
    }
    files.retain(|file| !file.as_os_str().is_empty());
    files
}

/// Logs what [`fe_gpui::keymap_report`] finds in the keymap in use.
pub(crate) fn log_keymap_report(cx: &App) {
    for issue in fe_gpui::keymap_report(cx) {
        log::info!("keymap: {issue}");
    }
}
