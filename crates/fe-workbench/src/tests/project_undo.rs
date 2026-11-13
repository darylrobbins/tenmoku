//! Undo and redo of the project tree's file operations (T1754): cmd-z
//! while the tree has focus undoes the last move, rename, create,
//! duplicate or move to the Trash, cmd-shift-z redoes it; an undo whose
//! old path is taken is refused, with a toast, and stays; in the name
//! field the keys are the field's. What goes to the Trash goes to the test
//! trash (`fe_project::fs_ops::testing`), never the user's.

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted};
use fe_project::fs_ops::testing;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use serde_json::{Value, json};

use super::{lsp::Lsp, open_root, panel, rows};
use crate::{ProjectPanel, Workbench};

/// Files at `paths` (relative, `/`-separated) holding their own path, a
/// path ending in `/` a folder, under a temporary folder; and its
/// canonical path.
fn project(paths: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("fe-workbench-undo-")
        .tempdir()
        .unwrap();
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
    cx.update(|window, _| window.activate_window());
    let panel = panel(&workbench, cx);
    focus(&workbench, cx);
    (workbench, panel, cx)
}

/// Focuses the tree, as `dock.show_project` does.
fn focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-e");
    cx.run_until_parked();
}

/// Selects `rel`'s row, its folders expanded.
fn select(panel: &Entity<ProjectPanel>, root: &Path, rel: &str, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.reveal(root.join(rel), cx));
    cx.run_until_parked();
}

/// Runs command `id` where focus is, as a key bound in the tree would.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

