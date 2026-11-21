// SPDX-License-Identifier: Apache-2.0

//! Which windows a launch opens, from the saved layouts: a launch with no
//! argument reopens those open when the app last quit, each on its own
//! folder (VS Code's `window.restoreWindows`), a folder launch the root's,
//! and a file launch one fresh window.
//!
//! A window's root is a [`ProjectLocation`]: a saved root that's a remote
//! folder's `ssh://` text is reopened, or not, without the local `is_dir`
//! check ([`launch_locations_in`]). The app has no remote windows yet, so its
//! launch ([`launch_windows_in`]) reopens local ones only, and says on
//! stderr which remote ones it couldn't, keeping their entries for when it
//! can (docs/decisions/j8wvgszah0-remote-development.md § 6).

use std::{
    cell::RefCell,
    io::{self, Write},
    path::{Path, PathBuf},
};

use fe_config::recent::{ProjectLocation, RemoteUnsupported};
use kiln::{command::Value, shell::layout::WindowLayout};
use kiln_l10n::verbatim;

/// A window a launch opens: its root, its layout (unless there's no state
/// directory), and the entry it restores, if any.
pub(crate) type LaunchWindow = (Option<PathBuf>, Option<WindowLayout>, Option<Value>);

/// [`LaunchWindow`], with a root that may be remote.
pub(crate) type LocationWindow = (Option<ProjectLocation>, Option<WindowLayout>, Option<Value>);

/// The windows a launch on the local folder `root` opens, at least one,
/// with the saved layouts in `dir` (none without a state directory), as
/// [`launch_locations_in`] has them less the remote ones, each said on
/// stderr.
pub(crate) fn launch_windows_in(
    dir: Option<PathBuf>,
    root: Option<&Path>,
    folder: bool,
) -> Vec<LaunchWindow> {
    launch_windows_reporting(dir, root, folder, &mut io::stderr())
}

/// [`launch_windows_in`], saying on `out` which remote windows aren't
/// reopened. Those are skipped as a gone folder's are, their entries kept,
/// and with none left the launch is as if none had been open.
pub(crate) fn launch_windows_reporting(
    dir: Option<PathBuf>,
    root: Option<&Path>,
    folder: bool,
    out: &mut dyn Write,
) -> Vec<LaunchWindow> {
    let skipped = RefCell::new(Vec::new());
    let root = root.map(|root| ProjectLocation::Local(root.to_owned()));
    let windows = launch_locations_in(dir, root.as_ref(), folder, &|remote| {
        skipped.borrow_mut().push(remote.clone());
        false
    });
    for remote in skipped.into_inner() {
        let error = RemoteUnsupported;
        log::warn!(target: "fastedit", "not reopening a window on {remote}: {error}");
        writeln!(out, "{}: {}: {}", verbatim!("tenmoku"), remote, error).ok();
    }
    (windows.into_iter())
        .map(|(root, layout, entry)| {
            let root = root.map(|root| match root {
                ProjectLocation::Local(path) => path,
                // The root is local, and the last quit's remote windows
                // were skipped.
                remote => unreachable!("{remote} reopened"),
            });
            (root, layout, entry)
        })
        .collect()
}

/// The windows a launch on `root` opens, at least one, with the saved
/// layouts in `dir` (none without a state directory): see
/// [`kiln::shell::layout::launch`]. A launch with no argument (no root, and
/// `folder`, no file) reopens the windows open when the app last quit, each
/// on its own folder or none ([`last_quit_windows`]), one on a remote folder
/// if `remote` says so; with none to reopen, those of the last such launch
/// with no root. A folder launch (`folder`) restores the root's saved
/// windows, a remote root's under its text form; a file launch restores
/// nothing.
pub(crate) fn launch_locations_in(
    dir: Option<PathBuf>,
    root: Option<&ProjectLocation>,
    folder: bool,
    remote: &dyn Fn(&ProjectLocation) -> bool,
) -> Vec<LocationWindow> {
    let dir = dir.as_deref();
    if root.is_none() && folder {
        let windows = dir
            .map(|dir| last_quit_windows(dir, remote))
            .unwrap_or_default();
        if !windows.is_empty() {
            return windows;
        }
    }
    let key = root.map(|root| match root {
        ProjectLocation::Local(path) => kiln::core::trust::canonical(path),
        remote => PathBuf::from(remote.text()),
    });
    let windows: Vec<_> = (dir.map(|dir| fe_workbench::window_layouts(dir, key.as_deref())))
        .iter()
        .flat_map(|store| kiln::shell::layout::launch(store, folder))
        .map(|(layout, entry)| (root.cloned(), Some(layout), entry))
        .collect();
    if windows.is_empty() {
        return vec![(root.cloned(), None, None)];
    }
    windows
}

/// The windows open when the app last quit, with their layouts in `dir`,
/// each on the folder it was on, or none for the windows with no root: see
/// [`kiln::shell::layout::launch_last_quit`]. A window whose folder is gone
/// is skipped, its entry kept for when it's back; one on a remote folder,
/// which isn't looked for, if `remote` says so.
fn last_quit_windows(dir: &Path, remote: &dyn Fn(&ProjectLocation) -> bool) -> Vec<LocationWindow> {
    // Before this launch's first write there, as `window_layouts` notes it.
    fe_config::first_run::note_writing(dir);
    let there = |root: &Path| {
        if root.as_os_str().is_empty() {
            return true;
        }
        match ProjectLocation::from_text(root.as_os_str()) {
            ProjectLocation::Local(root) => {
                let there = root.is_dir();
                if !there {
                    log::info!(target: "fastedit", "not reopening a window on {}: it's gone", root.display());
                }
                there
            }
            location => remote(&location),
        }
    };
    (kiln::shell::layout::launch_last_quit(dir, there).into_iter())
        .map(|(root, layout, entry)| {
            let root = (!root.as_os_str().is_empty())
                .then(|| ProjectLocation::from_text(root.as_os_str()));
            (root, Some(layout), Some(entry))
        })
        .collect()
}
