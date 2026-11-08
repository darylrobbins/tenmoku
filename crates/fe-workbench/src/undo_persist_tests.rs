//! Persistent undo (`editor.persistent_undo`): a saved file's undo history
//! is kept when its tab closes, after each save and as the app quits, and
//! `u` reaches it when the file opens again, unless the file changed on disk
//! meanwhile. Each window keeps it as its own root's settings say.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::{Buffer, Origin, ResolvedSelection, UndoStore};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext, WindowId};

use crate::{Launch, Workbench};

struct Fixture {
    workbench: Entity<Workbench>,
    cx: VisualTestContext,
    window: WindowId,
    store: UndoStore,
    path: PathBuf,
    _root: tempfile::TempDir,
    _state: tempfile::TempDir,
}

/// A workbench rooted where `a.txt` holds `one\n`, with vim on and an undo
/// store installed.
fn fixture(cx: &mut TestAppContext) -> Fixture {
    let state = tempfile::tempdir().unwrap();
    let store = UndoStore::new(state.path().join("undo"));
    let installed = store.clone();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        fe_gpui::pane_item::set_undo_store(cx, Some(installed));
    });
    let (root, path, workbench, mut vcx) = window(cx);
    let window = window_id(&mut vcx);
    Fixture {
        workbench,
        cx: vcx,
        window,
        store,
        path,
        _root: root,
        _state: state,
    }
}

