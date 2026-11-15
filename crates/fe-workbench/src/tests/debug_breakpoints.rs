//! The breakpoint store (`debug_breakpoints.rs`, debugger card D5) in a
//! headless workbench with vim on: breakpoints following edits typed
//! through the default keymap, saves and reloads, and the state file read
//! by a second app as a restart would.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use fe_project::FsChange;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use serde_json::{Value, json};

use crate::{
    Launch, Workbench,
    debug_breakpoints::{BreakpointEvent, BreakpointStore, WRITE_REST, set_state_dir, state_file},
};

const TEXT: &str = "zero\none\ntwo\nthree\nfour\nfive\n";

/// A workbench on a folder holding `a.txt` with `text`, showing it with
/// vim on, its breakpoints kept under `state` if given.
struct Opened<'a> {
    dir: tempfile::TempDir,
    path: PathBuf,
    workbench: Entity<Workbench>,
    editor: Entity<EditorView>,
    store: Entity<BreakpointStore>,
    cx: &'a mut VisualTestContext,
}

fn opened<'a>(cx: &'a mut TestAppContext, text: &str, state: Option<&Path>) -> Opened<'a> {
    let dir = tempfile::tempdir().unwrap();
    let path = dunce::canonicalize(dir.path()).unwrap().join("a.txt");
    fs::write(&path, text).unwrap();
    let (workbench, editor, store, cx) = open_window(cx, dir.path(), &path, state);
    Opened {
        dir,
        path,
        workbench,
        editor,
        store,
        cx,
    }
}

/// A window on `root` showing `path`, with vim on.
fn open_window<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    path: &Path,
    state: Option<&Path>,
) -> (
    Entity<Workbench>,
    Entity<EditorView>,
    Entity<BreakpointStore>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_state_dir(cx, state.map(Path::to_owned));
    });
    another_window(cx, root, path)
}

/// Another window on `root` showing `path`, in an app set up already.
fn another_window<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    path: &Path,
) -> (
    Entity<Workbench>,
    Entity<EditorView>,
    Entity<BreakpointStore>,
    &'a mut VisualTestContext,
) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    let store = cx
        .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.breakpoint_store(cx)))
        .expect("rooted");
    (workbench, editor, store, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Types `keys` into the focused editor, and lets the store follow.
fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// Types `text` in insert mode.
fn input(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

/// The file's breakpoints as (row from 0, enabled).
fn rows(o: &mut Opened, path: &Path) -> Vec<(u32, bool)> {
    rows_of(&o.store, path, o.cx)
}

fn rows_of(
    store: &Entity<BreakpointStore>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Vec<(u32, bool)> {
    store.read_with(cx, |store, _| {
        (store.breakpoints(path).into_iter())
            .map(|breakpoint| (breakpoint.line - 1, breakpoint.enabled))
            .collect()
    })
}

fn toggle(o: &mut Opened, row: u32) {
    let path = o.path.clone();
    o.store
        .update(o.cx, |store, cx| store.toggle(&path, row, cx));
}

fn disable(o: &mut Opened, row: u32) {
    let path = o.path.clone();
    let changed = o
        .store
        .update(o.cx, |store, cx| store.set_enabled(&path, row, false, cx));
    assert!(changed, "a breakpoint on row {row}");
}

fn text(o: &Opened) -> String {
    o.editor
        .read_with(o.cx, |editor, _| editor.model().buffer().text().to_string())
}

/// The text of `row` of the open file.
fn row_text(o: &Opened, row: u32) -> String {
    text(o)
        .split('\n')
        .nth(row as usize)
        .unwrap_or_default()
        .to_owned()
}

/// Saves the editor, as `editor.save` does.
fn save(o: &mut Opened) {
    let editor = o.editor.clone();
    o.cx.update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)))
        .detach();
    draw(o.cx);
    let dirty = editor.read_with(o.cx, |editor, _| editor.model().buffer().is_dirty());
    assert!(!dirty, "saved");
}

