//! What [`kiln::init`] sets up for fastedit: [`shipped`] is everything the
//! app owns, [`config`] adds the user's files.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fe_config::SettingsStore;
use fe_config::settings::LspServerSettings;
use fe_gpui::fe_keymap;
use gpui::App;
use kiln::{Config, core::ApiVersion};

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
/// settings.
pub(crate) fn with_extensions(config: Config, dir: PathBuf) -> Config {
    let api = ApiVersion { major: 1, minor: 0 };
    let lsp = schemars::schema_for!(BTreeMap<String, LspServerSettings>).to_value();
    config
        .extensions(dir, &fe_config::paths::app_id().short, api)
        .extension_kind("lsp", lsp)
        .on_extensions(|set, cx| {
            fe_workbench::set_extension_servers(cx, extension_servers(set.extensions()));
        })
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
