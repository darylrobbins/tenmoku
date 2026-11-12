//! The code plugins in Extensions: Show Installed (extensions record §7),
//! and removing them.
//!
//! - **Rows.** A plugin's row ([`PluginRow`], from [`plugin_states`]) has
//!   what it asks for and what it was granted, each capability in plain
//!   words ([`describe`]), a Full access marker when the grant holds a
//!   wildcard ([`is_full_access`]), and its state: Running, Waiting for
//!   grant, Declined, Stopped with its diagnostic, or Needs plugins preview,
//!   naming [`PREVIEW_SETTING`]. One the settings disable is listed too, as
//!   its manifest has it, to revoke or uninstall.
//! - **Revoke.** Extensions: Revoke Plugin Grants holds it in the host
//!   ([`hold_plugin`]): it unloads and nothing of it is asked or recorded.
//!   Once that's settled its grant record is forgotten ([`revoke`]): it
//!   stays stopped, and asks again the next time it loads.
//! - **Uninstall.** Extensions: Uninstall Plugin asks first, saying its
//!   stored secrets are deleted ([`confirm_uninstall`]). Confirmed, it's
//!   held likewise; once settled, if its directory still holds it, its
//!   secrets are deleted ([`PluginSecrets::delete_all`]), its grant record
//!   forgotten and its extension directory removed: a symlink's link, never
//!   what it points at. One whose secrets can't be deleted stays installed,
//!   grants and all, and is as it was before, running again if it ran; the
//!   toast says why. Only one revoke or uninstall of a plugin runs at once.
//!
//! The grant records and the secrets' index are in the state directory, as
//! the plugin host keeps them; the secrets in the platform's keychain,
//! opened only when a plugin has secrets to delete.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_command::capability::Grants;
use futures::channel::oneshot;
use gpui::{App, AppContext as _, Global, PromptLevel, SharedString, Subscription, Window};
use kiln::core::{
    extension::{MANIFEST_FILE, Manifest},
    secrets::{KeychainStore, Secret, SecretError, SecretStore},
};
use kiln_l10n::tr;
use kiln_plugin::{
    bindings::Error,
    grants::{GrantStore, Version},
    manifest::{ExtensionOrigin, PluginManifest},
    secrets::PluginSecrets,
    warnings::{describe, is_full_access},
};

use crate::{
    GRANTS_FILE, PluginInfo, PluginState, SECRETS_INDEX_FILE, Workbench,
    extensions_picker::ExtensionState,
    plugin_host::{PluginHost, Release, hold_plugin, plugin_settled, release_plugin},
    plugin_states,
};

/// The setting the plugins load under.
pub(crate) const PREVIEW_SETTING: &str = "extensions.plugins_preview";

/// The uninstall confirmation's buttons, and the index of Uninstall: an
/// answer is matched by its button's index, never its title.
fn uninstall_buttons() -> [SharedString; 2] {
    [
        tr!("workbench-extensions-plugin-uninstall"),
        tr!("workbench-extensions-cancel"),
    ]
}
const UNINSTALL: usize = 0;

/// A plugin's part of its extension's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: Version,
    /// The extension's directory.
    pub(crate) dir: PathBuf,
    /// One of the plugin states.
    pub(crate) state: ExtensionState,
    /// Whether what it was granted holds `net:*` or `process.spawn:*`.
    pub(crate) full_access: bool,
    /// Each capability it asks for, with what it lets it do.
    pub(crate) requested: Vec<String>,
    /// Each capability it was granted, likewise.
    pub(crate) granted: Vec<String>,
    /// Why it doesn't run, if it doesn't for a reason of its own: its
    /// diagnostic, or the setting it needs.
    pub(crate) reason: Option<String>,
}

impl PluginRow {
    /// `info`'s row.
    pub(crate) fn of(info: &PluginInfo) -> Self {
        let (state, reason) = match &info.state {
            PluginState::Running => (ExtensionState::Running, None),
            PluginState::WaitingForGrant => (ExtensionState::WaitingForGrant, None),
            PluginState::Declined => (ExtensionState::PluginDeclined, None),
            PluginState::Stopped(diagnostic) => (
                ExtensionState::Stopped,
                Some(
                    tr!(
                        "workbench-extensions-plugin-stopped",
                        diagnostic = diagnostic.to_string()
                    )
                    .into(),
                ),
            ),
            PluginState::PreviewOff => (ExtensionState::NeedsPluginsPreview, Some(needs_preview())),
        };
        Self {
            id: info.id.clone(),
            name: info.name.clone(),
            version: info.version,
            dir: info.dir.clone(),
            state,
            full_access: is_full_access(&info.granted),
            requested: capability_lines(&info.requested),
            granted: capability_lines(&info.granted),
            reason,
        }
    }

