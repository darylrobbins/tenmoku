//! A launch with no argument reopens the windows open when the app last
//! quit, each on its own folder or none, with their tabs (T1666): with the
//! host's own choice of windows ([`reopen::launch_windows_in`]), and real
//! workbenches saving and restoring their layouts.

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
    core::layout::LayoutStore,
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

/// A session: a window on each of `windows`' roots, each showing its file,
/// then the app quits, keeping their entries.
fn session(layouts: &Path, windows: &[(Option<&Path>, PathBuf)], cx: &mut TestAppContext) {
    let windows: Vec<_> = (windows.iter())
        .map(|(root, file)| (*root, file.clone(), None))
        .collect();
    session_re_rooting(layouts, &windows, cx);
}

/// [`session`], each window then re-rooted at its third folder, if any, as
/// Open Folder does.
fn session_re_rooting(
    layouts: &Path,
    windows: &[(Option<&Path>, PathBuf, Option<PathBuf>)],
    cx: &mut TestAppContext,
) {
    init(cx);
    let mut open_windows: Vec<AnyWindowHandle> = Vec::new();
    for (root, file, re_root) in windows {
        let store = fe_workbench::window_layouts(layouts, *root);
        let (layout, entry) = layout::launch(&store, true).remove(0);
        assert!(entry.is_none(), "nothing saved yet");
        let window = open(cx, root.map(Path::to_owned), Some(layout), None);
        let file = file.clone();
        window
            .update(cx, |workbench, window, cx| {
                workbench.open_document(file, window, cx)
            })
            .unwrap();
        assert_eq!(shown(window, 1, cx).1.len(), 1, "the file opened");
        if let Some(re_root) = re_root.clone() {
            window
                .update(cx, |workbench, window, cx| {
                    workbench.set_root(re_root, window, cx)
                })
                .unwrap();
            cx.run_until_parked();
        }
        open_windows.push(window.into());
    }
    quit(open_windows, cx);
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

/// What a launch with no argument opens, each window's root and tabs,
/// sorted; then the app quits.
fn relaunch(layouts: &Path, cx: &mut TestAppContext) -> Vec<(Option<PathBuf>, Vec<String>)> {
    init(cx);
    let windows = reopen::launch_windows_in(Some(layouts.to_owned()), None, true);
    let mut opened: Vec<AnyWindowHandle> = Vec::new();
    let mut shown: Vec<_> = (windows.into_iter())
        .map(|(root, layout, entry)| {
            assert!(layout.is_some(), "it saves its layout");
            let tabs = usize::from(entry.is_some());
            let window = open(cx, root, layout, entry);
            opened.push(window.into());
            shown(window, tabs, cx)
        })
        .collect();
    quit(opened, cx);
    shown.sort();
    shown
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[gpui::test]
fn a_launch_with_no_argument_reopens_the_last_quit_windows_on_their_folders(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    let a = folder(dir.path(), "a", "a.txt");
    let b = folder(dir.path(), "b", "b.txt");
    let loose = folder(dir.path(), "loose", "c.txt").join("c.txt");
    session(
        &layouts,
        &[
            (Some(&a), a.join("a.txt")),
            (Some(&b), b.join("b.txt")),
            (None, loose),
        ],
        first,
    );

    let mut expected = vec![
        (None, names(&["c.txt"])),
        (Some(a.clone()), names(&["a.txt"])),
        (Some(b.clone()), names(&["b.txt"])),
    ];
    expected.sort();
    assert_eq!(relaunch(&layouts, second), expected);

    // A folder launch is as it was: the root's own windows only.
    init(third);
    let windows = reopen::launch_windows_in(Some(layouts.clone()), Some(&a), true);
    assert_eq!(windows.len(), 1);
    let (root, layout, entry) = windows.into_iter().next().unwrap();
    assert_eq!(root.as_deref(), Some(a.as_path()));
    let window = open(third, root, layout, entry);
    assert_eq!(shown(window, 1, third), (Some(a), names(&["a.txt"])));
}

#[gpui::test]
fn a_gone_folder_is_skipped_and_with_nothing_to_reopen_it_falls_back(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    let a = folder(dir.path(), "a", "a.txt");
    let b = folder(dir.path(), "b", "b.txt");
    session(
        &layouts,
        &[(Some(&a), a.join("a.txt")), (Some(&b), b.join("b.txt"))],
        first,
    );
    fs::remove_dir_all(&b).unwrap();
    assert_eq!(
        relaunch(&layouts, second),
        [(Some(a.clone()), names(&["a.txt"]))]
    );
    let gone = fe_workbench::window_layouts(&layouts, Some(&b));
    assert_eq!(gone.entries().len(), 1, "the gone folder's window is kept");

    // The last quit's only folder is gone: the last session with no root
    // (an older one) is restored, as before.
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    let c = folder(dir.path(), "c", "c.txt");
    let rootless = fe_workbench::window_layouts(&layouts, None);
    write(&rootless, json_entry(1, 100, 7, &c.join("c.txt")));
    let gone = dir.path().join("gone");
    write(
        &LayoutStore::new(&layouts, &gone),
        json_entry(2, 200, 9, &gone.join("x.txt")),
    );
    init(third);
    let windows = reopen::launch_windows_in(Some(layouts.clone()), None, true);
    assert_eq!(windows.len(), 1);
    let (root, layout, entry) = windows.into_iter().next().unwrap();
    assert_eq!(root, None);
    assert_eq!(entry.as_ref().and_then(|e| e["id"].as_u64()), Some(1));
    drop(layout);

    // Nothing saved at all: one empty window, saving its layout; and with
    // no state directory, one with nowhere to save.
    let empty = tempfile::tempdir().unwrap();
    let windows = reopen::launch_windows_in(Some(empty.path().join("layouts")), None, true);
    assert!(matches!(&windows[..], [(None, Some(_), None)]));
    drop(windows);
    let windows = reopen::launch_windows_in(None, None, true);
    assert!(matches!(&windows[..], [(None, None, None)]));
}

/// A saved entry, as a quit leaves it, showing `file` in its one pane.
fn json_entry(id: u64, saved: u64, session: u64, file: &Path) -> Value {
    serde_json::json!({
        "id": id,
        "saved": saved,
        "session": session,
        "panes": {"tree": {"pane": {"items": [
            {"kind": "editor", "document": 0, "state": {"path": file}}
        ], "active": 0}}, "active": []},
    })
}

/// Puts `entry` in `store`'s file, its root recorded, as a write does.
fn write(store: &LayoutStore, entry: Value) {
    fs::create_dir_all(store.file().parent().unwrap()).unwrap();
    let text = serde_json::json!({
        "version": 1,
        "root": store.root(),
        "windows": [entry],
    });
    fs::write(store.file(), serde_json::to_vec(&text).unwrap()).unwrap();
}

/// A window re-rooted by Open Folder reopens on its new folder, whichever
/// it opened on: one that opened with no root, and one whose first folder
/// is gone since.
#[gpui::test]
fn a_re_rooted_window_reopens_on_its_new_folder(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    let a = folder(dir.path(), "a", "a.txt");
    let b = folder(dir.path(), "b", "b.txt");
    let c = folder(dir.path(), "c", "c.txt");
    let loose = folder(dir.path(), "loose", "l.txt").join("l.txt");
    session_re_rooting(
        &layouts,
        &[
            (None, loose, Some(b.clone())),
            (Some(&a), c.join("c.txt"), Some(c.clone())),
        ],
        first,
    );
    fs::remove_dir_all(&a).unwrap();
    assert_eq!(
        relaunch(&layouts, second),
        [(Some(b), names(&["l.txt"])), (Some(c), names(&["c.txt"])),]
    );
}
