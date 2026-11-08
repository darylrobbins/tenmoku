//! The project panel's file operations, on a temporary folder: the name
//! field for a new entry and a rename, the editors following a rename,
//! the move to the Trash, duplicate, copying paths, and vim.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand, set_vim_enabled};
use fe_project::FsChange;
use gpui::{
    AppContext as _, ClipboardItem, Entity, Focusable as _, TestAppContext, VisualTestContext,
};

use super::EditKind;
use crate::{Pane, PanelEvent, ProjectPanel, Workbench};

/// Files at `paths` (relative) with their text, in a temporary directory.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path(), files);
    dir
}

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// Every entry under `root`, relative, `/`-separated, sorted; folders end
/// in `/`.
fn snapshot(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = fe_project::relative_path(root, &path);
            if path.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &path, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// A workbench rooted at `root`, in an active window, so focus events
/// (the name field's blur) are dispatched.
fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = crate::tests::open_root(cx, root);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (workbench, cx)
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    workbench.read_with(cx, |workbench, _| workbench.panel().clone())
}

/// The panel's rows as `(depth, name)`.
fn rows(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
    panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        tree.rows(0..tree.row_count())
            .map(|row| (row.depth, row.name.to_string_lossy().into_owned()))
            .collect()
    })
}

/// The selected row, as `(depth, name)`.
fn selected(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<(usize, String)> {
    let ix = panel.read_with(cx, |panel, _| panel.selected_row())?;
    Some(rows(panel, cx)[ix].clone())
}

/// Focuses the workbench, as the app does, then the panel through
/// `dock.show_project`.
fn focus_panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-e");
    let panel = panel(workbench, cx);
    assert!(
        panel_focused(&panel, cx),
        "dock.show_project focuses the panel"
    );
    panel
}

fn panel_focused(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

/// Selects the row called `name`, expanding the folders `path` (relative,
/// its last component `name`) runs through.
fn select(panel: &Entity<ProjectPanel>, root: &Path, rel: &str, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.reveal(root.join(rel), cx));
    cx.run_until_parked();
    let name = rel.rsplit('/').next().unwrap();
    assert_eq!(
        selected(panel, cx).map(|(_, row)| row).as_deref(),
        Some(name),
        "{:?}",
        rows(panel, cx)
    );
}

/// Runs command `id` where focus is, as a key bound in the panel (or in
/// its name field, while it's open) would.
fn run(id: &str, cx: &mut VisualTestContext) {
    let mut context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    if id.starts_with("text_input.") {
        context = context.with(ContextFrame::new("text_input"));
    }
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

/// The name field's text and error, while it's open.
fn field(
    panel: &Entity<ProjectPanel>,
    cx: &mut VisualTestContext,
) -> Option<(String, Option<String>)> {
    panel.read_with(cx, |panel, cx| panel.name_field(cx))
}

fn field_focused(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> bool {
    let input = panel.read_with(cx, |panel, _| panel.name_input());
    input.is_some_and(|input| cx.update(|window, cx| input.focus_handle(cx).is_focused(window)))
}

fn active_editor(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<EditorView>> {
    workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
}

fn editor_path(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    cx.run_until_parked();
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane_group(cx)
            .panes()
            .into_iter()
            .cloned()
            .collect()
    })
}

#[gpui::test]
fn create(cx: &mut TestAppContext) {
    let dir = project(&[("app/main.rs", "fn main() {}\n"), ("README.md", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root);
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);

    // On a folder row: a name field as the folder's first row, expanded,
    // with focus.
    run("project_panel.new_file", cx);
    assert_eq!(field(&panel, cx), Some((String::new(), None)));
    assert!(field_focused(&panel, cx));
    assert_eq!(
        rows(&panel, cx),
        [
            (0, "app".into()),
            (1, "main.rs".into()),
            (0, "README.md".into())
        ],
        "app expanded"
    );
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.field_row()),
        Some((1, 1))
    );
    // The list draws it, one row more than the tree.
    let built = panel.read_with(cx, |panel, _| panel.built());
    assert_eq!(built.last(), Some(&(0..4)));

    // Typed, with `/`s naming folders to make, and Enter (`project_panel.
    // open`, the panel's binding): created, its row selected, and opened in
    // the active pane, focused.
    cx.simulate_input("src/x.rs");
    assert_eq!(field(&panel, cx), Some(("src/x.rs".into(), None)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let created = root.join("app/src/x.rs");
    assert!(created.is_file());
    assert_eq!(field(&panel, cx), None);
    assert_eq!(selected(&panel, cx), Some((2, "x.rs".into())));
    let editor = active_editor(&workbench, cx).expect("the new file opened");
    assert_eq!(editor_path(&editor, cx), Some(created.clone()));
    assert!(focused(&editor, cx));

    // An existing name: the error shows under the field, which stays open
    // with its text, and nothing changes.
    let before = snapshot(root);
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    run("project_panel.new_file", cx);
    cx.simulate_input("src/x.rs");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (text, error) = field(&panel, cx).expect("the field stays open");
    assert_eq!(text, "src/x.rs");
    let error = error.expect("the error shows");
    assert!(error.contains("already exists"), "{error}");
    assert!(field_focused(&panel, cx));
    assert_eq!(snapshot(root), before);

    // The field has a caret: left (the panel's key) moves it.
    run("text_input.clear", cx);
    cx.simulate_input("ab");
    cx.simulate_keystrokes("left");
    cx.simulate_input("c");
    assert_eq!(
        field(&panel, cx).map(|(text, _)| text).as_deref(),
        Some("acb")
    );

    // A name that isn't one is refused before anything is tried.
    run("text_input.clear", cx);
    cx.simulate_input("../y.rs");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let error = field(&panel, cx).and_then(|(_, error)| error);
    assert_eq!(error.as_deref(), Some("`..` is not a name"));
    assert_eq!(snapshot(root), before);

    // Escape creates nothing, and focus goes back to the panel.
    cx.simulate_keystrokes("escape");
    assert_eq!(field(&panel, cx), None);
    assert!(panel_focused(&panel, cx));
    run("project_panel.new_file", cx);
    cx.simulate_input("y.rs");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(field(&panel, cx), None);
    assert_eq!(snapshot(root), before);

    // Focus leaving the field cancels it too.
    run("project_panel.new_file", cx);
    cx.simulate_input("z.rs");
    cx.update(|window, cx| {
        window.focus(&workbench.focus_handle(cx), cx);
        // Focus listeners hear of it as the next frame draws.
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    assert_eq!(field(&panel, cx), None);
    assert_eq!(snapshot(root), before);

    // A folder, from a file row: in the file's folder.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app/main.rs", cx);
    run("project_panel.new_folder", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.field_row()),
        Some((1, 1))
    );
    cx.simulate_input("lib");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("app/lib").is_dir());
    assert_eq!(selected(&panel, cx), Some((1, "lib".into())));
    assert!(panel_focused(&panel, cx), "a folder opens nothing");

    // A new file's name ending in `/` makes a folder, and opens nothing:
    // here in lib, selected.
    let editor = active_editor(&workbench, cx);
    run("project_panel.new_file", cx);
    cx.simulate_input("pkg/");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("app/lib/pkg").is_dir());
    assert_eq!(selected(&panel, cx), Some((2, "pkg".into())));
    assert_eq!(active_editor(&workbench, cx), editor);
    let error = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).title(cx));
    assert!(!error.contains("loading"), "{error}");
    assert!(panel_focused(&panel, cx));

    // With no row selected, in the root.
    panel.update(cx, |panel, _| panel.selected = None);
    run("project_panel.new_file", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.field_row()),
        Some((0, 0))
    );
    cx.simulate_input("top.txt");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(root.join("top.txt").is_file());
    assert_eq!(selected(&panel, cx), Some((0, "top.txt".into())));
}