/// Another window, on a root of its own where `a.txt` holds `one\n`.
fn window(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Workbench>,
    VisualTestContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let launch = Launch {
        root: Some(root.path().to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, _| window.activate_window());
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    vcx.update(|_, cx| set_vim_enabled(cx, true));
    draw(&mut vcx);
    (root, path, workbench, vcx)
}

fn window_id(cx: &mut VisualTestContext) -> WindowId {
    cx.update(|window, _| window.window_handle().window_id())
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn user(origin: &Origin) -> bool {
    *origin == Origin::User
}

impl Fixture {
    /// Opens `a.txt`, focused, once its undo history is read.
    fn open(&mut self) -> Entity<EditorView> {
        let path = self.path.clone();
        self.open_path(path)
    }

    fn open_path(&mut self, path: PathBuf) -> Entity<EditorView> {
        let pane = self.workbench.read_with(&self.cx, |w, _| w.pane().clone());
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(&mut self.cx);
        self.active().expect("open")
    }

    fn active(&mut self) -> Option<Entity<EditorView>> {
        self.workbench
            .read_with(&self.cx, |w, cx| w.active_editor(cx).cloned())
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        draw(&mut self.cx);
    }

    /// Appends `text` to the caret's line, in vim.
    fn append(&mut self, text: &str) {
        self.keys("A");
        self.cx.simulate_input(text);
        self.keys("escape");
    }

    /// Types `:{line}` and Enter.
    fn ex(&mut self, line: &str) {
        self.cx.simulate_keystrokes(":");
        self.cx.simulate_input(line);
        self.keys("enter");
    }

    fn run(&mut self, id: &str) {
        let run = RunCommand(
            Invocation::of(id)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
    }

    fn close_tab(&mut self) {
        self.run("workbench.close_tab");
        draw(&mut self.cx);
        assert!(self.active().is_none(), "the tab closed");
    }

    fn text(&self, editor: &Entity<EditorView>) -> String {
        editor.read_with(&self.cx, |editor, _| editor.model().text().to_string())
    }

    fn caret(&self, editor: &Entity<EditorView>) -> usize {
        editor.read_with(&self.cx, |editor, _| {
            editor.model().primary_selection().head()
        })
    }

    fn dirty(&self, editor: &Entity<EditorView>) -> bool {
        editor.read_with(&self.cx, |editor, _| editor.model().is_dirty())
    }

    fn has_history(&self, editor: &Entity<EditorView>) -> bool {
        editor.read_with(&self.cx, |editor, _| {
            editor.model().buffer().history_revision() != 0
        })
    }

    fn buffer_id(&self, editor: &Entity<EditorView>) -> fe_buffer::BufferId {
        editor.read_with(&self.cx, |editor, _| editor.model().buffer().id())
    }

    fn record_file(&self) -> PathBuf {
        self.store.file_of(&self.path).unwrap()
    }

    fn wait_for_window_undo(&mut self) {
        let window = self.window;
        self.cx.update(|_, cx| {
            fe_gpui::pane_item::wait_for_window_persistent_undo(window, cx);
        });
    }

    /// Writes `a.txt` as `one two\n`, with the history of typing ` two` in
    /// the store.
    fn write_typed_history(&self) {
        let mut typed = Buffer::new("one\n");
        typed.edit([(3..3, " two")], Origin::User);
        self.store
            .write(&self.path, &typed.export_undo().unwrap())
            .unwrap();
        fs::write(&self.path, "one two\n").unwrap();
    }

    fn set_window_undo(&mut self, on: bool) {
        let window = self.window;
        self.cx.update(|_, cx| {
            fe_gpui::pane_item::set_window_persistent_undo(window, Some(on), cx);
        });
    }

    /// Opens `a.txt`, appends ` two` to its line, saves it and closes it.
    fn edit_save_and_close(&mut self) {
        let editor = self.open();
        self.append(" two");
        self.ex("w");
        assert_eq!(fs::read_to_string(&self.path).unwrap(), "one two\n");
        assert_eq!(self.text(&editor), "one two\n");
        self.close_tab();
    }

    /// Two editors over one buffer of `a.txt`, holding `one two\n` with the
    /// undo history of typing ` two` on disk, made in one update so the
    /// history is still being read when `with` runs on them.
    fn two_editors_reading(
        &mut self,
        with: impl FnOnce(&Entity<EditorView>, &Entity<EditorView>, &mut gpui::App),
    ) -> (Entity<EditorView>, Entity<EditorView>) {
        self.two_editors_reading_typed("one\n", 3, " two", with)
    }

    /// Two editors as [`Fixture::two_editors_reading`] makes, over `before`
    /// with `typed` inserted at `at`.
    fn two_editors_reading_typed(
        &mut self,
        before: &str,
        at: usize,
        typed: &str,
        with: impl FnOnce(&Entity<EditorView>, &Entity<EditorView>, &mut gpui::App),
    ) -> (Entity<EditorView>, Entity<EditorView>) {
        let mut buffer = Buffer::new(before);
        buffer.edit([(at..at, typed)], Origin::User);
        self.store
            .write(&self.path, &buffer.export_undo().unwrap())
            .unwrap();
        fs::write(&self.path, buffer.text().to_string()).unwrap();
        let path = self.path.clone();
        self.cx.update(|_, cx| {
            let buffer = Buffer::from_file(fs::File::open(&path).unwrap()).unwrap();
            let model = EditorModel::new(buffer, Some(path));
            let split = model.split();
            let theme = EditorTheme::default_dark(cx);
            let first = cx.new(|cx| EditorView::new(model, theme.clone(), cx));
            let second = cx.new(|cx| EditorView::new(split, theme, cx));
            with(&first, &second, cx);
            (first, second)
        })
    }

    /// A second window on a root of its own, sharing this one's store.
    fn another_window(&self, cx: &mut TestAppContext) -> Fixture {
        let (root, path, workbench, mut vcx) = window(cx);
        Fixture {
            workbench,
            window: window_id(&mut vcx),
            cx: vcx,
            store: self.store.clone(),
            path,
            _root: root,
            _state: tempfile::tempdir().unwrap(),
        }
    }

    /// Restores an editor of `a.txt` in this window, as a saved layout does.
    fn restore(&mut self) -> Entity<EditorView> {
        use kiln_shell::PaneItem as _;
        let state = fe_command::Value::from_iter([(
            "path",
            fe_command::Value::from(self.path.to_str().unwrap()),
        )]);
        let restored = std::rc::Rc::new(std::cell::RefCell::new(None));
        let slot = restored.clone();
        self.cx.update(|window, cx| {
            let task = EditorView::restore(state, window, cx);
            cx.spawn(async move |_| *slot.borrow_mut() = task.await)
                .detach();
        });
        draw(&mut self.cx);
        restored.borrow_mut().take().expect("restored")
    }
}

/// The records in the store, its lock aside.
fn store_files(dir: &Path) -> usize {
    fs::read_dir(dir).map_or(0, |items| {
        items
            .filter(|item| item.as_ref().unwrap().file_name() != ".lock")
            .count()
    })
}

#[gpui::test]
fn reopening_a_saved_file_lets_u_undo_the_last_edit(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let first = f.open();
    let first_buffer = f.buffer_id(&first);
    drop(first);
    f.close_tab();
    f.edit_save_and_close();
    assert!(f.record_file().exists(), "the save wrote the history");

    let editor = f.open();
    assert_ne!(f.buffer_id(&editor), first_buffer, "a buffer read afresh");
    assert_eq!(f.text(&editor), "one two\n");
    assert!(!f.dirty(&editor));
    assert_eq!(f.caret(&editor), 0, "the caret stays where the file opens");

    f.keys("u");
    assert_eq!(
        f.text(&editor),
        "one\n",
        "u undoes the edit made before closing"
    );
    f.keys("u");
    assert_eq!(f.text(&editor), "one\n", "and there's nothing older");
    f.keys("ctrl-r");
    assert_eq!(f.text(&editor), "one two\n", "<C-r> redoes it");
    assert!(!f.dirty(&editor));

    // Undone and closed clean: the redo is kept too.
    f.keys("u");
    f.ex("w");
    f.close_tab();
    let editor = f.open();
    assert_eq!(f.text(&editor), "one\n");
    f.keys("ctrl-r");
    assert_eq!(
        f.text(&editor),
        "one two\n",
        "the undone edit redoes after reopening"
    );
}

#[gpui::test]
fn a_file_changed_on_disk_between_opens_starts_with_no_history(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    f.edit_save_and_close();
    let kept = fs::read(f.record_file()).unwrap();
    fs::write(&f.path, "one two three\n").unwrap();

    let editor = f.open();
    assert_eq!(f.text(&editor), "one two three\n");
    assert!(!f.has_history(&editor), "the stale history isn't used");
    f.keys("u");
    assert_eq!(f.text(&editor), "one two three\n");
    assert_eq!(
        fs::read(f.record_file()).unwrap(),
        kept,
        "nor removed on reading"
    );
    // Closed with no history of its own, the record stays as it was.
    f.close_tab();
    assert_eq!(fs::read(f.record_file()).unwrap(), kept);

    // The next save replaces it with the history of the text as it is.
    let _editor = f.open();
    f.append(" four");
    f.ex("w");
    f.close_tab();
    let editor = f.open();
    f.keys("u");
    assert_eq!(f.text(&editor), "one two three\n");
}

#[gpui::test]
fn setting_off_writes_nothing(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    // As the window's root settings give `editor.persistent_undo = false`.
    f.set_window_undo(false);
    f.edit_save_and_close();
    assert_eq!(
        store_files(f.store.dir()),
        0,
        "nothing written on save or close"
    );

    // On again, a history is written; off, it isn't read, or removed.
    f.set_window_undo(true);
    let _editor = f.open();
    f.append(" three");
    f.ex("w");
    f.close_tab();
    let kept = fs::read(f.record_file()).unwrap();
    f.set_window_undo(false);
    let editor = f.open();
    assert!(!f.has_history(&editor), "not read while off");
    f.keys("u");
    assert_eq!(f.text(&editor), "one two three\n");
    f.append(" four");
    f.ex("w");
    f.close_tab();
    f.cx.quit();
    assert_eq!(fs::read(f.record_file()).unwrap(), kept, "kept as it was");
    assert_eq!(store_files(f.store.dir()), 1);
}

/// Two windows, one whose root has `persistent_undo` off: only the other
/// keeps its file's history (review F1).
#[gpui::test]
fn a_window_with_it_off_writes_nothing_while_another_writes(cx: &mut TestAppContext) {
    let mut on = fixture(cx);
    let mut off = on.another_window(cx);
    off.set_window_undo(false);
    on.set_window_undo(true);
    off.edit_save_and_close();
    on.edit_save_and_close();
    assert!(on.record_file().exists());
    assert!(!off.record_file().exists(), "the off window wrote nothing");
    assert_eq!(store_files(on.store.dir()), 1);
}

/// An editor restored from a saved layout, made outside its window's
/// update, keeps undo history as its window says: none read in a window
/// with it off, while another has it on (review round 2 F1).
#[gpui::test]
fn a_restored_editor_follows_its_windows_setting(cx: &mut TestAppContext) {
    let mut on = fixture(cx);
    let mut off = on.another_window(cx);
    on.set_window_undo(true);
    off.set_window_undo(false);
    for f in [&mut on, &mut off] {
        let mut typed = Buffer::new("one\n");
        typed.edit([(3..3, " two")], Origin::User);
        f.store
            .write(&f.path, &typed.export_undo().unwrap())
            .unwrap();
        fs::write(&f.path, "one two\n").unwrap();
    }
    let restored = off.restore();
    assert!(!off.has_history(&restored), "not read in the off window");
    let restored = on.restore();
    assert!(on.has_history(&restored), "read in the on window");
}

/// Closing a window with it off, holding a clean buffer whose history
/// changed since its save, writes nothing, while another window has it on
/// (review round 2 F1).
#[gpui::test]
fn closing_a_window_with_it_off_writes_nothing(cx: &mut TestAppContext) {
    let on = fixture(cx);
    let mut off = on.another_window(cx);
    off.set_window_undo(false);
    let editor = off.open();
    off.append(" two");
    off.ex("w");
    off.append(" three");
    off.keys("u");
    assert!(!off.dirty(&editor), "clean, with a redo");
    drop(editor);
    off.cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert!(!off.record_file().exists(), "nothing written as it closed");
    drop(on);
    cx.quit();
    assert!(!off.record_file().exists(), "nor as the app quit");
}

/// An untitled buffer saved for the first time keeps its history, from the
/// save and from closing (review F2).
#[gpui::test]
fn a_saved_untitled_buffer_keeps_its_history(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    f.run("workbench.new_file");
    draw(&mut f.cx);
    let editor = f.active().expect("an untitled editor");
    f.keys("i");
    f.cx.simulate_input("hello");
    f.keys("escape");
    let path = f.path.with_file_name("new.txt");
    let target = path.clone();
    f.cx.update(|window, cx| {
        editor
            .update(cx, |editor, cx| editor.save_as(target, window, cx))
            .detach();
    });
    draw(&mut f.cx);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    assert!(f.store.read(&path).unwrap().is_some(), "Save As wrote it");
    fs::remove_file(f.store.file_of(&path).unwrap()).unwrap();
    drop(editor);
    f.close_tab();
    assert!(f.store.read(&path).unwrap().is_some(), "and so did closing");

    let editor = f.open_path(path);
    f.keys("u");
    assert_eq!(f.text(&editor), "", "u undoes the typing");
}

/// Reopened while the closing editor's write still waits, the file gets
/// the history that write holds, not the older one on disk; quitting
/// writes it (review F3). A directory where the record goes holds the
/// write back meanwhile.
#[gpui::test]
fn reopening_while_the_close_write_waits_reads_it(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let editor = f.open();
    let first = f.buffer_id(&editor);
    f.append(" two");
    f.ex("w");
    assert!(f.record_file().exists());
    fs::remove_file(f.record_file()).unwrap();
    fs::create_dir(f.record_file()).unwrap();
    f.append(" three");
    f.keys("u");
    assert!(!f.dirty(&editor), "back at the saved text, with a redo");
    drop(editor);
    f.close_tab();
    assert!(
        f.record_file().is_dir(),
        "the close's write is still waiting"
    );

    let editor = f.open();
    assert_ne!(f.buffer_id(&editor), first);
    f.keys("ctrl-r");
    assert_eq!(f.text(&editor), "one two three\n", "the redo came with it");
    fs::remove_dir(f.record_file()).unwrap();
    f.cx.quit();
    let record = f.store.read(&f.path).unwrap().expect("written as it quit");
    assert_eq!(record.entries.len(), 2);
}

/// Quitting writes the history of each clean buffer still open, which may
/// have changed since its save, and every write waiting, before the app
/// exits (review F4).
#[gpui::test]
fn quitting_writes_the_history_of_clean_buffers(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let editor = f.open();
    f.append(" two");
    f.ex("w");
    assert_eq!(f.store.read(&f.path).unwrap().unwrap().entries.len(), 1);
    f.append(" three");
    f.keys("u");
    assert!(!f.dirty(&editor));
    f.cx.quit();
    let record = f.store.read(&f.path).unwrap().expect("written as it quit");
    let undone: Vec<_> = record.entries.iter().map(|entry| entry.undone).collect();
    assert_eq!(undone, [false, true], "with the redo made since the save");
}

/// Typing while a save is being written leaves the buffer dirty, but the
/// history up to the text saved is still written, and fits that text
/// (review F5).
#[gpui::test]
fn typing_during_a_save_still_writes_the_saved_history(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let editor = f.open();
    f.append(" two");
    f.cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.save(window, cx).detach();
            editor.model_mut().insert("!");
        });
    });
    draw(&mut f.cx);
    assert_eq!(fs::read_to_string(&f.path).unwrap(), "one two\n");
    assert!(f.dirty(&editor), "the typing since is unsaved");
    let record = f.store.read(&f.path).unwrap().expect("the save wrote it");
    let mut reopened = Buffer::from_file(fs::File::open(&f.path).unwrap()).unwrap();
    reopened
        .import_undo(record)
        .expect("it ends at the text saved");
    reopened.undo(user).unwrap();
    assert_eq!(reopened.text().to_string(), "one\n");
}