    /// The row of `manifest`'s plugin, which the host hasn't decided, in
    /// `state`: Needs plugins preview or Disabled. What it asks for is its
    /// manifest's; what it was granted isn't known.
    pub(crate) fn undecided(manifest: &Manifest, state: ExtensionState) -> Self {
        let plugin = PluginManifest::from_extension(manifest, ExtensionOrigin::User);
        let requested = (plugin.ok().flatten()).map(|plugin| plugin.capabilities);
        let reason = (state == ExtensionState::NeedsPluginsPreview).then(needs_preview);
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version,
            dir: manifest.dir.clone(),
            state,
            full_access: false,
            requested: requested.as_ref().map(capability_lines).unwrap_or_default(),
            granted: Vec::new(),
            reason,
        }
    }

    /// What the row's line says of it: the marker, what it asks for and
    /// what it has, then why it doesn't run.
    pub(crate) fn detail(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.full_access {
            parts.push(tr!("workbench-extensions-plugin-full-access").into());
        }
        if !self.requested.is_empty() {
            let asks = self.requested.join("; ");
            parts.push(tr!("workbench-extensions-plugin-row-asks", capabilities = asks).into());
        }
        if !self.granted.is_empty() {
            let granted = self.granted.join("; ");
            let granted = tr!(
                "workbench-extensions-plugin-row-granted",
                capabilities = granted
            );
            parts.push(granted.into());
        }
        parts.extend(self.reason.clone());
        parts
    }

    /// All it has to say, a line each, for the details Enter shows.
    pub(crate) fn full_detail(&self) -> Vec<String> {
        let state = self.state.label();
        let mut lines = vec![tr!("workbench-extensions-state", state = state).into()];
        if self.full_access {
            lines.push(tr!("workbench-extensions-plugin-full-access-detail").into());
        }
        let titles = [
            (
                tr!("workbench-extensions-plugin-row-asks-title"),
                &self.requested,
            ),
            (tr!("workbench-extensions-plugin-granted"), &self.granted),
        ];
        for (title, capabilities) in titles {
            lines.push(title.into());
            if capabilities.is_empty() {
                let nothing = tr!("workbench-extensions-plugin-nothing");
                lines.push(format!("  {}", nothing));
            }
            lines.extend(capabilities.iter().map(|line| format!("  {}", line)));
        }
        lines.extend(self.reason.clone());
        lines
    }
}

/// Why a plugin waits on the preview.
fn needs_preview() -> String {
    tr!(
        "workbench-extensions-plugin-needs-preview",
        setting = PREVIEW_SETTING
    )
    .into()
}

/// `grants`, each as `net:example.com: connect to example.com`.
fn capability_lines(grants: &Grants) -> Vec<String> {
    grants
        .iter()
        .map(|capability| {
            let capability_text = capability.to_string();
            let description = describe(capability);
            let line = tr!(
                "workbench-extensions-plugin-capability",
                capability = capability_text,
                description = description
            );
            line.into()
        })
        .collect()
}

/// Whether `extensions.plugins_preview` is on, as the host follows it.
pub(crate) fn preview_on(cx: &App) -> bool {
    crate::plugin_host::preview_on(cx)
}

/// The plugins' rows, in extension order.
pub(crate) fn plugin_rows(cx: &App) -> Vec<PluginRow> {
    let preview = preview_on(cx);
    (plugin_states(cx).iter())
        .map(|info| match PluginRow::of(info) {
            // Its first load: not decided yet.
            row if preview && row.state == ExtensionState::NeedsPluginsPreview => PluginRow {
                state: ExtensionState::PluginStarting,
                reason: None,
                ..row
            },
            row => row,
        })
        .collect()
}

/// Where the grant records and the secrets are.
struct Stores {
    state_dir: Option<PathBuf>,
    secrets: Arc<dyn SecretStore>,
}

impl Global for Stores {}

impl Stores {
    fn grants(&self) -> Option<GrantStore> {
        let dir = self.state_dir.as_ref()?;
        Some(GrantStore::new(dir.join(GRANTS_FILE)))
    }

    fn secrets(&self, id: &str) -> Option<PluginSecrets> {
        let dir = self.state_dir.as_ref()?;
        let service = fe_config::paths::app_id().reverse_dns;
        let index = dir.join(SECRETS_INDEX_FILE);
        Some(PluginSecrets::new(self.secrets.clone(), service, id, index))
    }
}

