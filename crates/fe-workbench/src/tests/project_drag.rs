//! Dragging the project tree's rows onto a folder (T1753): a drop moves
//! them there as a cut and paste would, with Option held copies them, and
//! does nothing into itself, in the folder they're in or with a name taken;
//! a collapsed folder under the pointer opens, and escape stops the drag.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};
use serde_json::json;

use super::{lsp::Lsp, open_root, panel, rows};
use crate::{ProjectPanel, Workbench};

/// Files at `paths` (relative, `/`-separated) holding their own path, a
/// path ending in `/` a folder, under a temporary folder; and its
/// canonical path.
fn project(paths: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for rel in paths {
        let path = root.join(rel);
        if rel.ends_with('/') {
            fs::create_dir_all(path).unwrap();
            continue;
        }
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, rel).unwrap();
    }
    (dir, root)
}

/// A workbench on `root`, in an active window, its project tree focused.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<ProjectPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("cmd-shift-e");
    cx.run_until_parked();
    let panel = panel(&workbench, cx);
    (workbench, panel, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// The middle of row `name`, where the last frame laid it out.
fn at(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) -> Point<Pixels> {
    let ix = (rows(panel, cx).iter())
        .position(|(_, row)| row == name)
        .unwrap_or_else(|| panic!("no row {name}"));
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    bounds.center()
}

/// A point in the list below its last row.
fn below_rows(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Point<Pixels> {
    let list = panel.read_with(cx, |panel, _| panel.list_bounds());
    point(list.center().x, list.bottom() - px(8.))
}

/// Clicks row `name` with `modifiers`.
fn click(
    panel: &Entity<ProjectPanel>,
    name: &str,
    modifiers: Modifiers,
    cx: &mut VisualTestContext,
) {
    let center = at(panel, name, cx);
    cx.simulate_click(center, modifiers);
    draw(cx);
}

/// Presses on row `from`, moves past the drag threshold, then to `to`.
fn hover(panel: &Entity<ProjectPanel>, from: &str, to: Point<Pixels>, cx: &mut VisualTestContext) {
    let none = Modifiers::none();
    let from = at(panel, from, cx);
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(point(from.x + px(4.), from.y), MouseButton::Left, none);
    draw(cx);
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
}

/// Releases the button at `to`, with `modifiers` held.
fn release(to: Point<Pixels>, modifiers: Modifiers, cx: &mut VisualTestContext) {
    cx.simulate_mouse_up(to, MouseButton::Left, modifiers);
    draw(cx);
}

/// Drags row `from` to `to` and drops it there, `modifiers` held.
fn drag(
    panel: &Entity<ProjectPanel>,
    from: &str,
    to: Point<Pixels>,
    modifiers: Modifiers,
    cx: &mut VisualTestContext,
) {
    hover(panel, from, to, cx);
    release(to, modifiers, cx);
}

/// Drags row `from` onto row `to` and drops it there.
fn drag_onto(panel: &Entity<ProjectPanel>, from: &str, to: &str, cx: &mut VisualTestContext) {
    let to = at(panel, to, cx);
    drag(panel, from, to, Modifiers::none(), cx);
}

/// The toasts shown, each as its message.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// The folder a drop would go in now, from the root, and the rows drawn as
/// where it would go, and whether the root's row and list are.
fn target(
    panel: &Entity<ProjectPanel>,
    root: &Path,
    cx: &mut VisualTestContext,
) -> (Option<PathBuf>, Vec<String>, bool) {
    let target = panel.read_with(cx, |panel, _| panel.drop_target_path());
    let target = target.map(|path| path.strip_prefix(root).unwrap().to_owned());
    let (rows, root) = panel.read_with(cx, |panel, _| panel.drawn_drop());
    (target, rows, root)
}

/// The rows' names, depth-first as the list shows them.
fn names(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    rows(panel, cx).into_iter().map(|(_, name)| name).collect()
}

fn selected(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    let rows = rows(panel, cx);
    let primary = panel.read_with(cx, |panel, _| panel.selected_row());
    primary.map(|ix| rows[ix].1.clone())
}

#[gpui::test]
fn drop_on_folder_moves_the_row(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "f/g.txt", "a.txt", "b.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    assert_eq!(names(&panel, cx), ["d", "f", "a.txt", "b.txt"]);

    // Over the collapsed folder, it's the target: its row is tinted.
    let d = at(&panel, "d", cx);
    hover(&panel, "a.txt", d, cx);
    assert!(panel.read_with(cx, |panel, _| panel.is_dragging()));
    assert_eq!(
        target(&panel, &root, cx),
        (Some("d".into()), vec!["d".into()], false)
    );
    // Not a click: a.txt isn't opened, nor selected.
    assert_eq!(selected(&panel, cx), None);

    release(d, Modifiers::none(), cx);
    assert!(root.join("d/a.txt").is_file());
    assert!(!root.join("a.txt").exists());
    // Shown and selected where it went; the drag is over.
    assert_eq!(names(&panel, cx), ["d", "a.txt", "e.txt", "f", "b.txt"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("a.txt"));
    assert!(!panel.read_with(cx, |panel, _| panel.is_dragging()));
    assert_eq!(target(&panel, &root, cx), (None, Vec::new(), false));
    assert_eq!(toasts(&workbench, cx), Vec::<String>::new());

    // Below the rows is the root: the list is tinted, and d/a.txt drops
    // there.
    let below = below_rows(&panel, cx);
    hover(&panel, "a.txt", below, cx);
    assert_eq!(
        target(&panel, &root, cx),
        (Some("".into()), Vec::new(), true)
    );
    release(below, Modifiers::none(), cx);
    assert!(root.join("a.txt").is_file());
    assert!(!root.join("d/a.txt").exists());

    assert_eq!(names(&panel, cx), ["d", "e.txt", "f", "a.txt", "b.txt"]);

    // A folder moves with what's in it, and stays expanded in an expanded
    // folder.
    click(&panel, "f", Modifiers::none(), cx);
    drag_onto(&panel, "d", "f", cx);
    assert!(root.join("f/d/e.txt").is_file());
    let moved = ["f", "d", "e.txt", "g.txt", "a.txt", "b.txt"];
    assert_eq!(names(&panel, cx), moved);
    assert_eq!(selected(&panel, cx).as_deref(), Some("d"));
}

#[gpui::test]
fn drop_on_file_uses_its_parent(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "d/f.txt", "a.txt"]);
    let (_, panel, cx) = open(cx, &root);
    click(&panel, "d", Modifiers::none(), cx);
    assert_eq!(names(&panel, cx), ["d", "e.txt", "f.txt", "a.txt"]);

    // Over a file, its folder is the target: its row and those under it.
    let f = at(&panel, "f.txt", cx);
    hover(&panel, "a.txt", f, cx);
    let tinted = vec!["d".into(), "e.txt".into(), "f.txt".into()];
    assert_eq!(target(&panel, &root, cx), (Some("d".into()), tinted, false));
    release(f, Modifiers::none(), cx);
    assert!(root.join("d/a.txt").is_file());
    assert!(!root.join("a.txt").exists());
    assert_eq!(fs::read_to_string(root.join("d/f.txt")).unwrap(), "d/f.txt");
}

#[gpui::test]
fn option_drop_copies(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/a.txt", "a.txt", "b/c.txt"]);
    let (_, panel, cx) = open(cx, &root);
    let alt = Modifiers::alt();

    // The name taken in d: the copy is a.txt's ` copy`, nothing replaced.
    let d = at(&panel, "d", cx);
    drag(&panel, "a.txt", d, alt, cx);
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a.txt");
    assert_eq!(fs::read_to_string(root.join("d/a.txt")).unwrap(), "d/a.txt");
    assert_eq!(
        fs::read_to_string(root.join("d/a copy.txt")).unwrap(),
        "a.txt"
    );
    // Shown and selected.
    assert_eq!(selected(&panel, cx).as_deref(), Some("a copy.txt"));

    // A folder is copied with what's in it.
    let d = at(&panel, "d", cx);
    drag(&panel, "b", d, alt, cx);
    assert!(root.join("b/c.txt").is_file());
    assert_eq!(
        fs::read_to_string(root.join("d/b/c.txt")).unwrap(),
        "b/c.txt"
    );
}

#[gpui::test]
fn drop_into_itself_does_nothing_and_toasts(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/s/t.txt", "a.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    click(&panel, "d", Modifiers::none(), cx);
    assert_eq!(names(&panel, cx), ["d", "s", "a.txt"]);

    // Over a folder inside it: no target.
    let s = at(&panel, "s", cx);
    hover(&panel, "d", s, cx);
    assert_eq!(target(&panel, &root, cx), (None, Vec::new(), false));
    release(s, Modifiers::none(), cx);
    assert!(root.join("d/s/t.txt").is_file());
    assert!(!root.join("d/s/d").exists());
    let message = "“d” can’t be moved into itself or a folder inside it.";
    assert_eq!(toasts(&workbench, cx), [message]);

    // Onto its own row, with Option held: a copy says so.
    let d = at(&panel, "d", cx);
    drag(&panel, "d", d, Modifiers::alt(), cx);
    assert!(!root.join("d/d").exists());
    let copy = "“d” can’t be copied into itself or a folder inside it.";
    assert!(toasts(&workbench, cx).contains(&copy.to_string()));
}

#[gpui::test]
fn drop_on_current_parent_is_a_no_op(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "d/f.txt", "a.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    click(&panel, "d", Modifiers::none(), cx);

    // Onto its folder's row, or a file beside it: no target, no move.
    let d = at(&panel, "d", cx);
    hover(&panel, "e.txt", d, cx);
    assert_eq!(target(&panel, &root, cx), (None, Vec::new(), false));
    release(d, Modifiers::none(), cx);
    drag_onto(&panel, "e.txt", "f.txt", cx);
    // A root-level file below the rows, with Option held too: no copy.
    let below = below_rows(&panel, cx);
    drag(&panel, "a.txt", below, Modifiers::alt(), cx);

    assert_eq!(names(&panel, cx), ["d", "e.txt", "f.txt", "a.txt"]);
    let mut on_disk: Vec<_> = (fs::read_dir(root.join("d")).unwrap())
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort();
    assert_eq!(on_disk, ["e.txt", "f.txt"]);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2, "d and a.txt");
    // Each says it can't go there, as a paste into itself does.
    let toasts = toasts(&workbench, cx);
    for said in [
        "“e.txt” can’t be moved into itself or a folder inside it.",
        "“a.txt” can’t be copied into itself or a folder inside it.",
    ] {
        assert!(toasts.contains(&said.to_string()), "{said} in {toasts:?}");
    }
}