/// A save leaving no user history to keep removes the file's record, so an
/// older one can't come back for the same text (review F6).
#[gpui::test]
fn a_save_with_no_user_history_left_removes_the_record(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    f.edit_save_and_close();
    assert!(f.record_file().exists());
    let editor = f.open();
    assert!(f.has_history(&editor));
    let agent = Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    editor.update(&mut f.cx, |editor, cx| {
        editor.model_mut().buffer_mut().edit([(7..7, "!")], agent);
        cx.notify();
    });
    f.ex("w");
    assert_eq!(fs::read_to_string(&f.path).unwrap(), "one two!\n");
    assert!(
        !f.record_file().exists(),
        "the AI's edit is newest: nothing to keep"
    );
    f.close_tab();
    assert!(!f.record_file().exists());
}

/// The first editor closing while the history is read leaves it to the
/// buffer's other editor (review F7).
#[gpui::test]
fn a_split_left_open_gets_the_history_read_for_the_first(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let (first, second) = f.two_editors_reading(|_, _, _| {});
    drop(first);
    draw(&mut f.cx);
    assert!(f.has_history(&second), "imported through the split");
    second.update(&mut f.cx, |editor, _| {
        editor.model_mut().buffer_mut().undo(user).unwrap();
    });
    assert_eq!(f.text(&second), "one\n");
}