/// Types `name` in the name field, and runs it.
fn name(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

/// Presses `keys` with the tree focused.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The tree's rows, by name.
fn names(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    rows(panel, cx).into_iter().map(|(_, name)| name).collect()
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

fn depth(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> (usize, usize) {
    panel.read_with(cx, |panel, _| panel.history_depth())
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    cx.run_until_parked();
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor is open")
}

fn editor_path(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
}

#[gpui::test]
fn undo_a_move(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["app/", "notes.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    open_file(&workbench, &root.join("notes.txt"), cx);
    let editor = active_editor(&workbench, cx);
    focus(&workbench, cx);
    select(&panel, &root, "notes.txt", cx);
    run("project_panel.cut", cx);
    select(&panel, &root, "app", cx);
    run("project_panel.paste", cx);
    assert!(root.join("app/notes.txt").is_file());
    assert_eq!(editor_path(&editor, cx), Some(root.join("app/notes.txt")));

    press("cmd-z", cx);

    assert_eq!(
        fs::read_to_string(root.join("notes.txt")).unwrap(),
        "notes.txt"
    );
    assert!(!root.join("app/notes.txt").exists());
    assert_eq!(editor_path(&editor, cx), Some(root.join("notes.txt")));
    assert_eq!(depth(&panel, cx), (0, 1));
    assert!(names(&panel, cx).contains(&"notes.txt".to_owned()));
}

#[gpui::test]
fn undo_a_rename(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["src/lib.rs", "src/main.rs"]);
    let (workbench, panel, cx) = open(cx, &root);
    open_file(&workbench, &root.join("src/main.rs"), cx);
    let editor = active_editor(&workbench, cx);
    focus(&workbench, cx);
    select(&panel, &root, "src", cx);
    run("project_panel.rename", cx);
    name("core", cx);
    assert!(root.join("core/main.rs").is_file());
    assert_eq!(editor_path(&editor, cx), Some(root.join("core/main.rs")));

    press("cmd-z", cx);

    assert!(root.join("src/main.rs").is_file());
    assert!(!root.join("core").exists());
    assert_eq!(editor_path(&editor, cx), Some(root.join("src/main.rs")));
    // Expanded before, it's expanded after.
    assert_eq!(names(&panel, cx), ["src", "lib.rs", "main.rs"]);
}

#[gpui::test]
fn undo_a_create(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["kept.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    run("project_panel.new_file", cx);
    name("deep/er/new.rs", cx);
    assert!(root.join("deep/er/new.rs").is_file());
    // The new file opened, focused: back to the tree.
    focus(&workbench, cx);

    press("cmd-z", cx);

    // The folders its name made go too, to the Trash.
    assert!(!root.join("deep").exists());
    assert_eq!(names(&panel, cx), ["kept.txt"]);
    let trashed = testing::trashed("deep");
    assert!(
        trashed.iter().any(|path| path.join("er/new.rs").is_file()),
        "{trashed:?}"
    );

    // Redone, it's back from the Trash.
    press("cmd-shift-z", cx);
    assert!(root.join("deep/er/new.rs").is_file());
    assert_eq!(depth(&panel, cx), (1, 0));
}

#[gpui::test]
fn undo_a_duplicate(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (_workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "a.txt", cx);
    run("project_panel.duplicate", cx);
    assert!(root.join("a copy.txt").is_file());
    // A copy pasted, too: one undo apiece, the last first.
    select(&panel, &root, "b.txt", cx);
    run("project_panel.copy", cx);
    run("project_panel.paste", cx);
    assert!(root.join("b copy.txt").is_file());

    press("cmd-z", cx);
    assert!(!root.join("b copy.txt").exists());
    assert!(root.join("a copy.txt").is_file());
    press("cmd-z", cx);
    assert!(!root.join("a copy.txt").exists());
    assert_eq!(names(&panel, cx), ["a.txt", "b.txt"]);
    assert_eq!(depth(&panel, cx), (0, 2));
}

#[gpui::test]
fn undo_a_trash_restores_it(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["app/lib.rs", "app/main.rs", "notes.txt"]);
    let identity = |path: &Path| kiln::core::platform::path_identity(path, false).unwrap();
    let app = identity(&root.join("app"));
    let (_workbench, panel, cx) = open(cx, &root);
    // Two at once: one undo puts both back.
    select(&panel, &root, "app", cx);
    cx.simulate_keystrokes("shift-down shift-down shift-down");
    run("project_panel.delete", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!root.join("app").exists());
    assert!(!root.join("notes.txt").exists());
    assert!(!testing::trashed("notes.txt").is_empty());

    press("cmd-z", cx);

    assert_eq!(identity(&root.join("app")), app, "the same folder");
    assert_eq!(
        fs::read_to_string(root.join("app/main.rs")).unwrap(),
        "app/main.rs"
    );
    assert_eq!(
        fs::read_to_string(root.join("notes.txt")).unwrap(),
        "notes.txt"
    );
    assert!(names(&panel, cx).contains(&"notes.txt".to_owned()));
}

#[gpui::test]
fn redo_repeats(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt"]);
    let (_workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "a.txt", cx);
    run("project_panel.rename", cx);
    name("b.txt", cx);
    press("cmd-z", cx);
    assert!(root.join("a.txt").is_file());

    press("cmd-shift-z", cx);
    assert!(root.join("b.txt").is_file());
    assert!(!root.join("a.txt").exists());
    // And undone again.
    press("cmd-z", cx);
    assert!(root.join("a.txt").is_file());
    assert_eq!(depth(&panel, cx), (0, 1));

    // A new operation leaves nothing to redo.
    select(&panel, &root, "a.txt", cx);
    run("project_panel.duplicate", cx);
    assert_eq!(depth(&panel, cx), (1, 0));
    press("cmd-shift-z", cx);
    assert!(root.join("a.txt").is_file());
    assert!(!root.join("b.txt").exists());
}

#[gpui::test]
fn refused_when_the_old_path_is_taken(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "a.txt", cx);
    run("project_panel.rename", cx);
    name("b.txt", cx);
    fs::write(root.join("a.txt"), "another\n").unwrap();

    press("cmd-z", cx);

    assert_eq!(
        toasts(&workbench, cx),
        ["“a.txt” can’t be put back: something by that name is there now."]
    );
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "another\n");
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "a.txt");
    // It stays, to undo once the path is free.
    assert_eq!(depth(&panel, cx), (1, 0));
    fs::remove_file(root.join("a.txt")).unwrap();
    press("cmd-z", cx);
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a.txt");
    assert!(!root.join("b.txt").exists());
}

