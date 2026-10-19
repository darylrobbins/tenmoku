//! What [`kiln::init`] sets up for fastedit: [`shipped`] is everything the
//! app owns, [`config`] adds the user's files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use fe_config::SettingsStore;
use fe_config::settings::LspServerSettings;
use fe_gpui::fe_keymap;
use gpui::{App, AppContext as _};
use kiln::{Config, core::ApiVersion};
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
/// `[engine] fastedit = "1.0"`. Their themes are chosen by the settings'
/// `theme.dark` and `theme.light` ([`theme::init`]). Their language
/// servers, `[contributes.lsp.<name>]` tables of the keys
/// `[lsp.servers.<name>]` takes, go to every workbench
/// ([`fe_workbench::set_extension_servers`]), under the user's `[lsp]`
/// settings. Their tree-sitter grammars, `[[contributes.grammars]]`
/// ([`ExtensionGrammar`]), are registered with fe-syntax on a background
/// thread, for files opened after.
pub(crate) fn with_extensions(config: Config, dir: PathBuf) -> Config {
    let api = ApiVersion { major: 1, minor: 0 };
    let lsp = schemars::schema_for!(BTreeMap<String, LspServerSettings>).to_value();
    let grammars = schemars::schema_for!(Vec<ExtensionGrammar>).to_value();
    config
        .extensions(dir, &fe_config::paths::app_id().short, api)
        .extension_kind("lsp", lsp)
        .extension_kind("grammars", grammars)
        .on_extensions(|set, cx| {
            fe_workbench::set_extension_servers(cx, extension_servers(set.extensions()));
            let grammars = extension_grammars(set.extensions());
            // Newer than any batch still registering, which then stops.
            let batch = GRAMMAR_BATCH.fetch_add(1, Ordering::SeqCst) + 1;
            if !grammars.is_empty() {
                cx.background_spawn(async move { register_grammars(batch, grammars) })
                    .detach();
            }
        })
}

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

/// The grammars `extensions` contribute, with their extension's ID, their
/// paths resolved: a grammar naming a file outside its extension is logged
/// and skipped.
fn extension_grammars(
    extensions: &[kiln::core::Manifest],
) -> Vec<(String, fe_syntax::GrammarSpec)> {
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
                    specs.push((manifest.id.clone(), spec));
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

/// Registers each grammar of `batch` ([`fe_syntax::register_language`]),
/// logging those refused with their extension's ID. Batches run one at a
/// time, and one stops once a newer one starts, so the extensions' latest
/// grammars are the ones kept.
fn register_grammars(batch: u64, grammars: Vec<(String, fe_syntax::GrammarSpec)>) {
    static REGISTERING: Mutex<()> = Mutex::new(());
    let _registering = REGISTERING.lock().unwrap_or_else(PoisonError::into_inner);
    for (id, spec) in grammars {
        if GRAMMAR_BATCH.load(Ordering::SeqCst) != batch {
            return;
        }
        if let Err(error) = fe_syntax::register_language(&spec) {
            log::warn!("extension {id}'s grammar {}: {error}", spec.language);
        }
    }
}

/// The language servers `extensions` contribute, in their order: each
/// enabled table with a `command`, as written ([`fe_lsp::extension_spec`]).
fn extension_servers(extensions: &[kiln::core::Manifest]) -> Vec<fe_lsp::ServerSpec> {
    let mut specs = Vec::new();
    for manifest in extensions {
        let Some(lsp) = manifest.app.get("lsp") else {
            continue;
        };
        // The schema has checked it.
        let servers: BTreeMap<String, LspServerSettings> = match lsp.clone().try_into() {
            Ok(servers) => servers,
            Err(error) => {
                log::warn!("extension {}'s language servers: {error}", manifest.id);
                continue;
            }
        };
        for (name, settings) in servers.iter().filter(|(_, settings)| settings.enabled) {
            match fe_lsp::extension_spec(name, settings) {
                Some(spec) => specs.push(spec),
                None => log::warn!(
                    "extension {}'s language server {name} has no command, so it never starts",
                    manifest.id
                ),
            }
        }
    }
    specs
}

/// Logs what [`fe_gpui::keymap_report`] finds in the keymap in use.
pub(crate) fn log_keymap_report(cx: &App) {
    for issue in fe_gpui::keymap_report(cx) {
        log::info!("keymap: {issue}");
    }
}