/// The app's: its state directory and the keychain; under test, none until
/// the test sets them ([`set_stores`]).
fn stores(cx: &mut App) -> &Stores {
    if !cx.has_global::<Stores>() {
        let state_dir = if cfg!(test) {
            None
        } else {
            fe_config::paths::state_dir()
        };
        cx.set_global(Stores {
            state_dir,
            secrets: Arc::new(Keychain),
        });
    }
    cx.global::<Stores>()
}

/// Has the grant records and the secrets' index be read from `state_dir`,
/// the secrets from `secrets`, as the test's plugin host keeps them.
#[cfg(test)]
pub(crate) fn set_stores(cx: &mut App, state_dir: PathBuf, secrets: Arc<dyn SecretStore>) {
    cx.set_global(Stores {
        state_dir: Some(state_dir),
        secrets,
    });
}

/// The platform keychain, opened on each use: only an uninstall reaches it,
/// and only for a plugin with secrets.
struct Keychain;

impl SecretStore for Keychain {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError> {
        KeychainStore::new()?.get(service, account)
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        KeychainStore::new()?.set(service, account, secret)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        KeychainStore::new()?.delete(service, account)
    }
}

/// Holds plugin `name` (`id`) and, once nothing of it runs or is being
/// recorded, forgets its grant record: it stays stopped, and asks again
/// the next time it loads.
pub(crate) fn revoke(id: &str, name: &str, cx: &mut App) {
    let Some(store) = stores(cx).grants() else {
        return notify_error(cx, cant_revoke(name), no_state_dir());
    };
    let Some(token) = hold_plugin(id, cx) else {
        return notify_error(cx, cant_revoke(name), busy());
    };
    let (settled, following) = settled(id, cx);
    let (id, name) = (id.to_owned(), name.to_owned());
    cx.spawn(async move |cx| {
        settled.await.ok();
        drop(following);
        let forgetting = id.clone();
        let forgot = cx
            .background_spawn(async move { store.forget(&forgetting) })
            .await;
        cx.update(|cx| {
            let release = if forgot.is_ok() {
                Release::Forgotten
            } else {
                Release::Restored
            };
            release_plugin(&id, token, release, cx);
            match forgot {
                Ok(_) => Workbench::notify(
                    cx,
                    log::Level::Info,
                    tr!("workbench-extensions-plugin-revoked", name = name.as_str()).into(),
                    Some(tr!("workbench-extensions-plugin-revoked-detail").into()),
                ),
                Err(error) => notify_error(cx, cant_revoke(&name), error.to_string()),
            }
        });
    })
    .detach();
}

/// The uninstall confirmation's question and detail for `plugin`.
pub(crate) fn uninstall_prompt_text(plugin: &PluginRow) -> (String, String) {
    let (major, minor, patch) = plugin.version;
    let question = tr!(
        "workbench-extensions-plugin-uninstall-question",
        name = plugin.name.as_str()
    );
    let detail = tr!(
        "workbench-extensions-plugin-uninstall-detail",
        id = plugin.id.as_str(),
        version = format!("{major}.{minor}.{patch}"),
        dir = plugin.dir.display().to_string()
    );
    (question.into(), detail.into())
}

/// Asks in `window` whether to uninstall `plugin`, and does it if so.
pub(crate) fn confirm_uninstall(plugin: &PluginRow, window: &mut Window, cx: &mut App) {
    let (question, detail) = uninstall_prompt_text(plugin);
    let buttons = uninstall_buttons();
    let answer = window.prompt(
        PromptLevel::Warning,
        &question,
        Some(&detail),
        &buttons.each_ref().map(|button| button.as_ref()),
        cx,
    );
    let plugin = plugin.clone();
    cx.spawn(async move |cx| {
        if answer.await == Ok(UNINSTALL) {
            cx.update(|cx| uninstall(plugin, cx));
        }
    })
    .detach();
}

/// Why an uninstall stopped.
enum Failure {
    /// Its directory no longer holds it: nothing was touched.
    Replaced(String),
    /// Its secrets weren't deleted: nothing else was touched.
    Secrets(Error),
    /// Its grant record wasn't forgotten, or its directory removed.
    Other(String),
}