#[gpui::test]
fn name_field_keeps_its_own_undo(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt"]);
    let (_workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "a.txt", cx);
    run("project_panel.rename", cx);
    name("b.txt", cx);
    assert_eq!(depth(&panel, cx), (1, 0));

    select(&panel, &root, "b.txt", cx);
    run("project_panel.rename", cx);
    cx.simulate_input("c");
    let text = |cx: &mut VisualTestContext| {
        panel
            .read_with(cx, |panel, cx| panel.name_field(cx))
            .map(|(text, _)| text)
    };
    assert_eq!(text(cx).as_deref(), Some("c.txt"));
    press("cmd-z", cx);
    assert_eq!(text(cx).as_deref(), Some("b.txt"), "the field's own undo");
    press("cmd-shift-z", cx);
    assert_eq!(text(cx).as_deref(), Some("c.txt"), "and redo");

    assert!(root.join("b.txt").is_file(), "the rename isn't undone");
    assert_eq!(depth(&panel, cx), (1, 0));
}

/// A fake that wants `*.rs` renames, asked first.
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

/// The `files` of each `method` request or notification the fake got.
fn files(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Notification { method: m, params }
            | Record::Request {
                method: m, params, ..
            } if m == method => Some(params["files"].clone()),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn an_undone_rename_asks_and_tells_servers(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    s.set_trust(true);
    s.open("src/lib.rs");
    let lib = s.uri("src/lib.rs");
    s.fake.wait_for_document(&lib, "didOpen", |r| r.is_some());
    let workbench = s.workbench.clone();
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-shift-e");
    let path = s.path("src/lib.rs");
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(path, cx)));
    s.draw();
    run("project_panel.rename", &mut s.cx);
    name("util.rs", &mut s.cx);
    s.until("the rename", |s| s.path("src/util.rs").is_file());
    let util = s.uri("src/util.rs");
    s.fake.wait_for("didRenameFiles", |records| {
        records
            .iter()
            .any(|r| r.method() == "workspace/didRenameFiles")
    });

    s.cx.simulate_keystrokes("cmd-z");
    s.until("the undo", |s| s.path("src/lib.rs").is_file());
    s.fake.wait_for("didRenameFiles", |records| {
        (records.iter())
            .filter(|r| r.method() == "workspace/didRenameFiles")
            .count()
            == 2
    });

    assert!(!s.path("src/util.rs").exists());
    let back = json!([{"oldUri": util.as_str(), "newUri": lib.as_str()}]);
    assert_eq!(files(&s, "workspace/willRenameFiles")[1], back);
    assert_eq!(files(&s, "workspace/didRenameFiles")[1], back);
}