/// Waits out the write's rest.
fn rest(cx: &mut VisualTestContext) {
    cx.executor()
        .advance_clock(WRITE_REST + Duration::from_millis(1));
    draw(cx);
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Writes `text` to `path` as another program would, its modified time
/// moved on so the change shows however coarse the file system's clock.
fn rewrite(path: &Path, text: &str) {
    static LATER: AtomicU64 = AtomicU64::new(1);
    fs::write(path, text).unwrap();
    let later = Duration::from_secs(10 * LATER.fetch_add(1, Ordering::Relaxed));
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + later).unwrap();
}

#[gpui::test]
fn a_breakpoint_follows_rows_inserted_and_deleted_above_it(cx: &mut TestAppContext) {
    let mut o = opened(cx, TEXT, None);
    let path = o.path.clone();
    toggle(&mut o, 3);
    assert_eq!(rows(&mut o, &path), [(3, true)]);

    // Two rows typed above it, by `O` on the first row.
    keys("g g shift-o", o.cx);
    input("new", o.cx);
    keys("enter", o.cx);
    input("newer", o.cx);
    keys("escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(5, true)]);
    assert_eq!(row_text(&o, 5), "three");

    // A row below it changes nothing.
    keys("shift-g o", o.cx);
    input("below", o.cx);
    keys("escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(5, true)]);

    // Three rows above it deleted.
    keys("g g 3 d d", o.cx);
    assert_eq!(text(&o), "one\ntwo\nthree\nfour\nfive\n\nbelow");
    assert_eq!(rows(&mut o, &path), [(2, true)]);
    assert_eq!(row_text(&o, 2), "three");

    // Undo puts them back, and the breakpoint follows.
    keys("u", o.cx);
    assert_eq!(rows(&mut o, &path), [(5, true)]);
    assert_eq!(row_text(&o, 5), "three");
}

#[gpui::test]
fn a_newline_at_the_rows_start_moves_it_down_with_its_text(cx: &mut TestAppContext) {
    let mut o = opened(cx, TEXT, None);
    let path = o.path.clone();
    toggle(&mut o, 2);

    // `O` on its row.
    keys("g g j j shift-o escape", o.cx);
    assert_eq!(row_text(&o, 2), "");
    assert_eq!(rows(&mut o, &path), [(3, true)]);
    assert_eq!(row_text(&o, 3), "two");

    // Enter at its row's column 0.
    keys("j 0 i enter escape", o.cx);
    assert_eq!(row_text(&o, 3), "");
    assert_eq!(rows(&mut o, &path), [(4, true)]);
    assert_eq!(row_text(&o, 4), "two");

    // Text typed at the row's start stays on the row.
    keys("0 i", o.cx);
    input("x ", o.cx);
    keys("escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(4, true)]);
    assert_eq!(row_text(&o, 4), "x two");

    // `o` on the row above moves it down too: the code moved.
    keys("k o escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(5, true)]);
    assert_eq!(row_text(&o, 5), "x two");
}

#[gpui::test]
fn deleting_its_row_moves_it_to_the_surviving_row(cx: &mut TestAppContext) {
    let mut o = opened(cx, TEXT, None);
    let path = o.path.clone();
    toggle(&mut o, 2);
    keys("g g j j d d", o.cx);
    assert_eq!(text(&o), "zero\none\nthree\nfour\nfive\n");
    assert_eq!(
        rows(&mut o, &path),
        [(2, true)],
        "on three, which took its place"
    );

    // The last row's: onto the row it leaves last.
    toggle(&mut o, 4);
    keys("5 G d d", o.cx);
    assert_eq!(text(&o), "zero\none\nthree\nfour\n");
    assert_eq!(row_text(&o, 4), "");
    assert_eq!(rows(&mut o, &path), [(2, true), (4, true)]);

    // Two on one row merge, the first's settings kept: the deleted row's
    // disabled one over the enabled one below it.
    toggle(&mut o, 1);
    disable(&mut o, 1);
    keys("g g j d d", o.cx);
    assert_eq!(text(&o), "zero\nthree\nfour\n");
    assert_eq!(rows(&mut o, &path), [(1, false), (3, true)]);
}

#[gpui::test]
fn joining_its_row_to_the_one_above_moves_it_there(cx: &mut TestAppContext) {
    let mut o = opened(cx, TEXT, None);
    let path = o.path.clone();
    toggle(&mut o, 2);
    keys("g g j shift-j", o.cx);
    assert_eq!(row_text(&o, 1), "one two");
    assert_eq!(rows(&mut o, &path), [(1, true)]);

    // Joined onto a row with its own: one stays, the first's, the one
    // above's settings.
    disable(&mut o, 1);
    toggle(&mut o, 2);
    keys("shift-j", o.cx);
    assert_eq!(row_text(&o, 1), "one two three");
    assert_eq!(rows(&mut o, &path), [(1, false)]);

    // And the rows below follow the join.
    toggle(&mut o, 3);
    keys("g g shift-j", o.cx);
    assert_eq!(row_text(&o, 0), "zero one two three");
    assert_eq!(rows(&mut o, &path), [(0, false), (2, true)]);
    assert_eq!(row_text(&o, 2), "five");
}

#[gpui::test]
fn a_restart_restores_breakpoints(cx: &mut TestAppContext, restarted: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    let root = dunce::canonicalize(o.dir.path()).unwrap();
    let file = state_file(state.path(), &root);
    assert_eq!(
        file,
        state
            .path()
            .join("debug")
            .join(format!("{}.json", kiln::core::layout::layout_key(&root)))
    );

    toggle(&mut o, 1);
    toggle(&mut o, 3);
    disable(&mut o, 3);
    draw(o.cx);
    assert!(!file.exists(), "written once changes rest");
    rest(o.cx);
    let written = json!({
        "breakpoints": [
            {"path": "a.txt", "line": 2, "enabled": true},
            {"path": "a.txt", "line": 4, "enabled": false},
        ],
        "watches": [],
    });
    assert_eq!(read_json(&file), written);

    // Edits alone don't write: the rows are written back on save.
    keys("g g shift-o", o.cx);
    input("new", o.cx);
    keys("escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(2, true), (4, false)]);
    rest(o.cx);
    assert_eq!(read_json(&file), written);
    save(&mut o);
    rest(o.cx);
    let saved = json!({
        "breakpoints": [
            {"path": "a.txt", "line": 3, "enabled": true},
            {"path": "a.txt", "line": 5, "enabled": false},
        ],
        "watches": [],
    });
    assert_eq!(read_json(&file), saved);
    let names: Vec<_> = (fs::read_dir(file.parent().unwrap()).unwrap())
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, [file.file_name().unwrap()], "no temporary file left");

    // A second app, as after a restart, reads them back, before and after
    // the file opens.
    let state_dir = state.path().to_owned();
    let store = restarted.update(|cx| {
        set_state_dir(cx, Some(state_dir));
        BreakpointStore::for_root(&root, cx)
    });
    let read = store.read_with(restarted, |store, _| store.breakpoints(&path));
    let lines: Vec<_> = read.iter().map(|b| (b.line, b.enabled)).collect();
    assert_eq!(lines, [(3, true), (5, false)]);
    let (_, editor, store, cx) = open_window(restarted, &root, &path, Some(state.path()));
    assert_eq!(rows_of(&store, &path, cx), [(2, true), (4, false)]);
    let text = editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string());
    assert_eq!(text, format!("new\n{TEXT}"));

    // And anchors them in the file: they follow the next edit.
    cx.simulate_keystrokes("g g d d");
    draw(cx);
    assert_eq!(rows_of(&store, &path, cx), [(1, true), (3, false)]);
}

#[gpui::test]
fn an_unknown_watches_list_survives_a_write(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("a.py"), "x = 1\ny = 2\nprint(x + y)\n").unwrap();
    let state = tempfile::tempdir().unwrap();
    let file = state_file(state.path(), &root);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let watches = json!([
        {"expression": "x + y", "pinned": true, "format": {"hex": true}},
        "len(items)",
        42,
    ]);
    let before = json!({
        "version": 3,
        "breakpoints": [
            {"path": "a.py", "line": 2, "condition": "y > 1", "hitCondition": "3",
             "logMessage": "y is {y}"},
        ],
        "watches": watches,
        "exceptionFilters": ["raised"],
    });
    fs::write(&file, serde_json::to_vec(&before).unwrap()).unwrap();

    let state_dir = state.path().to_owned();
    let store = cx.update(|cx| {
        set_state_dir(cx, Some(state_dir));
        BreakpointStore::for_root(&root, cx)
    });
    assert_eq!(
        store.read_with(cx, |store, _| store.watches().cloned()),
        Some(watches.clone())
    );

    // Changes spaced closer than the rest are written once, a rest after
    // the last.
    let path = root.join("a.py");
    let step = WRITE_REST / 2 + Duration::from_millis(50);
    for row in [2, 0] {
        store.update(cx, |store, cx| store.toggle(&path, row, cx));
        cx.executor().advance_clock(step);
        cx.run_until_parked();
        assert_eq!(read_json(&file), before, "not written yet");
    }
    cx.executor()
        .advance_clock(WRITE_REST - step + Duration::from_millis(1));
    cx.run_until_parked();

    let after = read_json(&file);
    assert_eq!(after["watches"], watches, "watches kept as they were");
    assert_eq!(after["version"], json!(3));
    assert_eq!(after["exceptionFilters"], json!(["raised"]));
    assert_eq!(
        after["breakpoints"],
        json!([
            {"path": "a.py", "line": 1, "enabled": true},
            {"path": "a.py", "line": 2, "enabled": true, "condition": "y > 1",
             "hitCondition": "3", "logMessage": "y is {y}"},
            {"path": "a.py", "line": 3, "enabled": true},
        ])
    );
    let names: Vec<_> = (fs::read_dir(file.parent().unwrap()).unwrap())
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, [file.file_name().unwrap()], "no temporary file left");
}

#[gpui::test]
fn a_file_reloaded_from_disk_keeps_its_breakpoints_on_the_right_rows(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    for row in [1, 2, 4] {
        toggle(&mut o, row);
    }
    disable(&mut o, 4);
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let _subscription = o.cx.update(|_, cx| {
        let events = events.clone();
        cx.subscribe(&o.store, move |_, event: &BreakpointEvent, _| {
            events.borrow_mut().push(event.clone())
        })
    });

    // A row added above, `one` changed in place, and rows changed below:
    // one edit from the first row to the last, by the buffer's reload.
    rewrite(&path, "new\nzero\nONE\ntwo\nthree\nfour\nFIVE\nsix\n");
    let panel = o
        .workbench
        .read_with(o.cx, |workbench, _| workbench.panel().clone());
    panel.update(o.cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(path.clone())], cx)
    });
    draw(o.cx);
    assert_eq!(text(&o), "new\nzero\nONE\ntwo\nthree\nfour\nFIVE\nsix\n");
    assert_eq!(rows(&mut o, &path), [(2, true), (3, true), (5, false)]);
    assert_eq!(row_text(&o, 3), "two");
    assert_eq!(row_text(&o, 5), "four");
    assert_eq!(
        *events.borrow(),
        [
            BreakpointEvent::Moved(path.clone()),
            BreakpointEvent::Saved(path.clone())
        ]
    );

    // The rows match the disk's: written back.
    rest(o.cx);
    let root = dunce::canonicalize(o.dir.path()).unwrap();
    let lines: Vec<_> = (read_json(&state_file(state.path(), &root))["breakpoints"]
        .as_array()
        .unwrap()
        .iter())
    .map(|breakpoint| breakpoint["line"].as_u64().unwrap())
    .collect();
    assert_eq!(lines, [3, 4, 6]);

    // And they're anchored in the reloaded text: the next edit moves them.
    keys("g g d d", o.cx);
    assert_eq!(rows(&mut o, &path), [(1, true), (2, true), (4, false)]);
}

