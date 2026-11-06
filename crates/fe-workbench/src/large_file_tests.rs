//! Files of [`loader::LARGE_FILE_BYTES`] or more open in a [`FileViewer`]
//! tab, read-only, rather than being read into an editor: from the project
//! panel, quick open, [`Pane::open`], the launch and a saved layout. The
//! threshold is lowered for each test's directory
//! ([`loader::test_support::lower_threshold`]), and the indexes held back,
//! so a viewer is seen before its index completes.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::Settings;
use fe_fileview::{FileView, IndexState, LineIndex, Step};
use fe_gpui::{FileViewer, SettingsEvent, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_shell::layout::{LayoutStore, WindowLayout};

use super::Pane;
use crate::{
    Launch, Workbench,
    loader::{self, Opened, test_support},
};

/// The lowered threshold: a large file here is a few hundred KiB.
const THRESHOLD: u64 = 64 * 1024;

/// What row `row` of every large file says.
fn line(row: usize) -> String {
    format!("line {row:06}\n")
}

/// A large file's text: more than two of the index's chunks, so one step
/// of its index leaves it building.
fn large_text() -> String {
    let rows = (2 * fe_fileview::CHUNK_BYTES as usize) / line(0).len() + 100;
    (0..rows).map(line).collect()
}

/// A project of `large` files over the lowered threshold and `small` ones
/// under it, its large files' indexes held unless `free`.
fn project(large: &[&str], small: &[&str], free: bool) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    test_support::lower_threshold(&root, THRESHOLD, !free);
    let text = large_text();
    for name in large {
        std::fs::write(root.join(name), &text).unwrap();
    }
    for name in small {
        std::fs::write(root.join(name), format!("{name}\n")).unwrap();
    }
    (dir, root)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// Each tab of `pane`: its path, and whether it's a viewer.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<(PathBuf, bool)> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .map(|tab| {
                assert_ne!(tab.editor().is_some(), tab.viewer().is_some());
                (tab.path.clone().unwrap(), tab.viewer().is_some())
            })
            .collect()
    })
}

/// The active tab's viewer.
fn active_viewer(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Option<Entity<FileViewer>> {
    pane.read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
}

/// The rows `viewer` shows so far, and its first.
fn shown(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> (u32, String) {
    viewer.read_with(cx, |viewer, _| {
        let text = viewer.model().text();
        let row = text.row_start(0)..text.row_start(0) + text.row_len(0) as usize;
        (text.row_count(), text.chunks_in_range(row).collect())
    })
}

/// The one held index of `path`, stepped once: scanned one chunk and still
/// building.
fn step_once(path: &Path) -> Step {
    let mut steps = test_support::take_steps(path);
    assert_eq!(steps.len(), 1, "{} was opened once", path.display());
    let mut step = steps.pop().unwrap();
    let progress = step.next().expect("the index is building");
    assert_eq!(progress.state, IndexState::Building);
    step
}

/// Checks that the active tab is a viewer of `path` under the read-only
/// banner, showing the file's first rows while its held index is building,
/// and lets the index finish.
fn shows_viewer_before_index(pane: &Entity<Pane>, path: &Path, cx: &mut VisualTestContext) {
    let viewer = active_viewer(pane, cx).expect("a viewer is shown");
    let banner = viewer.read_with(cx, |viewer, _| viewer.banner().to_string());
    assert!(
        banner.starts_with("Read-only: ") && banner.ends_with(" is over 512 MiB."),
        "{banner}"
    );
    let step = step_once(path);
    // The viewer polls its index for the rows found since.
    cx.executor().advance_clock(Duration::from_millis(200));
    draw(cx);
    let (rows, first) = shown(&viewer, cx);
    assert!(rows > 1, "the first chunk's rows show");
    assert_eq!(first + "\n", line(0));
    assert!(viewer.read_with(cx, |viewer, _| viewer.is_polling()));
    // Unheld, the index finishes.
    drop(step);
}

/// Clicks the project panel's row named `name` where the last frame laid
/// it out.
fn click_row(workbench: &Entity<Workbench>, name: &str, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let ix = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        (tree.rows(0..tree.row_count()))
            .position(|row| row.name.to_string_lossy() == name)
            .unwrap_or_else(|| panic!("no row {name}"))
    });
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
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

#[gpui::test]
fn large_files_open_read_only(cx: &mut TestAppContext) {
    let (_dir, root) = project(
        &["panel.log", "quick.log", "pane.log"],
        &["small.txt"],
        false,
    );
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);

    // From the project panel.
    click_row(&workbench, "panel.log", cx);
    assert_eq!(tabs(&pane, cx), [(root.join("panel.log"), true)]);
    shows_viewer_before_index(&pane, &root.join("panel.log"), cx);
    // The viewer has the focus, and no editor shows.
    let viewer = active_viewer(&pane, cx).unwrap();
    assert!(cx.update(|window, cx| viewer.focus_handle(cx).is_focused(window)));
    assert!(workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_none()));

    // From quick open.
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("quick.log");
    let picker = crate::tests::quick_open::picker(&workbench, cx);
    assert_eq!(crate::tests::quick_open::paths(&picker, cx)[0], "quick.log");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(tabs(&pane, cx)[1], (root.join("quick.log"), true));
    shows_viewer_before_index(&pane, &root.join("quick.log"), cx);

    // From Pane::open.
    let path = root.join("pane.log");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    draw(cx);
    assert_eq!(tabs(&pane, cx)[2], (path.clone(), true));
    shows_viewer_before_index(&pane, &path, cx);

    // A file under the threshold opens in an editor, as before.
    click_row(&workbench, "small.txt", cx);
    assert_eq!(tabs(&pane, cx)[3], (root.join("small.txt"), false));
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("an editor shows");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.model().text().to_string(), "small.txt\n");
        assert_eq!(
            editor.model().path(),
            Some(root.join("small.txt").as_path())
        );
    });
    assert!(test_support::take_steps(&root.join("small.txt")).is_empty());

    // The loader says so too.
    assert!(matches!(
        loader::open(root.join("small.txt")),
        Ok(Opened::Text(Some(_)))
    ));
    assert!(matches!(
        loader::open(root.join("missing.txt")),
        Ok(Opened::Text(None))
    ));
    assert!(matches!(
        loader::open(root.join("pane.log")),
        Ok(Opened::Large { .. })
    ));
    drop(test_support::take_steps(&root.join("pane.log")));
}