#[gpui::test]
fn rename_follows(cx: &mut TestAppContext) {
    let dir = project(&[("app/main.rs", "fn main() {}\n"), ("notes.txt", "hello\n")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root);

    // notes.txt in a split of its own, with unsaved changes, and
    // app/main.rs in the left pane.
    open_file(&workbench, &root.join("notes.txt"), cx);
    cx.simulate_input("x");
    cx.simulate_keystrokes("cmd-\\");
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    open_file(&workbench, &root.join("app/main.rs"), cx);
    let editors = |pane: &Entity<Pane>, cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| {
            (pane.tabs().iter())
                .filter_map(|tab| tab.editor().cloned())
                .collect::<Vec<_>>()
        })
    };
    let notes = [
        editors(&left, cx)[0].clone(),
        editors(&right, cx)[0].clone(),
    ];
    let main = editors(&right, cx)[1].clone();

    // The rename field starts from the name, and Enter renames.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "notes.txt", cx);
    run("project_panel.rename", cx);
    assert_eq!(field(&panel, cx), Some(("notes.txt".into(), None)));
    assert!(field_focused(&panel, cx));
    // Its stem selected: typing replaces it, keeping the extension.
    let input = panel.read_with(cx, |panel, _| panel.name_input().unwrap());
    assert_eq!(input.read_with(cx, |input, _| input.selected_range()), 0..5);
    cx.simulate_input("todo");
    assert_eq!(field(&panel, cx), Some(("todo.txt".into(), None)));
    // The field's own keys move its caret.
    cx.simulate_keystrokes("end");
    for _ in 0..3 {
        run("text_input.backspace", cx);
    }
    cx.simulate_input("md");
    assert_eq!(field(&panel, cx), Some(("todo.md".into(), None)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!root.join("notes.txt").exists());
    assert_eq!(fs::read_to_string(root.join("todo.md")).unwrap(), "hello\n");
    assert_eq!(field(&panel, cx), None);
    assert_eq!(selected(&panel, cx), Some((0, "todo.md".into())));
    assert!(panel_focused(&panel, cx));

    // Both editors of it, in both panes, take the new path, with their
    // buffer: the tabs and the title show it.
    let todo = root.join("todo.md");
    // Each detects its language again from the new name.
    let markdown = fe_syntax::detect_language_name(Some(&todo), "");
    assert!(markdown.is_some());
    for editor in &notes {
        assert_eq!(editor_path(editor, cx), Some(todo.clone()));
        let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
        assert_eq!(text, "xhello\n");
        let language = editor.read_with(cx, |editor, _| {
            editor.syntax().map(|syntax| syntax.language().name())
        });
        assert_eq!(language, markdown);
    }
    for pane in [&left, &right] {
        let tab = pane.read_with(cx, |pane, _| pane.tabs()[0].path.clone());
        assert_eq!(tab, Some(todo.clone()));
    }
    let title = left.read_with(cx, |pane, cx| pane.title(cx));
    assert!(title.starts_with("todo.md"), "{title}");
    // The right pane, active, shows it: the window's title follows.
    cx.update(|window, cx| right.update(cx, |pane, cx| pane.activate(0, window, cx)));
    cx.run_until_parked();
    assert!(
        cx.window_title()
            .is_some_and(|title| title.starts_with("todo.md")),
        "{:?}",
        cx.window_title()
    );
    assert_eq!(editor_path(&main, cx), Some(root.join("app/main.rs")));

    // The stamp moved with the path (`EditorModel::move_path`): todo.md
    // changed behind the editors is a conflict for a save from either
    // split, which asks before it overwrites.
    let theirs = "theirs, a longer line\n";
    fs::write(&todo, theirs).unwrap();
    for editor in &notes {
        let saved = cx.update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)));
        cx.run_until_parked();
        let (question, _) = cx.pending_prompt().expect("asked before overwriting");
        assert_eq!(question, "“todo.md” changed on disk since it was opened.");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        drop(saved);
        assert_eq!(fs::read_to_string(&todo).unwrap(), theirs);
    }

    // Renaming to the same name changes nothing.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "todo.md", cx);
    run("project_panel.rename", cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(field(&panel, cx), None);
    assert_eq!(editor_path(&notes[0], cx), Some(todo.clone()));

    // A name with a `/` is refused.
    run("project_panel.rename", cx);
    run("text_input.clear", cx);
    cx.simulate_input("a/b");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let error = field(&panel, cx).and_then(|(_, error)| error);
    assert_eq!(error.as_deref(), Some("A name can't hold a `/` here"));
    cx.simulate_keystrokes("escape");

    // A folder: the editors of the files in it follow.
    select(&panel, root, "app", cx);
    assert!(
        rows(&panel, cx).contains(&(1, "main.rs".into())),
        "app is expanded"
    );
    run("project_panel.rename", cx);
    run("text_input.clear", cx);
    cx.simulate_input("core");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(snapshot(root), ["core/", "core/main.rs", "todo.md"]);
    assert_eq!(editor_path(&main, cx), Some(root.join("core/main.rs")));
    assert_eq!(selected(&panel, cx), Some((0, "core".into())));
    assert_eq!(
        rows(&panel, cx),
        [
            (0, "core".into()),
            (1, "main.rs".into()),
            (0, "todo.md".into())
        ],
        "expanded, as app was"
    );
    let tab = right.read_with(cx, |pane, _| pane.tabs()[1].path.clone());
    assert_eq!(tab, Some(root.join("core/main.rs")));
    assert_eq!(editor_path(&notes[1], cx), Some(todo));
}