#[gpui::test]
fn closing_the_file_keeps_its_rows(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);
    keys("g g shift-o escape", o.cx);
    save(&mut o);
    let pane = o
        .workbench
        .read_with(o.cx, |workbench, _| workbench.pane().clone());
    o.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active(window, cx)));
    draw(o.cx);
    assert_eq!(rows(&mut o, &path), [(3, true)]);

    // Not anchored now: a toggle by row, and the rows stay as they are
    // while it's closed.
    toggle(&mut o, 0);
    assert_eq!(rows(&mut o, &path), [(0, true), (3, true)]);
    let store = o.store.clone();
    store.update(o.cx, |store, cx| store.remove_all(cx));
    assert_eq!(rows(&mut o, &path), []);
    assert!(store.read_with(o.cx, |store, _| store.files().is_empty()));
}

#[gpui::test]
fn closing_unsaved_edits_puts_its_rows_back(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);
    toggle(&mut o, 4);
    keys("g g shift-o escape j j j j d d", o.cx);
    assert_eq!(text(&o), "\nzero\none\ntwo\nfour\nfive\n");
    assert_eq!(rows(&mut o, &path), [(3, true), (4, true)]);

    // Discarded: on `two` and `four` in the file on disk, rows 2 and 4.
    let pane = o
        .workbench
        .read_with(o.cx, |workbench, _| workbench.pane().clone());
    o.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active(window, cx)));
    draw(o.cx);
    o.cx.simulate_prompt_answer("Discard");
    draw(o.cx);
    assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
    assert_eq!(rows(&mut o, &path), [(2, true), (4, true)]);
    rest(o.cx);
    let root = dunce::canonicalize(o.dir.path()).unwrap();
    let written = read_json(&state_file(state.path(), &root));
    assert_eq!(
        written["breakpoints"],
        json!([
            {"path": "a.txt", "line": 3, "enabled": true},
            {"path": "a.txt", "line": 5, "enabled": true},
        ])
    );
}