/// Every editor of the buffer keeps its selections, reversed ones too,
/// through the edits that rebuild the history (review F8).
#[gpui::test]
fn every_editor_of_the_buffer_keeps_its_selections(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let caret = ResolvedSelection {
        range: 5..5,
        reversed: false,
    };
    let reversed = ResolvedSelection {
        range: 4..7,
        reversed: true,
    };
    let (first, second) = f.two_editors_reading(|first, second, cx| {
        first.update(cx, |editor, _| {
            editor.model_mut().set_selections([caret.clone()])
        });
        second.update(cx, |editor, _| {
            editor.model_mut().set_selections([reversed.clone()])
        });
    });
    draw(&mut f.cx);
    assert!(f.has_history(&first));
    let selections = |editor: &Entity<EditorView>, f: &Fixture| {
        editor.read_with(&f.cx, |editor, _| editor.model().selections())
    };
    assert_eq!(selections(&first, &f), [caret]);
    assert_eq!(selections(&second, &f), [reversed]);
}

/// The primary selection, which Select Next Occurrence made the first by
/// wrapping, stays primary through the import, and a vertical move's goal
/// column stays (review round 2 F8).
#[gpui::test]
fn the_primary_selection_and_movement_goals_survive_the_import(cx: &mut TestAppContext) {
    use fe_editor::Movement;
    let mut f = fixture(cx);
    // `ab ab` over `ab`: the history rebuilds all of it.
    let (first, second) =
        f.two_editors_reading_typed("\n", 0, "ab ab\nab\n", |first, second, cx| {
            first.update(cx, |editor, _| {
                let model = editor.model_mut();
                model.set_cursor(3);
                model.select_next_occurrence();
                model.select_next_occurrence();
                model.select_next_occurrence();
            });
            second.update(cx, |editor, _| {
                let model = editor.model_mut();
                model.set_cursor(4);
                model.move_cursors(Movement::Down, false);
            });
        });
    let read = |editor: &Entity<EditorView>, f: &Fixture| {
        editor.read_with(&f.cx, |editor, _| {
            let model = editor.model();
            (model.selections(), model.primary_selection())
        })
    };
    let before = (read(&first, &f), read(&second, &f));
    assert_eq!(before.0.1.range, 0..2, "the wrapped match is primary");
    assert_eq!(before.1.1.range, 8..8, "the caret clamped to the short row");
    draw(&mut f.cx);
    assert!(f.has_history(&first));
    assert_eq!((read(&first, &f), read(&second, &f)), before);
    second.update(&mut f.cx, |editor, _| {
        editor.model_mut().move_cursors(Movement::Up, false);
    });
    assert_eq!(f.caret(&second), 4, "back to the goal column");
}