#[cfg(unix)]
#[gpui::test]
fn drop_on_an_unlistable_folder_does_nothing_and_toasts(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let (_dir, root) = project(&["locked/", "a.txt"]);
    let mode = |mode| fs::Permissions::from_mode(mode);
    fs::set_permissions(root.join("locked"), mode(0o000)).unwrap();
    let (workbench, panel, cx) = open(cx, &root);
    // Its listing fails.
    click(&panel, "locked", Modifiers::none(), cx);
    let error = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().unwrap();
        let id = tree.find(root.join("locked")).unwrap();
        tree.row(id).unwrap().error.is_some()
    });
    fs::set_permissions(root.join("locked"), mode(0o755)).unwrap();
    assert!(error, "the folder couldn't be read");

    let locked = at(&panel, "locked", cx);
    hover(&panel, "a.txt", locked, cx);
    assert_eq!(target(&panel, &root, cx), (None, Vec::new(), false));
    release(locked, Modifiers::none(), cx);
    assert!(root.join("a.txt").is_file());
    assert!(!root.join("locked/a.txt").exists());
    let said = "“a.txt” can’t be moved into itself or a folder inside it.";
    assert_eq!(toasts(&workbench, cx), [said]);
}

#[gpui::test]
fn selection_drags_together(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "c.txt", Modifiers::secondary_key(), cx);

    // A row outside the selection is dragged alone.
    drag_onto(&panel, "b.txt", "d", cx);
    assert!(root.join("d/b.txt").is_file());
    assert!(root.join("a.txt").is_file() && root.join("c.txt").is_file());

    // A selected row drags the selection, a folder's contents with it.
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "c.txt", Modifiers::secondary_key(), cx);
    drag_onto(&panel, "c.txt", "d", cx);
    for moved in ["d/a.txt", "d/b.txt", "d/c.txt"] {
        assert!(root.join(moved).is_file(), "{moved}");
    }
    assert!(!root.join("a.txt").exists() && !root.join("c.txt").exists());

    // A name taken in the folder isn't moved, and says so; the rest are.
    fs::write(root.join("e.txt"), "e.txt").unwrap();
    fs::write(root.join("x.txt"), "x.txt").unwrap();
    let made = [root.join("e.txt"), root.join("x.txt")].map(fe_project::FsChange::Path);
    panel.update(cx, |panel, cx| panel.apply_fs_changes(&made, cx));
    // d shut, so the row named e.txt is the root's.
    click(&panel, "d", Modifiers::none(), cx);
    assert_eq!(names(&panel, cx), ["d", "e.txt", "x.txt"]);
    click(&panel, "e.txt", Modifiers::none(), cx);
    click(&panel, "x.txt", Modifiers::secondary_key(), cx);
    drag_onto(&panel, "x.txt", "d", cx);
    assert!(root.join("d/x.txt").is_file());
    assert_eq!(fs::read_to_string(root.join("e.txt")).unwrap(), "e.txt");
    assert_eq!(fs::read_to_string(root.join("d/e.txt")).unwrap(), "d/e.txt");
    assert!(!root.join("d/e copy.txt").exists());
    let taken = "“e.txt” wasn’t moved: “d” has a file or folder of that name already.";
    assert_eq!(toasts(&workbench, cx), [taken]);
}