/// Makes folder `path` one its owner can't change, or can again.
fn writable(path: &Path, yes: bool) {
    let mode = if yes { 0o755 } else { 0o555 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[gpui::test]
fn a_file_saved_since_asks_first(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["kept.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    run("project_panel.new_file", cx);
    name("notes.txt", cx);
    // It opened, focused: typed in and saved, which replaces the file.
    let before = kiln::core::platform::path_identity(&root.join("notes.txt"), false).unwrap();
    cx.simulate_input("hello");
    press("cmd-s", cx);
    assert_eq!(fs::read_to_string(root.join("notes.txt")).unwrap(), "hello");
    let after = kiln::core::platform::path_identity(&root.join("notes.txt"), false).unwrap();
    assert_ne!(before, after, "the save replaced it");
    focus(&workbench, cx);

    press("cmd-z", cx);

    // A replaced file is asked about, whoever replaced it.
    assert!(cx.has_pending_prompt());
    assert!(root.join("notes.txt").exists());
    cx.simulate_prompt_answer("Undo");
    cx.run_until_parked();
    assert!(!root.join("notes.txt").exists());
    assert_eq!(names(&panel, cx), ["kept.txt"]);
}

#[gpui::test]
fn an_entry_replaced_since_asks_first(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt"]);
    let (_workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "a.txt", cx);
    run("project_panel.rename", cx);
    name("b.txt", cx);
    // Another program's save: a new file put in its place.
    fs::write(root.join(".b.txt.tmp"), "theirs").unwrap();
    fs::rename(root.join(".b.txt.tmp"), root.join("b.txt")).unwrap();

    press("cmd-z", cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(root.join("b.txt").is_file(), "cancelled");
    assert_eq!(depth(&panel, cx), (1, 0));

    press("cmd-z", cx);
    cx.simulate_prompt_answer("Undo");
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "theirs");
    assert!(!root.join("b.txt").exists());
    assert_eq!(depth(&panel, cx), (0, 1));
}

#[gpui::test]
fn an_operation_that_failed_keeps_what_there_is_to_redo(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "ro/"]);
    let (workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "a.txt", cx);
    run("project_panel.rename", cx);
    name("b.txt", cx);
    press("cmd-z", cx);
    assert_eq!(depth(&panel, cx), (0, 1));

    // A copy pasted where nothing can be made.
    run("project_panel.copy", cx);
    writable(&root.join("ro"), false);
    select(&panel, &root, "ro", cx);
    run("project_panel.paste", cx);
    writable(&root.join("ro"), true);
    assert_eq!(toasts(&workbench, cx), ["“a.txt” couldn’t be pasted."]);

    assert_eq!(depth(&panel, cx), (0, 1));
    press("cmd-shift-z", cx);
    assert!(root.join("b.txt").is_file());
}

#[gpui::test]
fn what_fails_of_an_undo_stays_to_undo(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["app/a.txt", "b.txt"]);
    let (_workbench, panel, cx) = open(cx, &root);
    let app = root.join("app");

    // Made: two copies, one in a folder that can't be changed now.
    select(&panel, &root, "app/a.txt", cx);
    press("shift-down", cx);
    run("project_panel.duplicate", cx);
    assert!(root.join("app/a copy.txt").is_file());
    assert!(root.join("b copy.txt").is_file());
    writable(&app, false);
    press("cmd-z", cx);
    writable(&app, true);
    assert!(root.join("app/a copy.txt").is_file());
    assert!(!root.join("b copy.txt").exists());
    assert_eq!(depth(&panel, cx), (1, 1));
    press("cmd-z", cx);
    assert!(!root.join("app/a copy.txt").exists());
    assert_eq!(depth(&panel, cx), (0, 2));

    // Trashed: one goes back, one can't yet.
    select(&panel, &root, "app/a.txt", cx);
    press("shift-down", cx);
    run("project_panel.delete", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!root.join("app/a.txt").exists());
    writable(&app, false);
    press("cmd-z", cx);
    writable(&app, true);
    assert!(root.join("b.txt").is_file());
    assert!(!root.join("app/a.txt").exists());
    assert_eq!(depth(&panel, cx), (1, 1));
    press("cmd-z", cx);
    assert_eq!(
        fs::read_to_string(root.join("app/a.txt")).unwrap(),
        "app/a.txt"
    );
}

#[gpui::test]
fn what_fails_of_an_undone_move_stays_to_undo(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["dst/", "one/a.txt", "b.txt"]);
    let (_workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "one/a.txt", cx);
    press("shift-down", cx);
    run("project_panel.cut", cx);
    select(&panel, &root, "dst", cx);
    run("project_panel.paste", cx);
    assert!(root.join("dst/a.txt").is_file());
    assert!(root.join("dst/b.txt").is_file());

    writable(&root.join("one"), false);
    press("cmd-z", cx);
    writable(&root.join("one"), true);
    assert!(root.join("b.txt").is_file());
    assert!(
        root.join("dst/a.txt").is_file(),
        "its folder couldn't take it"
    );
    assert_eq!(depth(&panel, cx), (1, 1));
    press("cmd-z", cx);
    assert!(root.join("one/a.txt").is_file());
    assert_eq!(depth(&panel, cx), (0, 2));
}

/// The fake answers `willRenameFiles` with `reply`, after `delay`.
fn answer(s: &Lsp, reply: Value, delay: Duration) {
    let reply = Reply::Result(reply);
    s.fake
        .set_reply("workspace/willRenameFiles", Scripted { reply, delay });
}

/// Renames `src/lib.rs` to `src/util.rs` in the tree, as the servers have
/// it answered, and focuses the tree.
fn rename_lib(s: &mut Lsp) -> Entity<ProjectPanel> {
    s.set_trust(true);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    let workbench = s.workbench.clone();
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-shift-e");
    let path = s.path("src/lib.rs");
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(path, cx)));
    s.draw();
    run("project_panel.rename", &mut s.cx);
    name("util.rs", &mut s.cx);
    s.until("the rename", |s| s.path("src/util.rs").is_file());
    panel
}