/// A window whose root's settings are still loading reads and writes no
/// undo history; once they're in, a restored editor's read goes ahead if
/// they have it on, and not if off, whatever the launch root has
/// (review round 3 F2).
#[gpui::test]
fn a_window_waiting_for_its_settings_reads_once_they_are_in(cx: &mut TestAppContext) {
    let mut on = fixture(cx);
    let mut off = on.another_window(cx);
    for f in [&mut on, &mut off] {
        f.write_typed_history();
        f.wait_for_window_undo();
    }
    let restored_off = off.restore();
    let restored_on = on.restore();
    assert!(!off.has_history(&restored_off), "not before its settings");
    assert!(!on.has_history(&restored_on), "not before its settings");
    off.set_window_undo(false);
    draw(&mut off.cx);
    assert!(!off.has_history(&restored_off), "its root has it off");
    on.set_window_undo(true);
    draw(&mut on.cx);
    assert!(on.has_history(&restored_on), "read once its root has it on");
}

/// After reopening, `u` puts the caret at the start of the text the
/// undone change gives back, for a deletion and for a replacement, as no
/// editor recorded where its caret was (review round 3 F3).
#[gpui::test]
fn undo_after_reopening_puts_the_caret_at_the_start_of_the_change(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    fs::write(&f.path, "abcDEF\n").unwrap();
    let _editor = f.open();
    f.keys("3 x");
    f.ex("w");
    assert_eq!(fs::read_to_string(&f.path).unwrap(), "DEF\n");
    f.close_tab();
    let editor = f.open();
    f.keys("u");
    assert_eq!(f.text(&editor), "abcDEF\n");
    assert_eq!(f.caret(&editor), 0, "on the a, not the D");
    f.keys("ctrl-r");
    assert_eq!(f.text(&editor), "DEF\n");
    assert_eq!(f.caret(&editor), 0);

    fs::write(&f.path, "one two\n").unwrap();
    f.close_tab();
    let _editor = f.open();
    f.keys("w c w");
    f.cx.simulate_input("six");
    f.keys("escape");
    f.ex("w");
    assert_eq!(fs::read_to_string(&f.path).unwrap(), "one six\n");
    f.close_tab();
    let editor = f.open();
    f.keys("u");
    assert_eq!(f.text(&editor), "one two\n");
    assert_eq!(f.caret(&editor), 4, "at the start of two");
}

