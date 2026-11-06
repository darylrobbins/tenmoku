//! Extensions: Install Folder for Development…: an extension, a plugin
//! usually, used in place from the folder it's built in, linked into the
//! user's extensions directory rather than copied.
//!
//! - **Check.** The folder holds `extension.toml` at its top level, checked
//!   as an install's is ([`check`]): kiln's extension check, then a plugin
//!   entry as the user's. The extensions directory and what's in it can't
//!   be linked.
//! - **Link.** As the symlink `<extensions dir>/<id>`, to the folder. One
//!   already there to that folder is kept as it is; anything else there,
//!   or another directory installing that id, refuses it.
//! - **Reload.** The extensions service watches the extensions directory's
//!   tree, links followed: kiln's tree watcher watches a directory a link
//!   leads to outside it too, so a rebuilt `.wasm` or edited manifest behind
//!   the link is a change under the extensions directory, which reloads the
//!   plugin as an installed one's does ([`crate::plugin_host`]'s Reload):
//!   the same capabilities ask nothing, more ask as any update does. The
//!   watcher's 3 s re-stat is only the fallback it is for any extension.
//! - **Listed, removed.** Extensions: Show Installed says where it's linked
//!   from; Uninstall Plugin removes the link, never the folder.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use gpui::{App, AppContext as _, AsyncApp, Context, PathPromptOptions, Window};
use kiln::core::extension::{ExtensionApi, MANIFEST_FILE};

use crate::{
    InstallError, Installed, Workbench,
    extension_install::{
        check, check_api, extensions_dir, file_name, installed_text, installed_with,
        reserved_namespaces, version_text,
    },
};

/// Links the extension in `folder` into `extensions_dir` as
/// `<extensions dir>/<id>`, after checking its manifest against `api`. A
/// link there to `folder` already is kept. Reads and writes files: for a
/// background thread.
pub fn link_extension_folder(
    folder: &Path,
    extensions_dir: &Path,
) -> Result<Installed, InstallError> {
    link(folder, extensions_dir, &check_api(Vec::new()))
}

/// [`link_extension_folder`], checked against `api`.
pub(crate) fn link(
    folder: &Path,
    extensions_dir: &Path,
    api: &ExtensionApi,
) -> Result<Installed, InstallError> {
    if !fs::metadata(folder)?.is_dir() {
        return Err(InstallError::Refused("Pick a folder".to_owned()));
    }
    fs::create_dir_all(extensions_dir)?;
    let extensions_dir = fs::canonicalize(extensions_dir)?;
    let folder = fs::canonicalize(folder)?;
    if extensions_dir.starts_with(&folder) || folder.starts_with(&extensions_dir) {
        let reason = "It's the folder the extensions are installed in, or in it, or holds it";
        return Err(InstallError::Refused(reason.to_owned()));
    }
    if !folder.join(MANIFEST_FILE).is_file() {
        return Err(InstallError::Refused(format!(
            "There's no {MANIFEST_FILE} at its top level"
        )));
    }
    let installed = check(&folder, &extensions_dir, api)?;
    let link = installed.dir.clone();
    let ours = links_to(&link, &folder);
    if !ours && fs::symlink_metadata(&link).is_ok() {
        return Err(InstallError::Refused(format!(
            "{} is there already, and isn't a link to this folder",
            link.display()
        )));
    }
    let elsewhere =
        (installed_with(&extensions_dir, &installed.id)?.into_iter()).find(|dir| *dir != link);
    if let Some(dir) = elsewhere {
        return Err(InstallError::Refused(format!(
            "{} is installed already, in {}: uninstall it first",
            installed.id,
            dir.display()
        )));
    }
    if !ours {
        symlink_dir(&folder, &link)?;
    }
    Ok(installed)
}

/// Whether `link` is a symlink leading to `folder`, a canonical path.
fn links_to(link: &Path, folder: &Path) -> bool {
    let is_link = fs::symlink_metadata(link).is_ok_and(|meta| meta.file_type().is_symlink());
    is_link && fs::canonicalize(link).is_ok_and(|target| target == folder)
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

/// Where an extension's directory leads, if it's a link: a linked
/// extension's folder.
pub(crate) fn linked_from(dir: &Path) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(dir).ok()?;
    meta.file_type()
        .is_symlink()
        .then(|| fs::read_link(dir).ok())
        .flatten()
}

/// The success toast's message and detail.
fn linked_text(installed: &Installed, folder: &Path, preview: bool) -> (String, String) {
    let message = format!(
        "Installed {} {} for development",
        installed.name,
        version_text(installed.version)
    );
    let (_, plugin) = installed_text(installed, preview);
    let mut detail = format!(
        "Linked from {}: it reloads when it's rebuilt.",
        folder.display()
    );
    if let Some(plugin) = plugin {
        detail.push_str(&format!(" {plugin}."));
    }
    (message, detail)
}

impl Workbench {
    /// Asks for a folder and links the extension in it, off the main
    /// thread; the toast says how it went.
    pub(crate) fn link_extension_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(extensions_dir) = extensions_dir(cx) else {
            let detail = "There's no state directory to install extensions in.".to_owned();
            let message = "Can't install an extension".to_owned();
            return Workbench::notify(cx, log::Level::Error, message, Some(detail));
        };
        let api = check_api(reserved_namespaces(cx));
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Install for Development".into()),
        });
        cx.spawn_in(window, async move |_, cx| {
            let folder = match answer.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    log::warn!("the open panel failed: {error}");
                    None
                }
            };
            let Some(folder) = folder else {
                return;
            };
            let linking = folder.clone();
            let linked = cx
                .background_spawn(async move { link(&linking, &extensions_dir, &api) })
                .await;
            AsyncApp::update(cx, |cx| finish_link(&folder, linked, cx));
        })
        .detach();
    }
}

/// Toasts how linking `folder` went.
fn finish_link(folder: &Path, linked: Result<Installed, InstallError>, cx: &mut App) {
    match linked {
        Ok(installed) => {
            let preview = crate::plugin_host::preview_on(cx);
            let (message, detail) = linked_text(&installed, folder, preview);
            Workbench::notify(cx, log::Level::Info, message, Some(detail));
        }
        Err(error) => {
            let message = format!("Can't install {}", file_name(folder));
            Workbench::notify(cx, log::Level::Error, message, Some(error.to_string()));
        }
    }
}

#[cfg(test)]
#[path = "plugin_dev_tests.rs"]
mod plugin_dev_tests;
