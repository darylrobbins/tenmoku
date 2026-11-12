//! Extensions: Install Folder for Development…: an extension, a plugin
//! usually, used in place from the folder it's built in, linked into the
//! user's extensions directory rather than copied.
//!
//! - **Check.** The folder holds `extension.toml` at its top level, checked
//!   as an install's is ([`check`]): kiln's extension check, then a plugin
//!   entry as the user's. The extensions directory and what's in it can't
//!   be linked.
//! - **Link.** As the symlink `<extensions dir>/<id>`, to the folder; on
//!   Windows without the privilege to symlink (error 1314), as a directory
//!   junction, which needs none but leads only to a local volume. One
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
use kiln_l10n::tr;

use crate::{
    InstallError, Installed, Workbench,
    extension_install::{
        cant_install, check, check_api, extensions_dir, installed_with, no_extensions_dir,
        reserved_namespaces, version_text,
    },
    plugin_manage::PREVIEW_SETTING,
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
    link_with(folder, extensions_dir, api, false)
}

/// [`link`], making the link as a junction, as it falls back to without the
/// privilege to symlink, if `force_junction`: the seam tests use.
pub(crate) fn link_with(
    folder: &Path,
    extensions_dir: &Path,
    api: &ExtensionApi,
    force_junction: bool,
) -> Result<Installed, InstallError> {
    if !fs::metadata(folder)?.is_dir() {
        return Err(refused(tr!("workbench-extensions-dev-pick-folder")));
    }
    fs::create_dir_all(extensions_dir)?;
    let extensions_dir = kiln::core::platform::canonicalize(extensions_dir)?;
    let folder = kiln::core::platform::canonicalize(folder)?;
    if extensions_dir.starts_with(&folder) || folder.starts_with(&extensions_dir) {
        return Err(refused(tr!("workbench-extensions-dev-extensions-folder")));
    }
    if !folder.join(MANIFEST_FILE).is_file() {
        return Err(refused(tr!(
            "workbench-extensions-dev-no-manifest",
            file = MANIFEST_FILE
        )));
    }
    let installed = check(&folder, &extensions_dir, api)?;
    let link = installed.dir.clone();
    let ours = links_to(&link, &folder);
    if !ours && fs::symlink_metadata(&link).is_ok() {
        return Err(refused(tr!(
            "workbench-extensions-dev-taken",
            dir = link.display().to_string()
        )));
    }
    let elsewhere =
        (installed_with(&extensions_dir, &installed.id)?.into_iter()).find(|dir| *dir != link);
    if let Some(dir) = elsewhere {
        return Err(refused(tr!(
            "workbench-extensions-dev-installed-elsewhere",
            id = installed.id.as_str(),
            dir = dir.display().to_string()
        )));
    }
    if !ours {
        make_link(&folder, &link, force_junction)?;
    }
    Ok(installed)
}

fn refused(reason: gpui::SharedString) -> InstallError {
    InstallError::Refused(reason.into())
}

/// Whether `link` is a symlink or a junction leading to `folder`, a
/// canonical path.
pub(crate) fn links_to(link: &Path, folder: &Path) -> bool {
    // std reports a junction, a mount-point reparse point, as a symlink.
    let is_link = fs::symlink_metadata(link).is_ok_and(|meta| meta.file_type().is_symlink());
    is_link && kiln::core::platform::canonicalize(link).is_ok_and(|target| target == folder)
}

/// Links `link` to the directory `target`: a symlink, or a junction if
/// `force_junction` or the symlink isn't allowed.
fn make_link(target: &Path, link: &Path, force_junction: bool) -> Result<(), InstallError> {
    if !force_junction {
        match symlink_dir(target, link) {
            Ok(()) => return Ok(()),
            Err(error) if !privilege_not_held(&error) => return Err(error.into()),
            Err(_) => {}
        }
    }
    make_junction(target, link).map_err(|error| {
        let reason = if is_network_path(target) {
            tr!(
                "workbench-extensions-dev-junction-network",
                folder = target.display().to_string()
            )
        } else {
            tr!(
                "workbench-extensions-dev-junction-failed",
                error = error.to_string()
            )
        };
        refused(reason)
    })
}