/// A save hook that holds each save until the clock passes the hook's
/// limit.
struct HoldSaves;

impl fe_gpui::pane_item::SaveHook for HoldSaves {
    fn before_save(&self, _: &Entity<EditorView>, cx: &mut gpui::App) -> gpui::Task<()> {
        cx.background_spawn(std::future::pending())
    }
}

/// A rename that would move a buffer being saved, in this window or
/// another, is refused, the field saying so, until the save is over: so the
/// save can't write the old path after the rename.
#[gpui::test]
fn rename_waits_for_saves(cx: &mut TestAppContext) {
    let dir = project(&[("app/main.rs", "fn main() {}\n"), ("notes.txt", "hello\n")]);
    let root = dir.path();
    let mut app = cx.clone();
    let (workbench, cx) = open_root(cx, root);
    open_file(&workbench, &root.join("app/main.rs"), cx);
    let main = active_editor(&workbench, cx).unwrap();
    cx.simulate_input("x");
    // Another window shows notes.txt, being saved.
    let (other, other_cx) = open_root(&mut app, root);
    open_file(&other, &root.join("notes.txt"), other_cx);
    let notes = active_editor(&other, other_cx).unwrap();
    other_cx.simulate_input("y");
    let saving = other_cx.update(|window, cx| {
        notes.update(cx, |notes, cx| {
            notes.set_save_hook(Some(std::rc::Rc::new(HoldSaves)));
            notes.save(window, cx)
        })
    });
    other_cx.run_until_parked();
    assert!(notes.read_with(other_cx, |notes, _| notes.is_saving()));

    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "notes.txt", cx);
    run("project_panel.rename", cx);
    cx.simulate_input("todo");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let error = field(&panel, cx).and_then(|(_, error)| error);
    assert_eq!(
        error.as_deref(),
        Some("“notes.txt” is being saved. Rename it once it’s saved.")
    );
    assert!(root.join("notes.txt").is_file());
    assert_eq!(editor_path(&notes, other_cx), Some(root.join("notes.txt")));

    // Once it's saved, the rename goes through, in both windows.
    other_cx
        .executor()
        .advance_clock(std::time::Duration::from_secs(2));
    other_cx.run_until_parked();
    drop(saving);
    assert_eq!(
        fs::read_to_string(root.join("notes.txt")).unwrap(),
        "yhello\n"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(field(&panel, cx), None);
    assert_eq!(
        fs::read_to_string(root.join("todo.txt")).unwrap(),
        "yhello\n"
    );
    assert!(!root.join("notes.txt").exists());
    assert_eq!(editor_path(&notes, other_cx), Some(root.join("todo.txt")));

    // A folder holding a file being saved, as well.
    let saving = cx.update(|window, cx| {
        main.update(cx, |main, cx| {
            main.set_save_hook(Some(std::rc::Rc::new(HoldSaves)));
            main.save(window, cx)
        })
    });
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    run("project_panel.rename", cx);
    cx.simulate_input("core");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let error = field(&panel, cx).and_then(|(_, error)| error);
    assert_eq!(
        error.as_deref(),
        Some("“app” is being saved. Rename it once it’s saved.")
    );
    assert!(root.join("app/main.rs").is_file());
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    cx.run_until_parked();
    drop(saving);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(root.join("core/main.rs")).unwrap(),
        "xfn main() {}\n"
    );
    assert_eq!(editor_path(&main, cx), Some(root.join("core/main.rs")));
}

/// A temporary project for the trash tests, its name a prefix of their own
/// files' names, to find them in the test trash by.
fn trashable_project() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("fe-workbench-trash-")
        .tempdir()
        .unwrap()
}

/// The one entry named `name` in the test trash, where every trash of a
/// test build goes (`fe_project::fs_ops::testing`).
fn in_test_trash(name: &str) -> PathBuf {
    let found = fe_project::fs_ops::testing::trashed(name);
    assert_eq!(found.len(), 1, "{name} in the test trash: {found:?}");
    found.into_iter().next().unwrap()
}