/// A write that failed while the window had it on, still waiting when the
/// window turns it off, isn't written as the app quits (review round 3 F4).
#[gpui::test]
fn quitting_drops_waiting_writes_of_a_window_since_turned_off(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    fs::create_dir_all(f.store.dir()).unwrap();
    fs::create_dir(f.record_file()).unwrap();
    let _editor = f.open();
    f.append(" two");
    f.ex("w");
    assert!(f.record_file().is_dir(), "the write is held back");
    f.set_window_undo(false);
    fs::remove_dir(f.record_file()).unwrap();
    f.cx.quit();
    assert!(!f.record_file().exists(), "not written once off");
}

impl Fixture {
    /// Runs the writers held back, as they would once released.
    fn release_writers(&mut self) {
        self.cx
            .update(|_, cx| fe_gpui::pane_item::flush_undo_writes_for_test(cx));
    }
}

/// A write held back while the window had it on, released after it turned
/// it off with nothing quitting, writes nothing (T507).
#[gpui::test]
fn a_queued_write_of_a_window_turned_off_is_not_written(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    fs::create_dir_all(f.store.dir()).unwrap();
    fs::create_dir(f.record_file()).unwrap();
    let _editor = f.open();
    f.append(" two");
    f.ex("w");
    assert!(f.record_file().is_dir(), "the write is held back");
    f.set_window_undo(false);
    fs::remove_dir(f.record_file()).unwrap();
    f.release_writers();
    assert!(!f.record_file().exists(), "not created once off");

    // An existing record isn't overwritten either.
    let mut old = Buffer::new("x");
    old.edit([(1..1, "y")], Origin::User);
    let old = old.export_undo().unwrap();
    f.set_window_undo(true);
    f.append(" three");
    fs::remove_file(f.record_file()).ok();
    fs::create_dir(f.record_file()).unwrap();
    f.ex("w");
    f.set_window_undo(false);
    fs::remove_dir(f.record_file()).unwrap();
    f.store.write(&f.path, &old).unwrap();
    let kept = fs::read(f.record_file()).unwrap();
    f.release_writers();
    assert_eq!(fs::read(f.record_file()).unwrap(), kept, "not overwritten");
}

/// Giving a window no setting of its own, with the host allowing it, keeps
/// the writes waiting for it (T507 review F2).
#[gpui::test]
fn a_queued_write_survives_a_window_inheriting_an_enabled_setting(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    f.set_window_undo(true);
    fs::create_dir_all(f.store.dir()).unwrap();
    fs::create_dir(f.record_file()).unwrap();
    let _editor = f.open();
    f.append(" two");
    f.ex("w");
    assert!(f.record_file().is_dir(), "the write is held back");
    let window = f.window;
    f.cx.update(|_, cx| fe_gpui::pane_item::set_window_persistent_undo(window, None, cx));
    fs::remove_dir(f.record_file()).unwrap();
    f.release_writers();
    assert!(f.record_file().exists(), "still written");
}