/// Windows' ERROR_PRIVILEGE_NOT_HELD: no right to create symlinks.
fn privilege_not_held(error: &io::Error) -> bool {
    error.raw_os_error() == Some(1314)
}

/// Whether `path` is on a network volume: a UNC path, plain or verbatim
/// (`\\?\UNC\server\share`, which canonicalizing keeps).
pub(crate) fn is_network_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    match text.strip_prefix("\\\\") {
        Some(rest) => {
            let verbatim = rest.strip_prefix("?\\");
            match verbatim {
                Some(rest) => rest
                    .get(..4)
                    .is_some_and(|head| head.eq_ignore_ascii_case("UNC\\")),
                None => !rest.starts_with(".\\"),
            }
        }
        None => false,
    }
}

/// Whether cmd.exe would read `path` as more than a name inside quotes.
#[cfg(any(windows, test))]
pub(crate) fn cmd_unsafe(path: &Path) -> bool {
    path.to_string_lossy()
        .chars()
        .any(|c| matches!(c, '%' | '"' | '^' | '!' | '\n' | '\r' | '\0'))
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

/// A directory junction `link` to `target`. Unix has none: a symlink stands
/// in, so tests of what follows run there.
#[cfg(unix)]
pub(crate) fn make_junction(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
pub(crate) fn make_junction(target: &Path, link: &Path) -> io::Result<()> {
    use kiln_l10n::verbatim;
    use std::os::windows::process::CommandExt as _;
    // mklink is cmd's own: the paths go in quotes, which keep `&` and
    // spaces as names, and a path cmd would still expand is refused.
    if cmd_unsafe(target) || cmd_unsafe(link) {
        return Err(io::Error::other(
            tr!("workbench-extensions-dev-junction-name").to_string(),
        ));
    }
    const QUOTE: &str = verbatim!("\"");
    const SPACE: &str = verbatim!(" ");
    const MKLINK: &str = verbatim!("mklink /J ");
    // The paths' own text, not a lossy rendering of it.
    let mut command = std::ffi::OsString::from(MKLINK);
    for path in [link, target] {
        command.push(QUOTE);
        command.push(path.as_os_str());
        command.push(QUOTE);
        command.push(SPACE);
    }
    let output = std::process::Command::new("cmd")
        .arg("/D")
        .arg("/C")
        .raw_arg(command)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        let text =
            String::from_utf8_lossy(&output.stdout) + String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(text.trim().to_owned()))
    }
}

/// Where an extension's directory leads, if it's a link or junction: a linked
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
    let message = tr!(
        "workbench-extensions-dev-installed",
        name = installed.name.as_str(),
        version = version_text(installed.version)
    );
    let folder = folder.display().to_string();
    let detail = match (installed.plugin, preview) {
        (false, _) => tr!("workbench-extensions-dev-linked", folder = folder),
        (true, true) => tr!("workbench-extensions-dev-linked-plugin", folder = folder),
        (true, false) => tr!(
            "workbench-extensions-dev-linked-plugin-needs-preview",
            folder = folder,
            setting = PREVIEW_SETTING
        ),
    };
    (message.into(), detail.into())
}

impl Workbench {
    /// Asks for a folder and links the extension in it, off the main
    /// thread; the toast says how it went.
    pub(crate) fn link_extension_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(extensions_dir) = extensions_dir(cx) else {
            let (message, detail) = no_extensions_dir();
            return Workbench::notify(cx, log::Level::Error, message, Some(detail));
        };
        let api = check_api(reserved_namespaces(cx));
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("workbench-extensions-dev-button")),
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
            let message = cant_install(folder);
            Workbench::notify(cx, log::Level::Error, message, Some(error.to_string()));
        }
    }
}

#[cfg(test)]
#[path = "plugin_dev_tests.rs"]
mod plugin_dev_tests;