#[gpui::test]
fn delete(cx: &mut TestAppContext) {
    let dir = trashable_project();
    // A name of the test's own, to find it in the trash.
    let unique = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let name = format!("{unique}.txt");
    let root = dunce::canonicalize(dir.path()).unwrap();
    write_files(&root, &[(&name, "kept\n"), ("other.txt", "")]);
    let (workbench, cx) = open_root(cx, &root);
    let path = root.join(&name);
    open_file(&workbench, &path, cx);
    let editor = active_editor(&workbench, cx).unwrap();

    // Asked first; Cancel keeps it.
    let panel = focus_panel(&workbench, cx);
    select(&panel, &root, &name, cx);
    run("project_panel.delete", cx);
    let (question, detail) = cx.pending_prompt().expect("asked");
    assert_eq!(question, format!("Move “{name}” to the Trash?"));
    assert_eq!(detail, "");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(path.is_file());
    assert!(rows(&panel, cx).contains(&(0, name.clone())));

    // Move to Trash moves it out of the folder and the tree; its editor
    // stays open with its buffer.
    run("project_panel.delete", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!path.exists(), "{} is still there", path.display());
    assert_eq!(fs::read_to_string(in_test_trash(&name)).unwrap(), "kept\n");
    assert_eq!(snapshot(&root), ["other.txt"]);
    assert_eq!(rows(&panel, cx), [(0, "other.txt".to_string())]);
    assert_eq!(selected(&panel, cx), Some((0, "other.txt".into())));
    let open = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx));
    assert_eq!(open, std::slice::from_ref(&editor));
    assert_eq!(editor_path(&editor, cx), Some(path));
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "kept\n");
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
fn misc(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "a\n"), ("b/c.txt", "c\n"), ("z.rs", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root);
    let panel = focus_panel(&workbench, cx);
    let clipboard =
        |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|item| item.text());

    // Duplicate: `name copy.ext`, selected.
    select(&panel, root, "a.txt", cx);
    run("project_panel.duplicate", cx);
    assert_eq!(fs::read_to_string(root.join("a copy.txt")).unwrap(), "a\n");
    assert_eq!(selected(&panel, cx), Some((0, "a copy.txt".into())));

    // A folder, with what's in it.
    select(&panel, root, "b", cx);
    run("project_panel.duplicate", cx);
    assert_eq!(
        fs::read_to_string(root.join("b copy/c.txt")).unwrap(),
        "c\n"
    );
    assert_eq!(selected(&panel, cx), Some((0, "b copy".into())));

    // The absolute path, and the relative one with `/`s.
    select(&panel, root, "b/c.txt", cx);
    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    run("project_panel.copy_path", cx);
    let absolute = root.join("b").join("c.txt").to_string_lossy().into_owned();
    assert_eq!(clipboard(cx), Some(absolute));
    run("project_panel.copy_relative_path", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("b/c.txt"));

    // The first and last rows.
    let shown = rows(&panel, cx);
    run("project_panel.select_last", cx);
    assert_eq!(selected(&panel, cx), shown.last().cloned());
    assert_eq!(selected(&panel, cx), Some((0, "z.rs".into())));
    run("project_panel.select_first", cx);
    assert_eq!(selected(&panel, cx), Some((0, "b".into())));
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.drawn_selected()),
        Some(0)
    );

    // `project_panel.open` (Enter, vim's `l`) toggles a folder.
    run("project_panel.open", cx);
    assert!(
        !rows(&panel, cx).contains(&(1, "c.txt".into())),
        "collapsed"
    );
    run("project_panel.open", cx);
    assert!(rows(&panel, cx).contains(&(1, "c.txt".into())), "expanded");

    // With no row selected, the row operations do nothing.
    panel.update(cx, |panel, _| panel.selected = None);
    let before = snapshot(root);
    for id in [
        "project_panel.duplicate",
        "project_panel.rename",
        "project_panel.copy_path",
    ] {
        run(id, cx);
    }
    assert_eq!(snapshot(root), before);
    assert_eq!(field(&panel, cx), None);
    assert_eq!(clipboard(cx).as_deref(), Some("b/c.txt"));

    // The Trash question is about the entry asked about: renamed while it
    // shows, and another put at its path, the other is kept.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "z.rs", cx);
    run("project_panel.delete", cx);
    assert!(cx.has_pending_prompt());
    fs::rename(root.join("z.rs"), root.join("y.rs")).unwrap();
    fs::write(root.join("z.rs"), "another\n").unwrap();
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    let (message, detail) = cx.pending_prompt().expect("says why");
    assert_eq!(message, "“z.rs” couldn’t be moved to the Trash.");
    assert_eq!(detail, "It was moved or replaced since you were asked.");
    cx.simulate_prompt_answer("OK");
    assert_eq!(fs::read_to_string(root.join("z.rs")).unwrap(), "another\n");
    assert!(root.join("y.rs").is_file());
}