fn lsp_depth(s: &Lsp, panel: &Entity<ProjectPanel>) -> (usize, usize) {
    panel.read_with(&s.cx, |panel, _| panel.history_depth())
}

#[gpui::test]
fn a_path_taken_while_servers_answer_keeps_the_undo(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    let panel = rename_lib(&mut s);
    answer(&s, Value::Null, Duration::from_millis(1500));

    s.cx.simulate_keystrokes("cmd-z");
    // Taken while the servers are asked.
    fs::write(s.path("src/lib.rs"), "theirs\n").unwrap();
    let workbench = s.workbench.clone();
    s.until("the refusal", |s| !toasts(&workbench, &mut s.cx).is_empty());

    assert!(s.path("src/util.rs").is_file());
    assert_eq!(
        fs::read_to_string(s.path("src/lib.rs")).unwrap(),
        "theirs\n"
    );
    assert_eq!(lsp_depth(&s, &panel), (1, 0), "it stays to undo");
    fs::remove_file(s.path("src/lib.rs")).unwrap();
    answer(&s, Value::Null, Duration::ZERO);
    s.cx.simulate_keystrokes("cmd-z");
    s.until("the undo", |s| s.path("src/lib.rs").is_file());
    assert_eq!(lsp_depth(&s, &panel), (0, 1));
}

#[gpui::test]
fn an_operation_while_an_undo_waits_leaves_nothing_to_redo(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    let panel = rename_lib(&mut s);
    answer(&s, Value::Null, Duration::from_millis(1500));

    s.cx.simulate_keystrokes("cmd-z");
    // Duplicated while the servers are asked.
    let tool = s.path("tool.py");
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(tool, cx)));
    s.draw();
    run("project_panel.duplicate", &mut s.cx);
    s.until("the copy", |s| s.path("tool copy.py").is_file());
    s.until("the undo", |s| s.path("src/lib.rs").is_file());

    // The copy's undo; the undone rename's redo is gone with the copy.
    assert_eq!(lsp_depth(&s, &panel), (1, 0));
}

#[gpui::test]
fn a_save_while_servers_answer_keeps_the_undo_which_then_asks(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    let panel = rename_lib(&mut s);
    answer(&s, Value::Null, Duration::from_millis(1500));

    s.cx.simulate_keystrokes("cmd-z");
    // A save of it that lands while the servers are asked.
    fs::write(s.path("src/.util.rs.tmp"), "pub fn saved() {}\n").unwrap();
    fs::rename(s.path("src/.util.rs.tmp"), s.path("src/util.rs")).unwrap();
    let workbench = s.workbench.clone();
    s.until("the move's end", |s| {
        !toasts(&workbench, &mut s.cx).is_empty()
    });
    assert_eq!(
        toasts(&workbench, &mut s.cx),
        ["“util.rs” changed meanwhile, so it was left as it is. Try again to go ahead."]
    );
    assert_eq!(lsp_depth(&s, &panel), (1, 0), "it stays to undo");

    answer(&s, Value::Null, Duration::ZERO);
    s.cx.simulate_keystrokes("cmd-z");
    s.draw();
    assert!(s.cx.has_pending_prompt());
    s.cx.simulate_prompt_answer("Undo");
    s.until("the undo", |s| s.path("src/lib.rs").is_file());
    assert_eq!(
        fs::read_to_string(s.path("src/lib.rs")).unwrap(),
        "pub fn saved() {}\n"
    );
}