#[gpui::test]
fn viewer_and_editor_tabs_together(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["big.log"], &["a.txt", "b.txt"], true);
    let (workbench, cx) = open_root(cx, &root);
    let pane = pane(&workbench, cx);
    for name in ["a.txt", "big.log", "b.txt"] {
        let path = root.join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    let big = root.join("big.log");
    let expected = vec![
        (root.join("a.txt"), false),
        (big.clone(), true),
        (root.join("b.txt"), false),
    ];
    assert_eq!(tabs(&pane, cx), expected);
    let viewer = pane.read_with(cx, |pane, _| pane.tabs()[1].viewer().cloned().unwrap());
    wait_for_rows(&viewer, cx);
    let active = |cx: &mut VisualTestContext| pane.read_with(cx, |pane, _| pane.active());

    // Cycling goes through the viewer, which is shown and focused.
    run("workbench.next_tab", cx);
    assert_eq!(active(cx), Some(0));
    run("workbench.next_tab", cx);
    assert_eq!(active(cx), Some(1));
    assert!(cx.update(|window, cx| viewer.focus_handle(cx).is_focused(window)));
    assert_eq!(
        pane.read_with(cx, |pane, cx| pane.title(cx)),
        "big.log — Tenmoku"
    );
    run("workbench.previous_tab", cx);
    assert_eq!(active(cx), Some(0));

    // Reopening the viewer's path shows its tab rather than a new one.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(big.clone(), window, cx)));
    draw(cx);
    assert_eq!(tabs(&pane, cx), expected);
    assert_eq!(active(cx), Some(1));

    // Only the editors are the open editors that LSP, git and the problems
    // follow; a sync with the viewer shown is fine.
    let editors = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            (workbench.open_editors(cx).iter())
                .map(|editor| editor.read(cx).model().path().unwrap().to_owned())
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(editors(cx), [root.join("a.txt"), root.join("b.txt")]);
    workbench.update(cx, |workbench, cx| workbench.sync_lsp(cx));
    draw(cx);

    // A settings reload reaches the editors and passes the viewer by.
    let mut settings = Settings::default();
    settings.editor.auto_close_pairs = false;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    let auto_close = pane.read_with(cx, |pane, cx| {
        (pane.tabs().iter())
            .filter_map(|tab| Some(tab.editor()?.read(cx).model().auto_close()))
            .collect::<Vec<_>>()
    });
    assert_eq!(auto_close, [false, false]);

    // Splitting the viewer opens another viewer of the file beside it. Both
    // share one index, so the original has to have read all of it first.
    wait_for_index(&viewer, &big, cx);
    run("pane.split_right", cx);
    let panes = workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().into_iter().cloned()).collect::<Vec<_>>()
    });
    assert_eq!(panes.len(), 2);
    assert_eq!(tabs(&panes[1], cx), [(big.clone(), true)]);
    let split = active_viewer(&panes[1], cx).unwrap();
    assert_ne!(split, viewer);
    assert_eq!(shown(&split, cx), shown(&viewer, cx));
    // Opened in the new pane, it shows that pane's viewer, not an editor.
    cx.update(|window, cx| panes[1].update(cx, |pane, cx| pane.open(big.clone(), window, cx)));
    draw(cx);
    assert_eq!(tabs(&panes[1], cx), [(big.clone(), true)]);
    // A small file opened there shares the first pane's buffer.
    let a = root.join("a.txt");
    cx.update(|window, cx| panes[1].update(cx, |pane, cx| pane.open(a.clone(), window, cx)));
    draw(cx);
    assert_eq!(tabs(&panes[1], cx), [(big.clone(), true), (a, false)]);
    run("workbench.close_tab", cx);
    run("workbench.close_tab", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.pane_group(cx).panes().len());
    assert_eq!(panes, 1, "the emptied pane closes");

    // A vim host command in an editor: `:e` of the viewer's path shows its
    // tab, and `:wa` writes the editors only.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    let first = pane.read_with(cx, |pane, _| pane.tabs()[0].editor().cloned().unwrap());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    ex("e big.log", cx);
    assert_eq!(tabs(&pane, cx), expected);
    assert_eq!(active(cx), Some(1));
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    cx.simulate_keystrokes("i x escape");
    assert!(first.read_with(cx, |editor, _| editor.model().is_dirty()));
    ex("wa", cx);
    assert!(!first.read_with(cx, |editor, _| editor.model().is_dirty()));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "xa.txt\n"
    );
    assert_eq!(std::fs::read_to_string(&big).unwrap(), large_text());

    // Closing the viewer's tab closes it without asking; the editors stay.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(1, window, cx)));
    draw(cx);
    run("workbench.close_tab", cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(
        tabs(&pane, cx),
        [(root.join("a.txt"), false), (root.join("b.txt"), false)]
    );
    assert!(workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_some()));
}