#[gpui::test]
fn collapsed_folder_opens_on_hover(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "f/g.txt", "a.txt"]);
    let (_, panel, cx) = open(cx, &root);

    // Passing over f on the way: it stays shut.
    let (d, f) = (at(&panel, "d", cx), at(&panel, "f", cx));
    hover(&panel, "a.txt", f, cx);
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.simulate_mouse_move(d, MouseButton::Left, Modifiers::none());
    draw(cx);
    cx.executor().advance_clock(Duration::from_millis(400));
    draw(cx);
    assert_eq!(names(&panel, cx), ["d", "f", "a.txt"], "neither yet");
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    assert_eq!(names(&panel, cx), ["d", "e.txt", "f", "a.txt"]);

    // Its row now: a drop on e.txt goes in d.
    let e = at(&panel, "e.txt", cx);
    cx.simulate_mouse_move(e, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert_eq!(target(&panel, &root, cx).0, Some("d".into()));

    // Escape stops the drag: nothing drops, and no folder opens.
    let f = at(&panel, "f", cx);
    cx.simulate_mouse_move(f, MouseButton::Left, Modifiers::none());
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!panel.read_with(cx, |panel, _| panel.is_dragging()));
    assert_eq!(target(&panel, &root, cx), (None, Vec::new(), false));
    cx.executor().advance_clock(Duration::from_secs(1));
    draw(cx);
    release(f, Modifiers::none(), cx);
    assert!(root.join("a.txt").is_file());
    assert_eq!(names(&panel, cx), ["d", "e.txt", "f", "a.txt"]);
}