#[gpui::test]
fn a_copy_gone_leaves_the_rest_of_its_step(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["one/a.txt", "b.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "one/a.txt", cx);
    press("shift-down", cx);
    run("project_panel.duplicate", cx);
    fs::remove_file(root.join("one/a copy.txt")).unwrap();

    press("cmd-z", cx);

    assert_eq!(
        toasts(&workbench, cx),
        ["“a copy.txt” was moved, replaced or deleted since, so it was left as it is."]
    );
    assert!(!root.join("b copy.txt").exists(), "the other went");
    assert_eq!(depth(&panel, cx), (0, 1));
}

#[gpui::test]
fn a_moved_entry_gone_leaves_the_rest_of_its_step(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["dst/", "one/a.txt", "b.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "one/a.txt", cx);
    press("shift-down", cx);
    run("project_panel.cut", cx);
    select(&panel, &root, "dst", cx);
    run("project_panel.paste", cx);
    fs::remove_file(root.join("dst/a.txt")).unwrap();

    press("cmd-z", cx);

    assert_eq!(
        toasts(&workbench, cx),
        ["“a.txt” was moved, replaced or deleted since, so it was left as it is."]
    );
    assert!(root.join("b.txt").is_file(), "the other went back");
    assert_eq!(depth(&panel, cx), (0, 1));
}

#[gpui::test]
fn an_entry_emptied_from_the_trash_leaves_the_rest_of_its_step(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["two/"]);
    // Names of the test's own, to find them in the test trash by.
    let unique = root.file_name().unwrap().to_str().unwrap().to_owned();
    let (gone, kept) = (format!("{unique}-gone.txt"), format!("{unique}-kept.txt"));
    fs::write(root.join("two").join(&gone), "").unwrap();
    fs::write(root.join("two").join(&kept), "kept").unwrap();
    let (workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, &format!("two/{gone}"), cx);
    press("shift-down", cx);
    run("project_panel.delete", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!root.join("two").join(&kept).exists());
    for path in testing::trashed(&gone) {
        fs::remove_file(path).unwrap();
    }

    press("cmd-z", cx);

    assert_eq!(
        toasts(&workbench, cx),
        [format!("“{gone}” is no longer in the Trash.")]
    );
    assert_eq!(
        fs::read_to_string(root.join("two").join(&kept)).unwrap(),
        "kept"
    );
    assert_eq!(depth(&panel, cx), (0, 1));
}

#[gpui::test]
fn the_question_names_every_entry_replaced_and_asks_again_of_one_replaced_since(
    cx: &mut TestAppContext,
) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (_workbench, panel, cx) = open(cx, &root);
    select(&panel, &root, "a.txt", cx);
    press("shift-down", cx);
    run("project_panel.duplicate", cx);
    let replace = |name: &str, text: &str| {
        let tmp = root.join(format!(".{name}.tmp"));
        fs::write(&tmp, text).unwrap();
        fs::rename(&tmp, root.join(name)).unwrap();
    };
    replace("a copy.txt", "theirs");
    replace("b copy.txt", "theirs");

    press("cmd-z", cx);
    assert!(cx.has_pending_prompt());
    // Replaced again while asked: what was asked about isn't it.
    replace("a copy.txt", "theirs again");
    cx.simulate_prompt_answer("Undo");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "asked again of a copy.txt");
    assert!(root.join("a copy.txt").exists());
    assert!(root.join("b copy.txt").exists());
    cx.simulate_prompt_answer("Undo");
    cx.run_until_parked();
    assert!(!root.join("a copy.txt").exists());
    assert!(!root.join("b copy.txt").exists());
}

#[gpui::test]
fn what_can_t_be_reached_or_has_no_folder_stays_to_undo(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["app/a.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    let app = root.join("app");

    // A copy in a folder that can't be searched now.
    select(&panel, &root, "app/a.txt", cx);
    run("project_panel.duplicate", cx);
    fs::set_permissions(&app, fs::Permissions::from_mode(0o000)).unwrap();
    press("cmd-z", cx);
    writable(&app, true);
    assert_eq!(
        toasts(&workbench, cx),
        ["“a copy.txt” can’t be reached now. Try again once it can."]
    );
    assert_eq!(depth(&panel, cx), (1, 0));
    press("cmd-z", cx);
    assert!(!root.join("app/a copy.txt").exists());

    // Trashed, its folder gone, then made again.
    select(&panel, &root, "app/a.txt", cx);
    run("project_panel.delete", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    fs::remove_dir(&app).unwrap();
    press("cmd-z", cx);
    assert_eq!(depth(&panel, cx), (1, 0), "it stays to undo");
    fs::create_dir(&app).unwrap();
    press("cmd-z", cx);
    assert_eq!(fs::read_to_string(app.join("a.txt")).unwrap(), "app/a.txt");
}
