//! A window whose folder was closed (File > Close Folder) reopens with no
//! folder at a launch with no argument, not on the folder it launched on;
//! a launch on that folder still opens on it. With the host's own choice
//! of windows ([`reopen::launch_windows_in`]) and real workbenches saving
//! and restoring their layouts, as `reopen_last_session.rs` has them.

#[allow(dead_code)]
#[path = "../src/reopen.rs"]
mod reopen;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{Launch, Workbench};
use gpui::{AnyWindowHandle, AppContext as _, TestAppContext, WindowHandle};
use kiln::{
    command::Value,
    shell::layout::{self, WindowLayout},
};

fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        cx.set_global(fe_workbench::ConfigDirOverride(None));
    });
}

/// A folder in `dir` holding `file`, canonical.
fn folder(dir: &Path, name: &str, file: &str) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir(&root).unwrap();
    fs::write(root.join(file), format!("{file}\n")).unwrap();
    dunce::canonicalize(root).unwrap()
}

/// The windows the app opens on a launch, as main.rs has them: local roots,
/// remote windows left out.
fn launch_windows(
    dir: Option<PathBuf>,
    root: Option<&Path>,
    folder: bool,
) -> Vec<(Option<PathBuf>, Option<WindowLayout>, Option<Value>)> {
    let windows = reopen::launch_windows_in(dir.clone(), root, folder);
    (reopen::openable(windows).into_iter())
        .map(|(root, layout, entry)| (reopen::local_root(root), layout, entry))
        .collect()
}

/// A window of a workbench on `root`, restoring `entry` and saving in
/// `layout`.
fn open(
    cx: &mut TestAppContext,
    root: Option<PathBuf>,
    layout: Option<WindowLayout>,
    entry: Option<Value>,
) -> WindowHandle<Workbench> {
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let launch = Launch { root, file: None };
        let mut workbench = Workbench::new(launch, theme, EditorSettings::default(), window, cx);
        if let Some(entry) = &entry {
            workbench.restore_layout(entry, window, cx);
        }
        if let Some(layout) = layout {
            workbench.set_layout(layout, window, cx);
        }
        workbench
    });
    cx.run_until_parked();
    window
}

/// The window's root, and its tabs' files by name, once it has `tabs` of
/// them (or the wait is over).
fn shown(
    window: WindowHandle<Workbench>,
    tabs: usize,
    cx: &mut TestAppContext,
) -> (Option<PathBuf>, Vec<String>) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        let shown = window
            .read_with(cx, |workbench, cx| {
                let names = (workbench.pane().read(cx).tabs().iter())
                    .filter_map(|tab| tab.path.as_ref()?.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                (workbench.root().map(Path::to_owned), names)
            })
            .unwrap();
        if shown.1.len() >= tabs || Instant::now() > deadline {
            return shown;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The app quits: its windows close keeping their entries.
fn quit(windows: Vec<AnyWindowHandle>, cx: &mut TestAppContext) {
    cx.update(layout::mark_quitting);
    for window in windows {
        cx.update_window(window, |_, window, _| window.remove_window())
            .unwrap();
    }
    cx.run_until_parked();
}

#[gpui::test]
fn a_closed_folder_is_not_reopened(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    let a = folder(dir.path(), "a", "a.txt");

    // A launch on a, a.txt open; the folder closed; the app quits.
    init(first);
    let store = fe_workbench::window_layouts(&layouts, Some(&a));
    let (saving, entry) = layout::launch(&store, true).remove(0);
    assert!(entry.is_none(), "nothing saved yet");
    let window = open(first, Some(a.clone()), Some(saving), None);
    let file = a.join("a.txt");
    window
        .update(first, |workbench, window, cx| {
            workbench.open_document(file, window, cx)
        })
        .unwrap();
    assert_eq!(
        shown(window, 1, first),
        (Some(a.clone()), vec!["a.txt".to_owned()])
    );
    window
        .update(first, |workbench, window, cx| {
            workbench.close_folder(window, cx)
        })
        .unwrap();
    assert_eq!(shown(window, 0, first), (None, Vec::new()), "closed");
    quit(vec![window.into()], first);

    // A launch with no argument: one window, with no folder and no tab.
    init(second);
    let windows = launch_windows(Some(layouts.clone()), None, true);
    assert_eq!(windows.len(), 1, "one window reopens");
    let (root, saving, entry) = windows.into_iter().next().unwrap();
    assert_eq!(root, None, "with no folder");
    assert!(entry.is_some(), "its saved window");
    let window = open(second, root, saving, entry);
    assert_eq!(shown(window, 0, second), (None, Vec::new()));
    quit(vec![window.into()], second);

    // A launch on a still opens on it.
    init(third);
    let windows = launch_windows(Some(layouts.clone()), Some(&a), true);
    assert_eq!(windows.len(), 1);
    let (root, saving, entry) = windows.into_iter().next().unwrap();
    assert_eq!(root.as_deref(), Some(a.as_path()));
    let window = open(third, root, saving, entry);
    assert_eq!(shown(window, 0, third).0, Some(a));
}