/// The host's setting turning persistence on after a window inherited it
/// off lets that window's writes through (T507 review F3).
#[gpui::test]
fn enabling_the_host_setting_lets_an_inheriting_window_write(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let window = f.window;
    let off = EditorSettings {
        persistent_undo: false,
        ..EditorSettings::default()
    };
    f.cx.update(|_, cx| {
        let theme = EditorTheme::default_dark(cx);
        fe_gpui::pane_item::set_restore_style(cx, theme, off);
        fe_gpui::pane_item::set_window_persistent_undo(window, None, cx);
    });
    let on = EditorSettings::default();
    f.cx.update(|_, cx| {
        let theme = EditorTheme::default_dark(cx);
        fe_gpui::pane_item::set_restore_style(cx, theme, on);
    });
    f.edit_save_and_close();
    assert!(f.record_file().exists(), "written once the host allows it");
}

/// The same for a removal: a save leaving no user history to keep.
#[gpui::test]
fn a_queued_removal_of_a_window_turned_off_removes_nothing(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    f.edit_save_and_close();
    let kept = fs::read(f.record_file()).unwrap();
    let editor = f.open();
    let agent = Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    editor.update(&mut f.cx, |editor, cx| {
        editor.model_mut().buffer_mut().edit([(7..7, "!")], agent);
        cx.notify();
    });
    // Hold the removal back: a directory can't be removed as a file.
    fs::remove_file(f.record_file()).unwrap();
    fs::create_dir(f.record_file()).unwrap();
    f.ex("w");
    assert!(f.record_file().is_dir(), "the removal is held back");
    f.set_window_undo(false);
    fs::remove_dir(f.record_file()).unwrap();
    fs::write(f.record_file(), &kept).unwrap();
    f.release_writers();
    assert_eq!(fs::read(f.record_file()).unwrap(), kept, "not removed");
}

/// The same for a window that started waiting for its settings.
#[gpui::test]
fn a_queued_write_of_a_window_waiting_for_its_settings_is_not_written(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    fs::create_dir_all(f.store.dir()).unwrap();
    fs::create_dir(f.record_file()).unwrap();
    let _editor = f.open();
    f.append(" two");
    f.ex("w");
    assert!(f.record_file().is_dir(), "the write is held back");
    f.wait_for_window_undo();
    fs::remove_dir(f.record_file()).unwrap();
    f.release_writers();
    assert!(!f.record_file().exists(), "not written while waiting");
}

/// A read landing in an open transaction that has changed nothing, as vim's
/// insert mode opens one, still imports: the history goes in beneath it. The
/// buffer's text and the record on disk are left as they were.
#[gpui::test]
fn a_read_landing_in_an_open_empty_transaction_keeps_the_record(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let (first, second) = f.two_editors_reading(|first, _, cx| {
        first.update(cx, |editor, _| {
            editor
                .model_mut()
                .buffer_mut()
                .start_transaction(Origin::User);
        });
    });
    let kept = fs::read(f.record_file()).unwrap();
    draw(&mut f.cx);
    assert_eq!(f.text(&first), "one two\n", "the text is as it was");
    first.update(&mut f.cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        assert!(buffer.open_transaction().is_some(), "still open");
        buffer.end_transaction();
        assert!(buffer.undo(user).unwrap().is_some(), "history imported");
    });
    assert_eq!(f.text(&first), "one\n", "undid the persisted edit");
    drop((first, second));
    draw(&mut f.cx);
    assert_eq!(
        fs::read(f.record_file()).unwrap(),
        kept,
        "the record is as it was"
    );
}

/// Vim insert mode entered before the read lands, then left without typing,
/// still gets the history, and `u` undoes the persisted edit.
#[gpui::test]
fn insert_mode_entered_before_the_read_lands_still_gets_the_history(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let (first, second) = f.two_editors_reading(|first, _, cx| {
        first.update(cx, |editor, _| {
            editor
                .model_mut()
                .buffer_mut()
                .start_transaction(Origin::User);
        });
    });
    draw(&mut f.cx);
    first.update(&mut f.cx, |editor, _| {
        editor.model_mut().buffer_mut().end_transaction();
    });
    assert_eq!(f.text(&first), "one two\n");
    first.update(&mut f.cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        assert!(buffer.undo(user).unwrap().is_some(), "history imported");
    });
    assert_eq!(f.text(&first), "one\n");
    drop(second);
}