#[gpui::test]
fn editors_follow_a_dragged_rename(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "s/t.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    // Opens a.txt and t.txt, in s.
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "s", Modifiers::none(), cx);
    click(&panel, "t.txt", Modifiers::none(), cx);
    let editors = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            let tabs = pane.tabs().iter();
            let mut paths: Vec<PathBuf> = tabs.filter_map(|tab| tab.path.clone()).collect();
            paths.sort();
            paths
        })
    };
    assert_eq!(editors(cx), [root.join("a.txt"), root.join("s/t.txt")]);

    drag_onto(&panel, "a.txt", "d", cx);
    drag_onto(&panel, "s", "d", cx);
    assert_eq!(editors(cx), [root.join("d/a.txt"), root.join("d/s/t.txt")]);
}

/// A rust-analyzer fake asked for its edits before a `*.rs` file's rename.
fn participant() -> Script {
    let mut result = fe_lsp::fake::initialize_result(None);
    let filters = json!({"filters": [{"scheme": "file", "pattern": {"glob": "**/*.rs"}}]});
    result.capabilities.workspace = Some(
        serde_json::from_value(json!({"fileOperations": {
            "didRename": filters, "willRename": filters,
        }}))
        .unwrap(),
    );
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

#[gpui::test]
fn lsp_will_rename_runs_for_a_drop(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), "mod util;\nfn main() {}\n").unwrap();
    fs::write(s.path("src/util.rs"), "pub fn h() {}\n").unwrap();
    fs::create_dir(s.path("src/sub")).unwrap();
    // `mod util;` to `#[path = "sub/util.rs"] mod util;` in main.rs.
    let edit = json!([{"range": {"start": {"line": 0, "character": 0},
        "end": {"line": 0, "character": 0}}, "newText": "#[path = \"sub/util.rs\"] "}]);
    let changes = json!({"changes": {s.uri("src/main.rs").as_str(): edit}});
    s.fake.set_reply(
        "workspace/willRenameFiles",
        Scripted {
            reply: Reply::Result(changes),
            delay: Duration::ZERO,
        },
    );
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());

    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-shift-e");
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let changed = [
        fe_project::FsChange::Path(s.path("src/util.rs")),
        fe_project::FsChange::Path(s.path("src/sub")),
    ];
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&changed, cx)));
    let util = s.path("src/util.rs");
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(util, cx)));
    s.draw();

    drag_onto(&panel, "util.rs", "sub", &mut s.cx);
    s.until("the move", |s| s.path("src/sub/util.rs").is_file());
    assert!(!s.path("src/util.rs").exists());
    let request = (s.fake.records().into_iter()).find_map(|record| match record {
        Record::Request { method, params, .. } if method == "workspace/willRenameFiles" => {
            Some(params)
        }
        _ => None,
    });
    let (old, new) = (s.uri("src/util.rs"), s.uri("src/sub/util.rs"));
    assert_eq!(
        request,
        Some(json!({"files": [{"oldUri": old.as_str(), "newUri": new.as_str()}]}))
    );
    s.until("the edit", |s| {
        let editor = s
            .workbench
            .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned());
        let text =
            editor.map(|editor| editor.read_with(&s.cx, |view, _| view.model().text().to_string()));
        text.as_deref() == Some("#[path = \"sub/util.rs\"] mod util;\nfn main() {}\n")
    });
}