#[gpui::test]
fn a_window_holds_its_store_weakly_and_the_next_window_gets_it(cx: &mut TestAppContext) {
    let mut o = opened(cx, TEXT, None);
    let root = dunce::canonicalize(o.dir.path()).unwrap();
    let path = o.path.clone();
    toggle(&mut o, 2);
    let store = o.store.downgrade();
    let Opened {
        workbench,
        editor,
        store: held,
        cx: window,
        ..
    } = o;
    drop((workbench, editor, held));

    // Only the app's table of stores holds it, not the window: a test
    // app's observers outlive it, and a strong handle there leaks.
    let id = store.upgrade().map(|store| store.entity_id());
    assert!(id.is_some(), "kept for the app's life");
    window.update(|window, _| window.remove_window());
    window.run_until_parked();

    // The next window on the root gets the same store, its rows kept.
    let (_, _, again, window) = open_window(cx, &root, &path, None);
    assert_eq!(Some(again.entity_id()), id);
    assert_eq!(rows_of(&again, &path, window), [(2, true)]);
}

/// The state file's breakpoints under `state`, as (path, line, enabled).
fn written(state: &Path, o: &Opened) -> Vec<(String, u64, bool)> {
    let root = dunce::canonicalize(o.dir.path()).unwrap();
    let json = read_json(&state_file(state, &root));
    (json["breakpoints"].as_array().unwrap().iter())
        .map(|breakpoint| {
            (
                breakpoint["path"].as_str().unwrap().to_owned(),
                breakpoint["line"].as_u64().unwrap(),
                breakpoint["enabled"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn line(path: &str, line: u64) -> (String, u64, bool) {
    (path.to_owned(), line, true)
}

/// Closes the active tab, discarding its unsaved edits.
fn close_discarding(o: &mut Opened) {
    let pane = o
        .workbench
        .read_with(o.cx, |workbench, _| workbench.pane().clone());
    o.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active(window, cx)));
    draw(o.cx);
    o.cx.simulate_prompt_answer("Discard");
    draw(o.cx);
}

fn buffer_of(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> fe_buffer::BufferId {
    editor.read_with(cx, |editor, _| editor.model().buffer().id())
}

/// The breakpoints of `path` in `buffer`, as (row from 0, enabled).
fn rows_in(
    store: &Entity<BreakpointStore>,
    path: &Path,
    buffer: fe_buffer::BufferId,
    cx: &mut VisualTestContext,
) -> Vec<(u32, bool)> {
    store.read_with(cx, |store, _| {
        (store.breakpoints_in(path, buffer).into_iter())
            .map(|breakpoint| (breakpoint.line - 1, breakpoint.enabled))
            .collect()
    })
}

#[gpui::test]
fn a_write_holds_the_lines_on_disk_not_unsaved_rows(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);
    rest(o.cx);
    assert_eq!(written(state.path(), &o), [line("a.txt", 3)]);

    // A row typed above it, unsaved, then another file's change written:
    // a.txt's line on disk stays as it is.
    keys("g g shift-o escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(3, true)]);
    let other = o.dir.path().join("b.txt");
    let store = o.store.clone();
    store.update(o.cx, |store, cx| store.toggle(&other, 0, cx));
    rest(o.cx);
    assert_eq!(
        written(state.path(), &o),
        [line("a.txt", 3), line("b.txt", 1)]
    );

    // Edits while a write waits out its rest: the line on disk too.
    toggle(&mut o, 5);
    keys("g g shift-o escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(4, true), (6, true)]);
    rest(o.cx);
    assert_eq!(
        written(state.path(), &o),
        [line("a.txt", 3), line("a.txt", 5), line("b.txt", 1)]
    );

    // Saved: the rows are the lines on disk now.
    save(&mut o);
    rest(o.cx);
    assert_eq!(
        written(state.path(), &o),
        [line("a.txt", 5), line("a.txt", 7), line("b.txt", 1)]
    );
}

#[gpui::test]
fn saving_under_a_new_path_leaves_the_old_files_lines(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);
    keys("g g shift-o escape g g shift-o escape", o.cx);
    assert_eq!(rows(&mut o, &path), [(4, true)]);

    // Saved as b.txt: a.txt, unchanged on disk, keeps its line, and the
    // save isn't taken as one of it.
    let new = path.with_file_name("b.txt");
    let (workbench, editor) = (o.workbench.clone(), o.editor.clone());
    let target = new.clone();
    workbench.update_in(o.cx, |workbench, window, cx| {
        workbench.save_editor_as(&editor, target, window, cx)
    });
    draw(o.cx);
    assert_eq!(fs::read_to_string(&new).unwrap(), format!("\n\n{TEXT}"));
    assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
    assert_eq!(rows(&mut o, &path), [(2, true)]);
    assert_eq!(rows(&mut o, &new), []);
    rest(o.cx);
    assert_eq!(written(state.path(), &o), [line("a.txt", 3)]);

    // And the new file's breakpoints follow the editor now.
    toggle_path(&mut o, &new, 4);
    keys("g g d d", o.cx);
    assert_eq!(rows(&mut o, &new), [(3, true)]);
    assert_eq!(rows(&mut o, &path), [(2, true)]);
}

fn toggle_path(o: &mut Opened, path: &Path, row: u32) {
    let store = o.store.clone();
    store.update(o.cx, |store, cx| store.toggle(path, row, cx));
}

#[gpui::test]
fn a_save_landing_after_more_edits_takes_the_text_it_saved(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);

    // A save of the text with one row above, landing after a second.
    keys("g g shift-o escape", o.cx);
    let saving = o
        .editor
        .read_with(o.cx, |editor, _| editor.model().buffer().save_snapshot());
    fs::write(&path, format!("\n{TEXT}")).unwrap();
    keys("g g shift-o escape", o.cx);
    let stamp = fe_buffer::DiskStamp::from_metadata(&fs::metadata(&path).unwrap()).unwrap();
    o.editor.update(o.cx, |editor, cx| {
        (editor.model_mut().buffer_mut()).did_save_snapshot(&saving, Some(stamp));
        cx.notify();
    });
    draw(o.cx);
    assert_eq!(rows(&mut o, &path), [(4, true)]);
    rest(o.cx);
    assert_eq!(written(state.path(), &o), [line("a.txt", 4)]);

    // Discarded: on `two` in the text saved.
    close_discarding(&mut o);
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("\n{TEXT}"));
    assert_eq!(rows(&mut o, &path), [(3, true)]);
}

#[gpui::test]
fn a_buffer_first_seen_unsaved_places_the_lines_on_disk_in_it(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);
    toggle(&mut o, 4);

    // As a restored backup is: text not on disk when the store sees it.
    let (store, editor) = (o.store.clone(), o.editor.clone());
    store.update(o.cx, |store, cx| store.detach(&editor, cx));
    keys("g g shift-o escape", o.cx);
    store.update(o.cx, |store, cx| store.attach(&editor, cx));
    assert_eq!(rows(&mut o, &path), [(3, true), (5, true)]);
    assert_eq!(row_text(&o, 3), "two");

    // Its edits move them, and discarding puts them back on disk's lines.
    keys("g g d d", o.cx);
    assert_eq!(rows(&mut o, &path), [(2, true), (4, true)]);
    keys("g g shift-o escape", o.cx);
    close_discarding(&mut o);
    assert_eq!(rows(&mut o, &path), [(2, true), (4, true)]);
}

#[gpui::test]
fn undoing_and_redoing_a_reload_keeps_the_breakpoints_apart(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);
    toggle(&mut o, 4);
    disable(&mut o, 4);

    // Only the first and last rows change: one edit spanning the rest.
    rewrite(&path, "ZERO\none\ntwo\nthree\nfour\nFIVE\n");
    let panel = o
        .workbench
        .read_with(o.cx, |workbench, _| workbench.panel().clone());
    panel.update(o.cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(path.clone())], cx)
    });
    draw(o.cx);
    assert_eq!(row_text(&o, 0), "ZERO");
    assert_eq!(rows(&mut o, &path), [(2, true), (4, false)]);

    keys("u", o.cx);
    assert_eq!(text(&o), TEXT);
    assert_eq!(rows(&mut o, &path), [(2, true), (4, false)]);
    keys("ctrl-r", o.cx);
    assert_eq!(row_text(&o, 0), "ZERO");
    assert_eq!(rows(&mut o, &path), [(2, true), (4, false)]);
}

