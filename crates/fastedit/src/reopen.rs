// SPDX-License-Identifier: Apache-2.0

//! Which windows a launch opens, from the saved layouts: a launch with no
//! argument reopens those open when the app last quit, each on its own
//! folder (VS Code's `window.restoreWindows`), a folder launch the root's,
//! and a file launch one fresh window.

use std::path::{Path, PathBuf};

use kiln::{command::Value, shell::layout::WindowLayout};

/// A window a launch opens: its root, its layout (unless there's no state
/// directory), and the entry it restores, if any.
pub(crate) type LaunchWindow = (Option<PathBuf>, Option<WindowLayout>, Option<Value>);

/// The windows a launch on `root` opens, at least one, with the saved
/// layouts in `dir` (none without a state directory): see
/// [`kiln::shell::layout::launch`]. A launch with no argument (no root, and
/// `folder`, no file) reopens the windows open when the app last quit, each
/// on its own folder or none ([`last_quit_windows`]); with none to reopen,
/// those of the last such launch with no root. A folder launch (`folder`)
/// restores the root's saved windows; a file launch restores nothing.
pub(crate) fn launch_windows_in(
    dir: Option<PathBuf>,
    root: Option<&Path>,
    folder: bool,
) -> Vec<LaunchWindow> {
    let dir = dir.as_deref();
    if root.is_none() && folder {
        let windows = dir.map(last_quit_windows).unwrap_or_default();
        if !windows.is_empty() {
            return windows;
        }
    }
    let canonical = root.map(kiln::core::trust::canonical);
    let root = root.map(Path::to_owned);
    let windows: Vec<_> = (dir.map(|dir| fe_workbench::window_layouts(dir, canonical.as_deref())))
        .iter()
        .flat_map(|store| kiln::shell::layout::launch(store, folder))
        .map(|(layout, entry)| (root.clone(), Some(layout), entry))
        .collect();
    if windows.is_empty() {
        return vec![(root, None, None)];
    }
    windows
}

/// The windows open when the app last quit, with their layouts in `dir`,
/// each on the folder it was on, or none for the windows with no root: see
/// [`kiln::shell::layout::launch_last_quit`]. A window whose folder is gone
/// is skipped, its entry kept for when it's back.
fn last_quit_windows(dir: &Path) -> Vec<LaunchWindow> {
    // Before this launch's first write there, as `window_layouts` notes it.
    fe_config::first_run::note_writing(dir);
    let there = |root: &Path| {
        let there = root.as_os_str().is_empty() || root.is_dir();
        if !there {
            log::info!(target: "fastedit", "not reopening a window on {}: it's gone", root.display());
        }
        there
    };
    (kiln::shell::layout::launch_last_quit(dir, there).into_iter())
        .map(|(root, layout, entry)| {
            let root = (!root.as_os_str().is_empty()).then_some(root);
            (root, Some(layout), Some(entry))
        })
        .collect()
}