/// Carried from T1752 (F6): a move that goes nowhere still has the panel
/// redrawn, the rows selected with the primary no longer selected.
#[gpui::test]
fn navigation_redraws_a_cleared_selection(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/", "a.txt", "b.txt"]);
    let (_, panel, cx) = open(cx, &root);
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let _seen = cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&panel, move |_, _| notified.set(notified.get() + 1))
    });
    // Left on a root-level row, then Right on a file and on an empty
    // folder, expanded: none moves the selection anywhere.
    for (first, keys) in [("a.txt", "left"), ("a.txt", "right"), ("d", "right")] {
        click(&panel, first, Modifiers::none(), cx);
        let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.simulate_keystrokes("shift-down");
        draw(cx);
        let drawn = panel.read_with(cx, |panel, _| panel.drawn_selection().len());
        assert_eq!(drawn, 2, "{first} and the row below");
        let before = notified.get();
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        assert!(notified.get() > before, "{keys} on {first} redraws");
        draw(cx);
        let drawn = panel.read_with(cx, |panel, _| panel.drawn_selection().len());
        assert_eq!(drawn, 1, "{keys} on {first}");
    }
}

/// Carried from T1752 (F8): selecting from none says the count.
#[gpui::test]
fn selecting_from_empty_announces_the_count(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (_, panel, cx) = open(cx, &root);
    let announced = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.announced());
    let deselect = |cx: &mut VisualTestContext| {
        click(&panel, "a.txt", Modifiers::none(), cx);
        click(&panel, "a.txt", Modifiers::secondary_key(), cx);
        assert_eq!(announced(cx).as_deref(), Some("0 items selected"));
    };
    deselect(cx);
    click(&panel, "b.txt", Modifiers::secondary_key(), cx);
    assert_eq!(announced(cx).as_deref(), Some("1 item selected"));
    deselect(cx);
    click(&panel, "b.txt", Modifiers::shift(), cx);
    assert_eq!(announced(cx).as_deref(), Some("1 item selected"));
    deselect(cx);
    let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_keystrokes("shift-down");
    draw(cx);
    assert_eq!(announced(cx).as_deref(), Some("1 item selected"));
}

/// A selected folder `d` and `d/e.txt` in it, dragged by `e.txt` onto `g`
/// with `modifiers` held, in a project of its own: its root.
fn drag_selected_child(cx: &mut TestAppContext, modifiers: Modifiers) -> tempfile::TempDir {
    let (dir, root) = project(&["d/e.txt", "d/k.txt", "g/"]);
    let (_, panel, cx) = open(cx, &root);
    click(&panel, "d", Modifiers::none(), cx);
    assert_eq!(names(&panel, cx), ["d", "e.txt", "k.txt", "g"]);
    click(&panel, "e.txt", Modifiers::secondary_key(), cx);
    let g = at(&panel, "g", cx);
    drag(&panel, "e.txt", g, modifiers, cx);
    dir
}

#[gpui::test]
fn a_selected_childs_drag_carries_its_folder(cx: &mut TestAppContext) {
    // Dragging the row drags the folder, the row in it.
    let dir = drag_selected_child(cx, Modifiers::none());
    let root = dunce::canonicalize(dir.path()).unwrap();
    assert!(root.join("g/d/e.txt").is_file() && root.join("g/d/k.txt").is_file());
    assert!(!root.join("d").exists() && !root.join("g/e.txt").exists());
}