#[gpui::test]
fn vim(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "one\ntwo\n"), ("d/e.txt", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    let panel = focus_panel(&workbench, cx);
    let context = |cx: &mut VisualTestContext| {
        cx.update(|window, _| {
            let stack = window.context_stack();
            let has = |key: &str| stack.iter().any(|context| context.contains(key));
            (has("project_panel"), has("vim"))
        })
    };

    // The panel's key context carries the vim flag.
    assert_eq!(context(cx), (true, true));

    // A file opened from the tree is in normal mode.
    select(&panel, root, "a.txt", cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let a = active_editor(&workbench, cx).expect("a.txt opened");
    assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("normal"));
    // Its caret on the second line.
    cx.simulate_keystrokes("j");
    let caret = |cx: &mut VisualTestContext| {
        a.read_with(cx, |a, _| a.model().selections()[0].range.clone())
    };
    let moved = caret(cx);
    assert_eq!(moved.start, 4, "{moved:?}");

    // Left in insert mode, or a visual mode, then opened again from the
    // tree: in normal mode, its text as it was.
    for keys in ["i", "v"] {
        cx.simulate_keystrokes(keys);
        assert_ne!(
            a.read_with(cx, |a, _| a.vim_mode()),
            Some("normal"),
            "{keys}"
        );
        let panel = focus_panel(&workbench, cx);
        select(&panel, root, "a.txt", cx);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(active_editor(&workbench, cx), Some(a.clone()));
        assert!(focused(&a, cx));
        assert_eq!(
            a.read_with(cx, |a, _| a.vim_mode()),
            Some("normal"),
            "{keys}"
        );
        assert_eq!(
            a.read_with(cx, |a, _| a.model().text().to_string()),
            "one\ntwo\n"
        );
    }
    let moved = caret(cx);

    // Keys typed in the name field reach the field only: not the panel's
    // vim keys (their flag is off meanwhile), nor the editor's.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "d", cx);
    run("project_panel.new_file", cx);
    assert_eq!(context(cx), (true, false));
    cx.simulate_keystrokes("j k g g shift-g l h d d i");
    cx.simulate_input("x.rs");
    assert_eq!(field(&panel, cx), Some(("jkggGlhddix.rs".into(), None)));
    assert_eq!(selected(&panel, cx), Some((0, "d".into())));
    assert_eq!(
        a.read_with(cx, |a, _| a.model().text().to_string()),
        "one\ntwo\n"
    );
    assert_eq!(caret(cx), moved);

    // A file created from the tree opens in normal mode.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let created = active_editor(&workbench, cx).expect("the new file opened");
    assert_eq!(
        editor_path(&created, cx),
        Some(root.join("d/jkggGlhddix.rs"))
    );
    assert_eq!(created.read_with(cx, |e, _| e.vim_mode()), Some("normal"));

    // A renamed open buffer keeps its vim mode and caret.
    let panel = focus_panel(&workbench, cx);
    assert_eq!(context(cx), (true, true));
    select(&panel, root, "a.txt", cx);
    run("project_panel.rename", cx);
    cx.simulate_keystrokes("b");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(editor_path(&a, cx), Some(root.join("b.txt")));
    assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("normal"));
    assert_eq!(caret(cx), moved);

    // An open from the tree that waits behind a Save As of its file: once
    // the save lands and the editor shows, it's in normal mode too.
    open_file(&workbench, &root.join("b.txt"), cx);
    assert!(focused(&a, cx));
    cx.simulate_keystrokes("i");
    assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("insert"));
    let c = root.join("c.txt");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.save_editor_as(&a, c.clone(), window, cx);
        // As the panel would, with the destination held.
        panel.update(cx, |_, cx| cx.emit(PanelEvent::OpenFile(c.clone())));
    });
    cx.run_until_parked();
    assert_eq!(editor_path(&a, cx), Some(c));
    assert_eq!(active_editor(&workbench, cx), Some(a.clone()));
    assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("normal"));
}

/// Starts naming with `kind` on the selected row, types `name` and runs
/// it, in one turn: so whatever ran before is still running.
fn name_now(
    panel: &Entity<ProjectPanel>,
    kind: EditKind,
    name: &str,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    panel.update(cx, |panel, cx| {
        panel.start_edit(kind, window, cx);
        let input = panel.edit.as_ref().unwrap().input.clone();
        input.update(cx, |input, cx| input.set_value(name, cx));
        panel.commit_edit(window, cx);
    });
}

/// The name field's error.
fn error(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    field(panel, cx).and_then(|(_, error)| error)
}