/// Holds `plugin` and, once nothing of it runs or is being recorded, off
/// the main thread checks its directory still holds it, deletes its
/// secrets, forgets its grants and removes its directory. On a failure it
/// is as it was before.
fn uninstall(plugin: PluginRow, cx: &mut App) {
    let stores = stores(cx);
    let (Some(grants), Some(secrets)) = (stores.grants(), stores.secrets(&plugin.id)) else {
        return notify_error(cx, cant_uninstall(&plugin.name), no_state_dir());
    };
    let Some(token) = hold_plugin(&plugin.id, cx) else {
        return notify_error(cx, cant_uninstall(&plugin.name), busy());
    };
    let (settled, following) = settled(&plugin.id, cx);
    cx.spawn(async move |cx| {
        settled.await.ok();
        drop(following);
        let dir = plugin.dir.clone();
        let id = plugin.id.clone();
        let removed = cx
            .background_spawn(async move {
                check_still_installed(&dir, &id).map_err(Failure::Replaced)?;
                secrets.delete_all().map_err(Failure::Secrets)?;
                (grants.forget(&id)).map_err(|error| Failure::Other(error.to_string()))?;
                remove_extension_dir(&dir).map_err(|error| {
                    Failure::Other(
                        tr!(
                            "workbench-extensions-path-error",
                            path = dir.display().to_string(),
                            error = error.to_string()
                        )
                        .into(),
                    )
                })
            })
            .await;
        cx.update(|cx| {
            let name = &plugin.name;
            let failure = match removed {
                Ok(()) => {
                    release_plugin(&plugin.id, token, Release::Forgotten, cx);
                    let message = tr!("workbench-extensions-plugin-uninstalled", name = name);
                    return Workbench::notify(cx, log::Level::Info, message.into(), None);
                }
                Err(Failure::Secrets(error)) => tr!(
                    "workbench-extensions-plugin-secrets-kept",
                    error = error_text(&error)
                )
                .into(),
                Err(Failure::Replaced(message) | Failure::Other(message)) => message,
            };
            release_plugin(&plugin.id, token, Release::Restored, cx);
            notify_error(cx, cant_uninstall(name), failure);
        });
    })
    .detach();
}

/// Whether the extension in `dir` is still plugin `id`: its manifest names
/// it. If not, why not.
fn check_still_installed(dir: &Path, id: &str) -> Result<(), String> {
    let manifest = dir.join(MANIFEST_FILE);
    let text = fs::read_to_string(&manifest).map_err(|error| {
        tr!(
            "workbench-extensions-plugin-manifest-unreadable",
            path = manifest.display().to_string(),
            error = error.to_string()
        )
        .to_string()
    })?;
    let named = (text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("id")?.as_str().map(str::to_owned));
    if named.as_deref() == Some(id) {
        Ok(())
    } else {
        Err(tr!(
            "workbench-extensions-plugin-replaced",
            dir = dir.display().to_string()
        )
        .into())
    }
}

/// Removes the extension directory `dir`: the link only, if it's a symlink
/// or a junction (std reports both as symlinks).
fn remove_extension_dir(dir: &Path) -> io::Result<()> {
    if fs::symlink_metadata(dir)?.file_type().is_symlink() {
        // A directory symlink is a directory on Windows.
        fs::remove_file(dir).or_else(|_| fs::remove_dir(dir))
    } else {
        fs::remove_dir_all(dir)
    }
}

/// Resolves once nothing of plugin `id` runs, loads, unloads or is being
/// recorded ([`plugin_settled`]), while the subscription is held.
fn settled(id: &str, cx: &mut App) -> (oneshot::Receiver<()>, Option<Subscription>) {
    let (sender, receiver) = oneshot::channel();
    let id = id.to_owned();
    if plugin_settled(&id, cx) {
        sender.send(()).ok();
        return (receiver, None);
    }
    let mut sender = Some(sender);
    let following = cx.observe_global::<PluginHost>(move |cx| {
        if plugin_settled(&id, cx)
            && let Some(sender) = sender.take()
        {
            sender.send(()).ok();
        }
    });
    (receiver, Some(following))
}

fn error_text(error: &Error) -> &str {
    match error {
        Error::Unimplemented(message)
        | Error::Denied(message)
        | Error::NotFound(message)
        | Error::InvalidArgument(message)
        | Error::Conflict(message)
        | Error::Failed(message) => message,
    }
}

fn busy() -> String {
    tr!("workbench-extensions-plugin-busy").into()
}

fn no_state_dir() -> String {
    tr!("workbench-extensions-plugin-no-state-dir").into()
}

fn cant_revoke(name: &str) -> String {
    tr!("workbench-extensions-plugin-cant-revoke", name = name).into()
}

fn cant_uninstall(name: &str) -> String {
    tr!("workbench-extensions-plugin-cant-uninstall", name = name).into()
}

fn notify_error(cx: &mut App, message: String, detail: String) {
    Workbench::notify(cx, log::Level::Error, message, Some(detail));
}

#[cfg(test)]
#[path = "plugin_manage_tests.rs"]
mod plugin_manage_tests;