#[gpui::test]
fn a_selected_childs_option_drag_copies_its_folder(cx: &mut TestAppContext) {
    let dir = drag_selected_child(cx, Modifiers::alt());
    let root = dunce::canonicalize(dir.path()).unwrap();
    assert!(root.join("g/d/e.txt").is_file() && root.join("g/d/k.txt").is_file());
    assert!(root.join("d/e.txt").is_file() && !root.join("g/e.txt").exists());
}

/// Presses on row `from` and moves once, past the drag threshold, straight
/// to `to`: the drag starts there.
fn start_at(
    panel: &Entity<ProjectPanel>,
    from: &str,
    to: Point<Pixels>,
    cx: &mut VisualTestContext,
) {
    let from = at(panel, from, cx);
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn a_drag_starting_over_a_folder_targets_it_at_once(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt"]);
    let (_, panel, cx) = open(cx, &root);

    // One move, onto d, and the pointer stays: d is the target, and opens.
    let d = at(&panel, "d", cx);
    start_at(&panel, "a.txt", d, cx);
    assert!(panel.read_with(cx, |panel, _| panel.is_dragging()));
    assert_eq!(target(&panel, &root, cx).0, Some("d".into()));
    cx.executor().advance_clock(Duration::from_millis(700));
    draw(cx);
    assert_eq!(names(&panel, cx), ["d", "e.txt", "a.txt"]);
    cx.simulate_keystrokes("escape");
    draw(cx);
    release(d, Modifiers::none(), cx);
    assert!(root.join("a.txt").is_file());

    // Escape right after such a start stops it.
    let d = at(&panel, "d", cx);
    start_at(&panel, "a.txt", d, cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!panel.read_with(cx, |panel, _| panel.is_dragging()));
    release(d, Modifiers::none(), cx);
    assert!(root.join("a.txt").is_file() && !root.join("d/a.txt").exists());
}

#[gpui::test]
fn option_escape_cancels_a_copy(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/", "a.txt"]);
    let (_, panel, cx) = open(cx, &root);
    let d = at(&panel, "d", cx);
    hover(&panel, "a.txt", d, cx);
    cx.simulate_modifiers_change(Modifiers::alt());
    cx.simulate_keystrokes("alt-escape");
    draw(cx);
    assert!(!panel.read_with(cx, |panel, _| panel.is_dragging()));
    release(d, Modifiers::alt(), cx);
    assert!(!root.join("d/a.txt").exists());
    assert_eq!(fs::read_dir(root.join("d")).unwrap().count(), 0);
}

#[gpui::test]
fn rows_scrolled_under_a_still_pointer_are_the_target(cx: &mut TestAppContext) {
    let folders: Vec<String> = (0..100).map(|n| format!("d{n:02}/f.txt")).collect();
    let paths: Vec<&str> = folders.iter().map(String::as_str).collect();
    let (_dir, root) = project(&paths);
    let (_, panel, cx) = open(cx, &root);

    let d01 = at(&panel, "d01", cx);
    hover(&panel, "d00", d01, cx);
    assert_eq!(target(&panel, &root, cx).0, Some("d01".into()));
    cx.executor().advance_clock(Duration::from_millis(400));

    // Three rows down, the pointer still: d04 is under it now.
    let height = panel.read_with(cx, |panel, _| panel.row_bounds(0).unwrap().size.height);
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: d01,
        delta: gpui::ScrollDelta::Pixels(point(px(0.), -height * 3.)),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    draw(cx);
    assert_eq!(target(&panel, &root, cx).0, Some("d04".into()));
    let (_, tinted, _) = target(&panel, &root, cx);
    assert_eq!(tinted, ["d04"]);

    // d01's wait was dropped; d04's runs from the scroll.
    cx.executor().advance_clock(Duration::from_millis(400));
    draw(cx);
    assert!(
        !names(&panel, cx).contains(&"f.txt".to_string()),
        "nothing open yet"
    );
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    let rows = rows(&panel, cx);
    let open: Vec<&str> = (rows.windows(2))
        .filter(|pair| pair[1].0 == 1)
        .map(|pair| pair[0].1.as_str())
        .collect();
    assert_eq!(open, ["d04"]);

    // The drop goes where the tint is.
    release(d01, Modifiers::none(), cx);
    assert!(root.join("d04/d00/f.txt").is_file());
    assert!(!root.join("d01/d00").exists());
}