/// A rename waits for what would act on the old path after it: a Save As
/// to a file under it, a save whose tab closed, a file being read into a
/// tab, and the panel's own create and duplicate there. Each lands where it
/// was asked to, and the rename goes through once it has.
#[gpui::test]
fn rename_waits_for_what_acts_on_the_old_path(cx: &mut TestAppContext) {
    let dir = project(&[
        ("app/main.rs", "fn main() {}\n"),
        ("app/lib.rs", ""),
        ("notes.txt", "hello\n"),
    ]);
    let root = dir.path();
    let mut app = cx.clone();
    let (workbench, cx) = open_root(cx, root);
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    let retry = |cx: &mut VisualTestContext| {
        cx.simulate_keystrokes("escape");
        let panel = focus_panel(&workbench, cx);
        select(&panel, root, "app", cx);
        cx.update(|window, cx| name_now(&panel, EditKind::Rename, "core", window, cx));
        cx.run_until_parked();
    };
    let undo = |cx: &mut VisualTestContext| {
        fs::rename(root.join("core"), root.join("app")).unwrap();
        let workbench = workbench.clone();
        workbench.update(cx, |workbench, cx| {
            workbench.follow_rename(&root.join("core"), &root.join("app"), cx)
        });
        let panel = panel.clone();
        panel.update(cx, |panel, cx| {
            let changes = [root.join("core"), root.join("app")].map(FsChange::Path);
            panel.apply_fs_changes(&changes, cx)
        });
        cx.run_until_parked();
    };

    // A Save As of notes.txt to app/b.txt.
    open_file(&workbench, &root.join("notes.txt"), cx);
    let notes = active_editor(&workbench, cx).unwrap();
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    let b = root.join("app/b.txt");
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.save_editor_as(&notes, b.clone(), window, cx)
        });
        name_now(&panel, EditKind::Rename, "core", window, cx);
    });
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“app” is being saved. Rename it once it’s saved.")
    );
    cx.run_until_parked();
    assert_eq!(editor_path(&notes, cx), Some(b));
    retry(cx);
    assert_eq!(editor_path(&notes, cx), Some(root.join("core/b.txt")));
    assert_eq!(
        fs::read_to_string(root.join("core/b.txt")).unwrap(),
        "hello\n"
    );
    undo(cx);

    // A save of app/main.rs, held by its hook, its tab closed meanwhile.
    open_file(&workbench, &root.join("app/main.rs"), cx);
    let main = active_editor(&workbench, cx).unwrap();
    cx.simulate_input("x");
    let saving = cx.update(|window, cx| {
        main.update(cx, |main, cx| {
            main.set_save_hook(Some(std::rc::Rc::new(HoldSaves)));
            main.save(window, cx)
        })
    });
    cx.simulate_keystrokes("cmd-w");
    if cx.has_pending_prompt() {
        cx.simulate_prompt_answer("Discard");
    }
    cx.run_until_parked();
    let open = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx));
    assert!(!open.contains(&main), "its tab closed");
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    cx.update(|window, cx| name_now(&panel, EditKind::Rename, "core", window, cx));
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“app” is being saved. Rename it once it’s saved.")
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    cx.run_until_parked();
    drop(saving);
    assert_eq!(
        fs::read_to_string(root.join("app/main.rs")).unwrap(),
        "xfn main() {}\n"
    );
    retry(cx);
    assert!(root.join("core/main.rs").is_file());
    assert!(!root.join("app").exists(), "nothing wrote the old path");
    undo(cx);

    // A file under it being read into a tab.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    let lib = root.join("app/lib.rs");
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| pane.open(lib.clone(), window, cx));
        name_now(&panel, EditKind::Rename, "core", window, cx);
    });
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“app” is being opened. Rename it once it’s open.")
    );
    cx.run_until_parked();
    let opened = active_editor(&workbench, cx).unwrap();
    assert_eq!(editor_path(&opened, cx), Some(lib));
    retry(cx);
    assert_eq!(editor_path(&opened, cx), Some(root.join("core/lib.rs")));
    undo(cx);

    // The panel's own create there: the new file opens where it was made.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    cx.update(|window, cx| {
        name_now(&panel, EditKind::NewFile, "y.rs", window, cx);
        name_now(&panel, EditKind::Rename, "core", window, cx);
    });
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“app” is being worked on. Rename it once that’s done.")
    );
    cx.run_until_parked();
    let y = active_editor(&workbench, cx).unwrap();
    assert_eq!(editor_path(&y, cx), Some(root.join("app/y.rs")));
    assert!(root.join("app/y.rs").is_file());
    retry(cx);
    assert_eq!(editor_path(&y, cx), Some(root.join("core/y.rs")));
    undo(cx);

    // A create started at the root whose name makes folders in it.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app", cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, _| panel.selected = None);
        name_now(&panel, EditKind::NewFile, "app/sub/z.rs", window, cx);
        panel.update(cx, |panel, cx| panel.reveal(root.join("app"), cx));
        name_now(&panel, EditKind::Rename, "core", window, cx);
    });
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“app” is being worked on. Rename it once that’s done.")
    );
    cx.run_until_parked();
    let z = active_editor(&workbench, cx).unwrap();
    assert_eq!(editor_path(&z, cx), Some(root.join("app/sub/z.rs")));
    retry(cx);
    assert_eq!(editor_path(&z, cx), Some(root.join("core/sub/z.rs")));
    undo(cx);

    // And its duplicate there: the copy is selected where it was made.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "app/lib.rs", cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.duplicate_selected(window, cx);
            panel.reveal(root.join("app"), cx);
        });
        name_now(&panel, EditKind::Rename, "core", window, cx);
    });
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“app” is being worked on. Rename it once that’s done.")
    );
    cx.run_until_parked();
    assert!(root.join("app/lib copy.rs").is_file());
    assert_eq!(selected(&panel, cx), Some((1, "lib copy.rs".into())));
    retry(cx);
    assert!(root.join("core/lib copy.rs").is_file());

    // A duplicate holds its folder, where its copy goes, until it lands:
    // so nothing beside it is renamed meanwhile.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "core/lib.rs", cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.duplicate_selected(window, cx);
            panel.reveal(root.join("core/main.rs"), cx);
        });
        name_now(&panel, EditKind::Rename, "start", window, cx);
    });
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“main.rs” is being worked on. Rename it once that’s done.")
    );
    cx.run_until_parked();
    assert!(root.join("core/lib copy 2.rs").is_file());
    assert_eq!(selected(&panel, cx), Some((1, "lib copy 2.rs".into())));
    cx.simulate_keystrokes("escape");

    // A save in another window, which closes while it runs.
    let (other, other_cx) = open_root(&mut app, root);
    open_file(&other, &root.join("core/main.rs"), other_cx);
    let theirs = active_editor(&other, other_cx).unwrap();
    other_cx.simulate_input("z");
    let saving = other_cx.update(|window, cx| {
        theirs.update(cx, |theirs, cx| {
            theirs.set_save_hook(Some(std::rc::Rc::new(HoldSaves)));
            theirs.save(window, cx)
        })
    });
    other_cx.run_until_parked();
    drop(other);
    other_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "core", cx);
    cx.update(|window, cx| name_now(&panel, EditKind::Rename, "app", window, cx));
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“core” is being saved. Rename it once it’s saved.")
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    cx.run_until_parked();
    drop(saving);
    assert!(
        fs::read_to_string(root.join("core/main.rs"))
            .unwrap()
            .starts_with("z")
    );
    cx.simulate_keystrokes("escape");
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "core", cx);
    cx.update(|window, cx| name_now(&panel, EditKind::Rename, "app", window, cx));
    assert_eq!(error(&panel, cx), None);
    assert!(root.join("app/main.rs").is_file());
    assert!(!root.join("core").exists());
}