#[gpui::test]
fn two_buffers_of_one_file_each_keep_their_rows(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let path = root.join("a.txt");
    fs::write(&path, TEXT).unwrap();
    let (_, first, store, window) = open_window(cx, &root, &path, Some(state.path()));
    store.update(window, |store, cx| store.toggle(&path, 2, cx));
    let one = buffer_of(&first, window);

    // The file opened apart in a second window: another buffer.
    let (_, second, again, window) = another_window(cx, &root, &path);
    assert_eq!(again.entity_id(), store.entity_id());
    let two = buffer_of(&second, window);
    assert_ne!(one, two, "a buffer each");
    assert_eq!(rows_in(&store, &path, two, window), [(2, true)]);

    // Edited in the second: its rows follow, the first's stay.
    window.simulate_keystrokes("g g shift-o escape");
    draw(window);
    assert_eq!(rows_in(&store, &path, two, window), [(3, true)]);
    assert_eq!(rows_in(&store, &path, one, window), [(2, true)]);

    // One added in the second shows in the first on its line too.
    store.update(window, |store, cx| store.toggle_in(&path, two, 5, cx));
    assert_eq!(rows_in(&store, &path, two, window), [(3, true), (5, true)]);
    assert_eq!(rows_in(&store, &path, one, window), [(2, true), (4, true)]);

    // Saved from the second: its rows are the lines on disk, and the set
    // a session sends.
    window
        .update(|window, cx| second.update(cx, |editor, cx| editor.save(window, cx)))
        .detach();
    draw(window);
    rest(window);
    let lines = || {
        let json = read_json(&state_file(state.path(), &root));
        (json["breakpoints"].as_array().unwrap().iter())
            .map(|breakpoint| breakpoint["line"].as_u64().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(), [4, 6]);
    let sent = |window: &mut VisualTestContext| {
        let enabled = store.read_with(window, |store, _| store.enabled(&path));
        enabled.iter().map(|b| b.line).collect::<Vec<_>>()
    };
    assert_eq!(sent(window), [4, 6]);

    // The first, unchanged, now differs from the file on disk: one set
    // on its `one` goes on disk's `one`, and on the second's.
    store.update(window, |store, cx| store.toggle_in(&path, one, 1, cx));
    assert_eq!(
        rows_in(&store, &path, one, window),
        [(1, true), (2, true), (4, true)]
    );
    assert_eq!(
        rows_in(&store, &path, two, window),
        [(2, true), (3, true), (5, true)]
    );
    rest(window);
    assert_eq!(lines(), [3, 4, 6]);

    // And one set in the second after its save, on its `zero`, too.
    store.update(window, |store, cx| store.toggle_in(&path, two, 1, cx));
    assert_eq!(
        rows_in(&store, &path, one, window),
        [(0, true), (1, true), (2, true), (4, true)]
    );
    rest(window);
    assert_eq!(lines(), [2, 3, 4, 6]);

    // Unsaved edits in the second: the session's set stays on disk's.
    window.simulate_keystrokes("g g shift-o escape");
    draw(window);
    assert_eq!(
        rows_in(&store, &path, two, window),
        [(2, true), (3, true), (4, true), (6, true)]
    );
    assert_eq!(sent(window), [2, 3, 4, 6]);
}

#[gpui::test]
fn a_failed_write_is_tried_again_at_quit(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    // Its folder can't be made: a file is in the way.
    let blocker = state.path().join("debug");
    fs::write(&blocker, "").unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    toggle(&mut o, 2);
    rest(o.cx);
    let root = dunce::canonicalize(o.dir.path()).unwrap();
    let file = state_file(state.path(), &root);
    assert!(!file.exists());

    fs::remove_file(&blocker).unwrap();
    o.cx.quit();
    assert_eq!(written(state.path(), &o), [line("a.txt", 3)]);
}

#[gpui::test]
fn quitting_before_the_rest_writes_the_last_change(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    toggle(&mut o, 2);
    rest(o.cx);
    toggle(&mut o, 4);
    o.cx.quit();
    assert_eq!(
        written(state.path(), &o),
        [line("a.txt", 3), line("a.txt", 5)]
    );
}

/// Starts a save of the editor's text as it is now, to land later.
fn start_save(o: &mut Opened) -> fe_buffer::SaveSnapshot {
    let saving = o
        .editor
        .read_with(o.cx, |editor, _| editor.model().buffer().save_snapshot());
    fs::write(&o.path, saving.text().to_string()).unwrap();
    saving
}

/// Lands a save started by [`start_save`].
fn land_save(o: &mut Opened, saving: fe_buffer::SaveSnapshot) {
    let stamp = fe_buffer::DiskStamp::from_metadata(&fs::metadata(&o.path).unwrap()).unwrap();
    o.editor.update(o.cx, |editor, cx| {
        (editor.model_mut().buffer_mut()).did_save_snapshot(&saving, Some(stamp));
        cx.notify();
    });
    draw(o.cx);
}

#[gpui::test]
fn a_save_landing_after_its_row_is_deleted_keeps_it_on_the_text_saved(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let path = o.path.clone();
    toggle(&mut o, 2);
    toggle(&mut o, 3);
    disable(&mut o, 3);
    keys("g g shift-o escape", o.cx);
    let saving = start_save(&mut o);

    // `two`'s row deleted before the save lands: it shows on `three`'s.
    keys("4 G d d", o.cx);
    assert_eq!(rows(&mut o, &path), [(3, true)]);
    land_save(&mut o, saving);
    rest(o.cx);
    let lines = vec![line("a.txt", 4), ("a.txt".to_owned(), 5, false)];
    assert_eq!(written(state.path(), &o), lines);

    // Discarded: both on their own rows of the text saved.
    close_discarding(&mut o);
    assert_eq!(rows(&mut o, &path), [(3, true), (4, false)]);
}

#[gpui::test]
fn a_save_landing_after_its_row_is_joined_keeps_it_on_the_text_saved(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    toggle(&mut o, 2);
    keys("g g shift-o escape", o.cx);
    let saving = start_save(&mut o);

    // `two` joined onto `one` before the save lands.
    keys("3 G shift-j", o.cx);
    assert_eq!(row_text(&o, 2), "one two");
    land_save(&mut o, saving);
    rest(o.cx);
    assert_eq!(written(state.path(), &o), [line("a.txt", 4)]);
}

#[gpui::test]
fn a_change_made_while_a_write_fails_is_written_after_it(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let blocker = state.path().join("debug");
    fs::write(&blocker, "").unwrap();
    let mut o = opened(cx, TEXT, Some(state.path()));
    let store = o.store.clone();
    let release = store.update(o.cx, |store, _| store.hold_next_write());
    toggle(&mut o, 2);
    rest(o.cx);

    // A change while that write runs; then it fails.
    toggle(&mut o, 4);
    release.send(()).unwrap();
    draw(o.cx);
    let root = dunce::canonicalize(o.dir.path()).unwrap();
    assert!(!state_file(state.path(), &root).exists());

    // The newer change is written once the folder can be made.
    fs::remove_file(&blocker).unwrap();
    rest(o.cx);
    assert_eq!(
        written(state.path(), &o),
        [line("a.txt", 3), line("a.txt", 5)]
    );
}
