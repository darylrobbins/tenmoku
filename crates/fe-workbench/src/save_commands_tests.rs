//! Save All (`workbench.save_all`) and Save without Formatting
//! (`workbench.save_without_formatting`) in a headless workbench. The
//! project's `.editorconfig` trims trailing whitespace and adds a final
//! newline, as a save that formats would.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::EditorModel;
use fe_gpui::{EditorView, SaveHook, commands::RunCommand};
use gpui::{App, Entity, Task, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench, tests::open, workbench::toast::Level};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on a folder with `a.txt` to `d.txt` holding `x\n`.
fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
    )
    .unwrap();
    for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        fs::write(dir.path().join(name), "x\n").unwrap();
    }
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    draw(cx);
    (dir, workbench, cx)
}

fn open_file(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn editor_of(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        workbench
            .open_editors(cx)
            .into_iter()
            .find(|editor| editor.read(cx).model().path() == Some(path))
            .expect("the file is open")
    })
}

fn type_into(editor: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, _| {
        editor.model_mut().select_all();
        editor.model_mut().insert(text);
    });
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

/// The files whose save ran the hook, by name (`untitled` for none).
type Runs = Rc<RefCell<Vec<String>>>;

/// Stands for format on save and code actions on save: the text gets `;`
/// at its start, and the file is recorded.
struct Formats(Runs);

impl SaveHook for Formats {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut App) -> Task<()> {
        editor.update(cx, |view, cx| {
            let name = (view.model().path())
                .and_then(Path::file_name)
                .map_or("untitled".into(), |name| {
                    name.to_string_lossy().into_owned()
                });
            self.0.borrow_mut().push(name);
            view.model_mut().select_all();
            let text = view.model().text().to_string();
            view.model_mut().insert(&format!(";{text}"));
            cx.notify();
        });
        Task::ready(())
    }
}

/// Puts the hook on every editor open in the workbench, each view of a
/// shared buffer included.
fn format_on_save(workbench: &Entity<Workbench>, runs: &Runs, cx: &mut VisualTestContext) {
    let editors = workbench.read_with(cx, |workbench, cx| workbench.file_editors(cx));
    for editor in editors {
        editor.update(cx, |view, _| {
            view.set_save_hook(Some(Rc::new(Formats(runs.clone()))))
        });
    }
}

/// Each toast shown: its level and message.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Level, String)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| (entry.toast.level, entry.toast.message.clone()))
            .collect()
    })
}

fn dirty(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.model().is_dirty())
}

#[gpui::test]
fn save_all_writes_every_dirty_file(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx);
    let (a, b, c) = (
        dir.path().join("a.txt"),
        dir.path().join("b.txt"),
        dir.path().join("c.txt"),
    );
    open_file(&workbench, a.clone(), cx);
    // The split shows a again, sharing its buffer; b and c open in it.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.split(crate::Direction::Right, window, cx);
    });
    draw(cx);
    open_file(&workbench, b.clone(), cx);
    open_file(&workbench, c.clone(), cx);
    // A viewer that's read-only, though its text differs from the file's.
    let d = dir.path().join("d.txt");
    open_file(&workbench, d.clone(), cx);
    let viewer = editor_of(&workbench, &d, cx);
    type_into(&viewer, "viewed", cx);
    viewer.update(cx, |view, cx| view.set_read_only(true, cx));
    // An untitled buffer, dirty too.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let mut model = EditorModel::new(Buffer::new(""), None);
            model.insert("untitled");
            pane.add(model, cx);
            pane.activate(pane.tabs().len() - 1, window, cx);
        });
    });
    draw(cx);

    let runs = Runs::default();
    format_on_save(&workbench, &runs, cx);
    type_into(&editor_of(&workbench, &a, cx), "one  ", cx);
    type_into(&editor_of(&workbench, &b, cx), "two  ", cx);
    run("workbench.save_all", cx);

    // Each as an explicit save: the hook (format, code actions) and the
    // editorconfig's trim and final newline; a, shown twice, once.
    assert_eq!(fs::read_to_string(&a).unwrap(), ";one\n");
    assert_eq!(fs::read_to_string(&b).unwrap(), ";two\n");
    let mut ran = runs.borrow().clone();
    ran.sort();
    assert_eq!(
        ran,
        ["a.txt", "b.txt"],
        "the clean, read-only and untitled skip"
    );
    assert_eq!(fs::read_to_string(&c).unwrap(), "x\n");
    assert_eq!(fs::read_to_string(&d).unwrap(), "x\n");
    assert!(toasts(&workbench, cx).is_empty());
    assert!(!dirty(&editor_of(&workbench, &a, cx), cx));
    assert!(!dirty(&editor_of(&workbench, &b, cx), cx));
}

#[gpui::test]
fn save_all_reports_failures(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx);
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    open_file(&workbench, a.clone(), cx);
    open_file(&workbench, b.clone(), cx);
    type_into(&editor_of(&workbench, &a, cx), "mine", cx);
    type_into(&editor_of(&workbench, &b, cx), "two", cx);
    fs::write(&a, "theirs\n").unwrap();

    let task = workbench.update_in(cx, |workbench, window, cx| workbench.save_all(window, cx));
    cx.run_until_parked();
    // The changed file asks; the other is written meanwhile.
    assert!(cx.has_pending_prompt());
    assert_eq!(fs::read_to_string(&b).unwrap(), "two\n");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    cx.update(|_, cx| cx.foreground_executor().block_test(task));

    assert_eq!(fs::read_to_string(&a).unwrap(), "theirs\n");
    assert!(dirty(&editor_of(&workbench, &a, cx), cx));
    assert!(!dirty(&editor_of(&workbench, &b, cx), cx));
    // Declining to overwrite is the person's own answer: no toast.
    assert_eq!(toasts(&workbench, cx), []);

    // A save that fails is reported, and doesn't stop the other.
    let c = dir.path().join("c.txt");
    open_file(&workbench, c.clone(), cx);
    type_into(&editor_of(&workbench, &c, cx), "gone", cx);
    type_into(&editor_of(&workbench, &b, cx), "two again", cx);
    fs::remove_file(&c).unwrap();
    fs::create_dir(&c).unwrap();
    run("workbench.save_all", cx);
    // The first file still differs from the disk's, and asks again.
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(fs::read_to_string(&b).unwrap(), "two again\n");
    let shown = toasts(&workbench, cx);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0], (Level::Error, "Can't save c.txt".to_owned()));
}

#[gpui::test]
fn save_without_formatting(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx);
    let a = dir.path().join("a.txt");
    open_file(&workbench, a.clone(), cx);
    let editor = editor_of(&workbench, &a, cx);

    let runs = Runs::default();
    format_on_save(&workbench, &runs, cx);

    type_into(&editor, "raw  ", cx);
    run("workbench.save_without_formatting", cx);
    assert_eq!(fs::read_to_string(&a).unwrap(), "raw  ");
    assert!(runs.borrow().is_empty());
    assert!(!dirty(&editor, cx));

    // A plain save formats and applies the fixes.
    // (The frames drawn since have synced the editor's hook away.)
    format_on_save(&workbench, &runs, cx);
    type_into(&editor, "again  ", cx);
    run("editor.save", cx);
    assert_eq!(fs::read_to_string(&a).unwrap(), ";again\n");
    assert_eq!(*runs.borrow(), ["a.txt"]);
}