/// Editors of the renamed file spelled through other folders (the project
/// opened through a symlink in one window, its real path in another)
/// follow it too, in every window; a symlinked file keeps its link.
#[gpui::test]
#[cfg(unix)]
fn rename_follows_other_spellings(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let real = dunce::canonicalize(dir.path()).unwrap().join("real");
    write_files(
        &real,
        &[("app/main.rs", "fn main() {}\n"), ("notes.txt", "hello\n")],
    );
    std::os::unix::fs::symlink("notes.txt", real.join("alias.txt")).unwrap();
    let link = real.with_file_name("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    // A symlink straight into the folder renamed below, which dangles once
    // it is.
    let into = real.with_file_name("into-app");
    std::os::unix::fs::symlink(real.join("app"), &into).unwrap();
    let mut app = cx.clone();
    // This window on the link, with app/main.rs open by its real path.
    let (workbench, cx) = open_root(cx, &link);
    open_file(&workbench, &real.join("app/main.rs"), cx);
    let main = active_editor(&workbench, cx).unwrap();
    // The other on the real path.
    let (other, other_cx) = open_root(&mut app, &real);
    open_file(&other, &real.join("notes.txt"), other_cx);
    let notes = active_editor(&other, other_cx).unwrap();
    open_file(&other, &into.join("main.rs"), other_cx);
    let via = active_editor(&other, other_cx).unwrap();
    open_file(&other, &real.join("alias.txt"), other_cx);
    let alias = active_editor(&other, other_cx).unwrap();

    // A save through alias.txt, which writes notes.txt, holds the rename.
    other_cx.simulate_input("y");
    let saving = other_cx.update(|window, cx| {
        alias.update(cx, |alias, cx| {
            alias.set_save_hook(Some(std::rc::Rc::new(HoldSaves)));
            alias.save(window, cx)
        })
    });
    other_cx.run_until_parked();
    let panel = focus_panel(&workbench, cx);
    select(&panel, &link, "notes.txt", cx);
    run("project_panel.rename", cx);
    cx.simulate_input("todo");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        error(&panel, cx).as_deref(),
        Some("“notes.txt” is being saved. Rename it once it’s saved.")
    );
    other_cx
        .executor()
        .advance_clock(std::time::Duration::from_secs(2));
    other_cx.run_until_parked();
    drop(saving);
    assert_eq!(
        fs::read_to_string(real.join("notes.txt")).unwrap(),
        "yhello\n"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(real.join("todo.txt").is_file());
    assert!(!real.join("notes.txt").exists());
    other_cx.run_until_parked();
    assert_eq!(editor_path(&notes, other_cx), Some(real.join("todo.txt")));
    assert_eq!(editor_path(&alias, other_cx), Some(real.join("alias.txt")));

    select(&panel, &link, "app", cx);
    run("project_panel.rename", cx);
    cx.simulate_input("core");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(real.join("core/main.rs").is_file());
    assert_eq!(editor_path(&main, cx), Some(real.join("core/main.rs")));
    // Spelled through the symlink into app, it follows too, by the real
    // path: the symlink dangles now.
    other_cx.run_until_parked();
    assert_eq!(editor_path(&via, other_cx), Some(real.join("core/main.rs")));
}

/// Opens from the tree waiting in two panes at once (one behind a Save As
/// of its file): each editor is in normal mode once it shows.
#[gpui::test]
fn deferred_tree_opens_in_two_panes(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "a\n"), ("b.txt", "b\n"), ("y.txt", "y\n")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    open_file(&workbench, &root.join("a.txt"), cx);
    let a = active_editor(&workbench, cx).unwrap();
    let left = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.simulate_keystrokes("cmd-\\");
    open_file(&workbench, &root.join("y.txt"), cx);
    let right = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_ne!(left, right);
    let panel = panel(&workbench, cx);

    // a.txt, in insert mode, saved as c.txt, and c.txt opened from the tree
    // in its pane, waiting for the save.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.activate(left.clone(), window, cx)
    });
    cx.update(|window, cx| window.focus(&a.focus_handle(cx), cx));
    cx.simulate_keystrokes("i");
    assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("insert"));
    let c = root.join("c.txt");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.save_editor_as(&a, c.clone(), window, cx);
        panel.update(cx, |_, cx| cx.emit(PanelEvent::OpenFile(c.clone())));
    });
    // Then, the save still held, b.txt opened from the tree in the other
    // pane.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.activate(right.clone(), window, cx);
        panel.update(cx, |_, cx| {
            cx.emit(PanelEvent::OpenFile(root.join("b.txt")))
        });
    });
    cx.run_until_parked();
    let shown = left.read_with(cx, |pane, _| pane.active_editor().cloned());
    assert_eq!(shown, Some(a.clone()));
    assert_eq!(editor_path(&a, cx), Some(c));
    assert_eq!(a.read_with(cx, |a, _| a.vim_mode()), Some("normal"));
    let b = right
        .read_with(cx, |pane, _| pane.active_editor().cloned())
        .unwrap();
    assert_eq!(editor_path(&b, cx), Some(root.join("b.txt")));
}