/// Types `:{line}` and Enter in the focused editor.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

#[gpui::test]
fn launch_file_and_restore(cx: &mut TestAppContext) {
    let (dir, root) = project(&["big.log"], &[], true);
    let big = root.join("big.log");
    let store = LayoutStore::new(dir.path().join("layouts"), &root);

    // The host opened the launch file as a large one.
    let view = Arc::new(FileView::open(&big).unwrap());
    let index = Arc::new(LineIndex::build(view.clone()));
    while index.progress().state == IndexState::Building {
        std::thread::sleep(Duration::from_millis(1));
    }
    let launch = Launch {
        root: Some(root.clone()),
        file: Some((big.clone(), Some(Opened::Large { view, index }))),
    };
    let mut app = cx.clone();
    let (first, cx, _) = crate::tests::open(&mut app, launch);
    draw(cx);
    let pane = pane(&first, cx);
    assert_eq!(tabs(&pane, cx), [(big.clone(), true)]);
    let viewer = active_viewer(&pane, cx).unwrap();
    let banner = viewer.read_with(cx, |viewer, _| viewer.banner().to_string());
    assert!(banner.ends_with(" is over 512 MiB."), "{banner}");
    assert_eq!(shown(&viewer, cx).1 + "\n", line(0));

    // Saved, the window's layout keeps the viewer.
    let claim = store.claim_new().unwrap();
    first.update_in(cx, |workbench, window, cx| {
        workbench.set_layout(WindowLayout::new(store.clone(), claim), window, cx)
    });
    draw(cx);
    cx.update(|window, _| window.remove_window());
    drop(first);
    cx.run_until_parked();
    let entries = store.entries();
    assert_eq!(entries.len(), 1);

    // Restored in a new window.
    let mut app = app.clone();
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (second, cx, _) = crate::tests::open(&mut app, launch);
    draw(cx);
    let (claim, entry) = store.restorable().pop().expect("the entry restores");
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx);
        workbench.set_layout(WindowLayout::new(store.clone(), claim), window, cx);
    });
    draw(cx);
    let pane = second.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(tabs(&pane, cx), [(big.clone(), true)]);
    let viewer = active_viewer(&pane, cx).unwrap();
    wait_for_rows(&viewer, cx);
    assert_eq!(shown(&viewer, cx).1 + "\n", line(0));
    let editors = second.read_with(cx, |workbench, cx| workbench.open_editors(cx));
    assert!(editors.is_empty());
}

/// Waits for the index of `viewer`, which shows `path`, to finish building
/// and for the viewer's poll to take all of its rows.
fn wait_for_index(viewer: &Entity<FileViewer>, path: &Path, cx: &mut VisualTestContext) {
    for _ in 0..500 {
        if !viewer.read_with(cx, |viewer, _| viewer.is_polling()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        cx.executor().advance_clock(Duration::from_millis(200));
        draw(cx);
    }
    panic!("the index of {} never finished", path.display());
}

/// Waits for `viewer`'s index, building on a thread of its own, to find
/// more than one row, and for the viewer's poll to show them.
fn wait_for_rows(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) {
    for _ in 0..500 {
        if shown(viewer, cx).0 > 1 {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        cx.executor().advance_clock(Duration::from_millis(200));
        draw(cx);
    }
    panic!("the viewer shows no rows");
}
