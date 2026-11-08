//! Several windows (`app-shell.md` §4.1, §4.6 and F4): `window.new` and
//! Open Folder in New Window ask the host for a window; two windows on one
//! root keep their own layouts across a quit and a relaunch, and a window
//! closed while the other stays doesn't come back; a launch on a root that
//! another app has open gets fresh windows rather than copies of its.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::Buffer;
use fe_command::Value;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{
    AnyWindowHandle, AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext,
    WindowHandle,
};
use kiln_shell::layout::{self, LayoutStore, WindowLayout};

use crate::{Launch, Workbench};

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

/// A window of a workbench for `launch`, restoring `entry` and saving in
/// `layout`, drawn.
fn open(
    cx: &mut TestAppContext,
    launch: Launch,
    layout: Option<WindowLayout>,
    entry: Option<Value>,
) -> (WindowHandle<Workbench>, VisualTestContext) {
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let mut workbench = Workbench::new(launch, theme, EditorSettings::default(), window, cx);
        if let Some(entry) = &entry {
            workbench.restore_layout(entry, window, cx);
        }
        if let Some(layout) = layout {
            workbench.set_layout(layout, window, cx);
        }
        workbench
    });
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let workbench = window.root(cx).unwrap();
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (window, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Each pane's files, by name.
fn files(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> Vec<Vec<String>> {
    let workbench: Entity<Workbench> = window.root(cx).unwrap();
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .map(|pane| {
                (pane.read(cx).tabs().iter())
                    .map(|tab| {
                        tab.path
                            .as_ref()
                            .unwrap()
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect()
            })
            .collect()
    })
}

fn project() -> (tempfile::TempDir, PathBuf, LayoutStore) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    for (name, text) in [("a.txt", "alpha\n"), ("b.txt", "beta\n")] {
        std::fs::write(root.join(name), text).unwrap();
    }
    let root = dunce::canonicalize(&root).unwrap();
    let store = LayoutStore::new(dir.path().join("layouts"), &root);
    (dir, root, store)
}

fn launch_file(root: &Path, name: &str) -> Launch {
    let text = std::fs::read_to_string(root.join(name)).unwrap();
    Launch {
        root: Some(root.to_owned()),
        file: Some((root.join(name), Some(Buffer::new(&text).into()))),
    }
}

/// The app quits: its windows close keeping their entries.
fn quit(windows: &[AnyWindowHandle], cx: &mut TestAppContext) {
    cx.update(layout::mark_quitting);
    for window in windows {
        cx.update_window(*window, |_, window, _| window.remove_window())
            .unwrap();
    }
    cx.run_until_parked();
}

/// The workbenches a launch on the root restores, opened.
fn relaunch(
    root: &Path,
    store: &LayoutStore,
    cx: &mut TestAppContext,
) -> Vec<WindowHandle<Workbench>> {
    init(cx);
    (layout::launch(store, true).into_iter())
        .map(|(layout, entry)| {
            let launch = Launch {
                root: Some(root.to_owned()),
                file: None,
            };
            open(cx, launch, Some(layout), entry).0
        })
        .collect()
}

#[gpui::test]
fn new_windows_come_from_the_host(cx: &mut TestAppContext) {
    let (_dir, root, _) = project();
    let other = tempfile::tempdir().unwrap();
    init(cx);
    let asked: Rc<RefCell<Vec<Option<PathBuf>>>> = Rc::default();
    cx.update(|cx| {
        let asked = asked.clone();
        crate::set_window_opener(cx, move |root, _| asked.borrow_mut().push(root));
    });
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (_, mut vcx) = open(cx, launch, None, None);
    run("window.new", &mut vcx);
    assert_eq!(*asked.borrow(), [Some(root.clone())]);
    run("workbench.open_folder_in_new_window", &mut vcx);
    assert!(vcx.did_prompt_for_paths());
    let picked = other.path().to_owned();
    vcx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files);
        Some(vec![picked])
    });
    vcx.run_until_parked();
    assert_eq!(*asked.borrow(), [Some(root), Some(other.path().to_owned())]);
}

#[gpui::test]
fn two_windows_on_one_root_keep_their_own_layouts(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    meanwhile: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    let (_dir, root, store) = project();

    // One app: a window split on a.txt, then a new window on b.txt.
    init(first);
    let mut launched = layout::launch(&store, true);
    assert_eq!(launched.len(), 1);
    let (layout, entry) = launched.remove(0);
    assert!(entry.is_none(), "nothing saved yet");
    let (one, mut one_cx) = open(first, launch_file(&root, "a.txt"), Some(layout), None);
    run("pane.split_right", &mut one_cx);
    let (layout, entry) = layout::new_window(&store).unwrap();
    assert!(entry.is_none(), "the first window holds its entry");
    let (two, _) = open(first, launch_file(&root, "b.txt"), Some(layout), None);
    assert_eq!(files(one, first), [["a.txt"], ["a.txt"]]);
    assert_eq!(files(two, first), [["b.txt"]]);
    assert_eq!(store.entries().len(), 2);
    quit(&[one.into(), two.into()], first);
    assert_eq!(store.entries().len(), 2);

    // Relaunched, both come back, each with its own tree.
    let windows = relaunch(&root, &store, second);
    let mut restored: Vec<_> = windows
        .iter()
        .map(|window| files(*window, second))
        .collect();
    restored.sort();
    assert_eq!(
        restored,
        [
            vec![vec!["a.txt".to_owned()], vec!["a.txt".to_owned()]],
            vec![vec!["b.txt".to_owned()]]
        ]
    );

    // Meanwhile another app launches on the root: its windows are fresh,
    // and its close leaves the first app's entries.
    init(meanwhile);
    let mut launched = layout::launch(&store, true);
    let (layout, entry) = launched.remove(0);
    assert!(
        launched.is_empty() && entry.is_none(),
        "not a copy of a window open elsewhere"
    );
    let ids: Vec<u64> = (store.entries().iter())
        .filter_map(kiln_core_entry_id)
        .collect();
    assert!(!ids.contains(&layout.id().unwrap()));
    let (fresh, _) = open(meanwhile, launch_file(&root, "a.txt"), Some(layout), None);
    assert_eq!(store.entries().len(), 3);
    meanwhile
        .update_window(fresh.into(), |_, window, _| window.remove_window())
        .unwrap();
    meanwhile.run_until_parked();
    let left: Vec<u64> = store
        .entries()
        .iter()
        .filter_map(kiln_core_entry_id)
        .collect();
    assert!(
        ids.iter().all(|id| left.contains(id)),
        "the first app's entries stay"
    );

    // One of the two closes while the other stays: it doesn't come back.
    let a = windows
        .iter()
        .copied()
        .find(|window| files(*window, second).len() == 2)
        .unwrap();
    second
        .update_window(a.into(), |_, window, _| window.remove_window())
        .unwrap();
    second.run_until_parked();
    let b = windows.into_iter().find(|window| *window != a).unwrap();
    quit(&[b.into()], second);
    let windows = relaunch(&root, &store, third);
    let mut restored: Vec<_> = windows.iter().map(|window| files(*window, third)).collect();
    restored.sort();
    // b.txt's window, and the other app's, which it kept as its last.
    assert_eq!(
        restored,
        [
            vec![vec!["a.txt".to_owned()]],
            vec![vec!["b.txt".to_owned()]]
        ]
    );
}

fn kiln_core_entry_id(entry: &Value) -> Option<u64> {
    entry.get("id")?.as_u64()
}