/// The panel's operations hold renames in every window, whichever spelling
/// of the project each is rooted at, and after their own window closes: a
/// duplicate started through a symlinked root holds renames through the
/// real one, of its source and of what's beside it, where its copy goes.
#[gpui::test]
#[cfg(unix)]
fn operations_hold_renames_across_spellings_and_windows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let real = dunce::canonicalize(dir.path()).unwrap().join("real");
    write_files(&real, &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    let link = real.with_file_name("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut app = cx.clone();
    let (through_link, link_cx) = open_root(cx, &link);
    let (through_real, real_cx) = open_root(&mut app, &real);
    let real_panel = focus_panel(&through_real, real_cx);
    select(&real_panel, &real, "a.txt", real_cx);
    let link_panel = focus_panel(&through_link, link_cx);
    select(&link_panel, &link, "a.txt", link_cx);
    let refused = |name: &str, cx: &mut VisualTestContext| {
        let message = format!("“{name}” is being worked on. Rename it once that’s done.");
        assert_eq!(error(&real_panel, cx), Some(message));
    };
    let rename_from_real = |rel: &str, to: &str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            real_panel.update(cx, |panel, cx| panel.reveal(real.join(rel), cx));
            name_now(&real_panel, EditKind::Rename, to, window, cx);
        });
    };

    // Through the link, a.txt is duplicated: through the real root, neither
    // it nor b.txt beside it is renamed until the copy is made.
    link_cx.update(|window, cx| {
        link_panel.update(cx, |panel, cx| panel.duplicate_selected(window, cx))
    });
    rename_from_real("a.txt", "x.txt", real_cx);
    refused("a.txt", real_cx);
    rename_from_real("b.txt", "x.txt", real_cx);
    refused("b.txt", real_cx);
    link_cx.run_until_parked();
    assert_eq!(fs::read_to_string(real.join("a copy.txt")).unwrap(), "a\n");
    rename_from_real("b.txt", "c.txt", real_cx);
    assert_eq!(field(&real_panel, real_cx), None);
    assert!(real.join("c.txt").is_file());

    // Started again, then its window closes: the duplicate goes on, and
    // still holds the rename until it's done.
    link_cx.update(|window, cx| {
        link_panel.update(cx, |panel, cx| panel.duplicate_selected(window, cx))
    });
    drop((through_link, link_panel));
    link_cx.update(|window, _| window.remove_window());
    rename_from_real("a.txt", "x.txt", real_cx);
    refused("a.txt", real_cx);
    real_cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(real.join("a copy 2.txt")).unwrap(),
        "a\n"
    );
    rename_from_real("a.txt", "x.txt", real_cx);
    assert_eq!(field(&real_panel, real_cx), None);
    assert!(!real.join("a.txt").exists());
    assert_eq!(fs::read_to_string(real.join("x.txt")).unwrap(), "a\n");
}

/// A rename into a name a pending copy or Save As holds as its destination
/// is refused until it's let go: the write would otherwise replace the file.
#[gpui::test]
fn rename_waits_for_a_held_destination(cx: &mut TestAppContext) {
    let dir = project(&[("notes.txt", "hello\n"), ("todo.txt", "")]);
    let root = dir.path();
    std::fs::remove_file(root.join("todo.txt")).unwrap();
    let (workbench, cx) = open_root(cx, root);
    let held = root.join("todo.txt");
    let buffer = fe_buffer::Buffer::new("");
    let reservation =
        crate::open::reserve(buffer.id(), held.clone(), crate::open::file_identity(&held));

    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "notes.txt", cx);
    run("project_panel.rename", cx);
    cx.simulate_input("todo");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let error = field(&panel, cx).and_then(|(_, error)| error);
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("being saved")),
        "{error:?}"
    );
    assert!(root.join("notes.txt").is_file());
    assert!(!held.exists());

    drop(reservation);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(held.is_file());
    assert!(!root.join("notes.txt").exists());
}

/// A `:w copy.txt` waiting behind a running save holds its destination: a
/// rename into it is refused meanwhile, and the destination is let go once
/// the copy is written.
#[gpui::test]
fn rename_waits_for_a_pending_copy(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "one\n"), ("notes.txt", "hello\n")]);
    let root = dir.path();
    let mut app = cx.clone();
    let (workbench, cx) = open_root(cx, root);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    open_file(&workbench, &root.join("a.txt"), cx);
    cx.simulate_keystrokes("x");
    // The copy stops inside its background write until released.
    let (release, gate) = futures::channel::oneshot::channel();
    fe_gpui::pane_item::pause_next_copy(gate);
    cx.simulate_keystrokes(":");
    cx.simulate_input("w copy.txt");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!root.join("copy.txt").exists(), "paused before it writes");

    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "notes.txt", cx);
    run("project_panel.rename", cx);
    cx.simulate_input("copy");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let error = field(&panel, cx).and_then(|(_, error)| error);
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("being saved")),
        "{error:?}"
    );
    assert!(root.join("notes.txt").is_file());

    // The tab and its window close (`:q!`); another window's panel is
    // still refused, as the copy goes on holding its destination.
    cx.simulate_keystrokes("escape");
    let (other, other_cx) = open_root(&mut app, root);
    open_file(&workbench, &root.join("a.txt"), cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input("q!");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(
        crate::open::reserved_paths().contains(&root.join("copy.txt")),
        "held after the window closed"
    );
    let other_panel = focus_panel(&other, other_cx);
    select(&other_panel, root, "notes.txt", other_cx);
    run("project_panel.rename", other_cx);
    other_cx.simulate_input("copy");
    other_cx.simulate_keystrokes("enter");
    other_cx.run_until_parked();
    let error = field(&other_panel, other_cx).and_then(|(_, error)| error);
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("being saved")),
        "{error:?}"
    );
    other_cx.simulate_keystrokes("escape");

    // Released: the copy is written and its destination let go.
    release.send(()).unwrap();
    other_cx.run_until_parked();
    assert_eq!(fs::read_to_string(root.join("copy.txt")).unwrap(), "ne\n");
    assert!(
        crate::open::reserved_paths().is_empty(),
        "let go once written"
    );
    let fresh = crate::open::reserve(fe_buffer::Buffer::new("").id(), root.join("copy.txt"), None);
    assert_eq!(crate::open::reserved_paths(), [root.join("copy.txt")]);
    drop(fresh);
}