/// An import is still refused once the open transaction has a typed edit,
/// and `u` undoes only that edit.
#[gpui::test]
fn typing_before_the_read_lands_still_gets_no_history(cx: &mut TestAppContext) {
    let mut f = fixture(cx);
    let (first, second) = f.two_editors_reading(|first, _, cx| {
        first.update(cx, |editor, _| {
            let mut buffer = editor.model_mut().buffer_mut();
            buffer.start_transaction(Origin::User);
            buffer.edit([(0..0, "x")], Origin::User);
        });
    });
    draw(&mut f.cx);
    first.update(&mut f.cx, |editor, _| {
        let mut buffer = editor.model_mut().buffer_mut();
        buffer.end_transaction();
        assert!(buffer.undo(user).unwrap().is_some(), "the typed edit");
        assert!(buffer.undo(user).unwrap().is_none(), "no imported history");
    });
    assert_eq!(f.text(&first), "one two\n");
    drop(second);
}

/// Sweeps the executor-stepping interleavings of a read in flight when its
/// window re-roots: for `k = 0, 1, …` makes the editors reading `a.txt`'s
/// record in a fresh fixture, runs `k` single tasks, then calls `body` with
/// whether the history was already imported. Stops once the executor ran
/// dry before `k` tasks.
fn sweep_re_root(
    cx: &mut TestAppContext,
    mut body: impl FnMut(&mut Fixture, &Entity<EditorView>, bool),
) {
    for k in 0..200 {
        let mut f = fixture(cx);
        let (first, second) = f.two_editors_reading(|_, _, _| {});
        let mut dry = false;
        for _ in 0..k {
            if !f.cx.executor().tick() {
                dry = true;
                break;
            }
        }
        let imported = f.has_history(&first);
        body(&mut f, &first, imported);
        drop((first, second));
        draw(&mut f.cx);
        if dry {
            return;
        }
    }
    panic!("the executor never ran dry in 200 steps");
}

/// A read started before the window re-roots and landing during the wait
/// is imported once the new root's settings have it on.
#[gpui::test]
fn a_read_in_flight_when_the_window_re_roots_is_imported_once_its_settings_are_in(
    cx: &mut TestAppContext,
) {
    sweep_re_root(cx, |f, first, _| {
        f.wait_for_window_undo();
        draw(&mut f.cx);
        f.set_window_undo(true);
        draw(&mut f.cx);
        assert!(f.has_history(first), "history imported");
        first.update(&mut f.cx, |editor, _| {
            let mut buffer = editor.model_mut().buffer_mut();
            assert!(buffer.undo(user).unwrap().is_some());
        });
        assert_eq!(f.text(first), "one\n");
    });
}

/// The same read, with the new root having it off, imports nothing and
/// leaves the record alone.
#[gpui::test]
fn a_read_in_flight_when_the_window_re_roots_onto_a_root_with_it_off_imports_nothing(
    cx: &mut TestAppContext,
) {
    sweep_re_root(cx, |f, first, imported| {
        let kept = fs::read(f.record_file()).unwrap();
        f.wait_for_window_undo();
        draw(&mut f.cx);
        f.set_window_undo(false);
        draw(&mut f.cx);
        if !imported {
            assert!(!f.has_history(first), "nothing imported");
            assert_eq!(f.text(first), "one two\n");
        }
        assert_eq!(fs::read(f.record_file()).unwrap(), kept, "record as it was");
    });
}

/// A buffer edited during the wait gets no history once the settings are
/// in, and `u` undoes only that edit.
#[gpui::test]
fn a_buffer_edited_while_its_window_waits_gets_no_history(cx: &mut TestAppContext) {
    sweep_re_root(cx, |f, first, imported| {
        if imported {
            return;
        }
        f.wait_for_window_undo();
        draw(&mut f.cx);
        first.update(&mut f.cx, |editor, _| {
            editor
                .model_mut()
                .buffer_mut()
                .edit([(0..0, "x")], Origin::User);
        });
        f.set_window_undo(true);
        draw(&mut f.cx);
        first.update(&mut f.cx, |editor, _| {
            let mut buffer = editor.model_mut().buffer_mut();
            assert!(buffer.undo(user).unwrap().is_some(), "the typed edit");
            assert!(buffer.undo(user).unwrap().is_none(), "no imported history");
        });
        assert_eq!(f.text(first), "one two\n");
    });
}
