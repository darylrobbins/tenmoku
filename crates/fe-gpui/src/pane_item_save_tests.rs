//! Saving an editor off the UI thread: cmd-s and [`PaneItem::save`] write
//! on the background executor, ask before overwriting a file another writer
//! changed, show a failed cmd-s, and buffers read from disk carry the stamp
//! a save checks.

use std::{cell::RefCell, fs, path::Path, rc::Rc};

use fe_buffer::DiskStamp;
use fe_command::Value;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, Keystroke, Task, TestAppContext, VisualTestContext};
use kiln_shell::PaneItem;

use crate::{EditorTheme, EditorView};

/// An editor over `path`, read from disk, focused in a headless window.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    path: &Path,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(crate::init);
    let path = path.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::open(&path).unwrap();
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    (view, cx)
}

/// Presses cmd-s without running the executor, so a save it starts hasn't
/// run yet.
fn cmd_s(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse("cmd-s").unwrap(), cx);
    });
}

fn insert(view: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.model_mut().insert(text);
        cx.notify();
    });
}

fn is_dirty(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    view.update(cx, |view, _| view.model().is_dirty())
}

fn is_saving(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    view.update(cx, |view, _| view.is_saving())
}

/// What `task` resolves to, once the executor has run.
fn outcome(
    task: Task<gpui::Result<()>>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Option<Result<(), String>>>> {
    let result = Rc::new(RefCell::new(None));
    cx.update(|_, cx| {
        let result = result.clone();
        cx.spawn(async move |_| {
            *result.borrow_mut() = Some(task.await.map_err(|error| format!("{error:#}")));
        })
        .detach();
    });
    result
}

#[gpui::test]
fn cmd_s_saves_in_background(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (view, cx) = opened(cx, &path);

    insert(&view, "x", cx);
    cmd_s(cx);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "one\n",
        "not written yet"
    );
    assert!(is_saving(&view, cx));
    assert!(is_dirty(&view, cx));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "xone\n");
    assert!(!is_saving(&view, cx));
    assert!(!is_dirty(&view, cx), "the tab is clean");
    assert!(!cx.has_pending_prompt());

    // Typed while the save runs: written next time.
    insert(&view, "y", cx);
    cmd_s(cx);
    insert(&view, "z", cx);
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "xyone\n");
    assert!(is_dirty(&view, cx), "what was typed after is unsaved");

    // A save asked for while one runs waits for it, then saves the text as
    // it is then.
    cmd_s(cx);
    insert(&view, "w", cx);
    let second = cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    assert!(is_saving(&view, cx));
    let second = outcome(second, cx);
    cx.run_until_parked();
    second.take().expect("done").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "xyzwone\n");
    assert!(!is_dirty(&view, cx));
    assert!(!is_saving(&view, cx));
    assert!(
        !cx.has_pending_prompt(),
        "its own save isn't a change on disk"
    );
}

#[gpui::test]
fn changed_on_disk_asks(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (view, cx) = opened(cx, &path);

    // Overwrite writes the buffer.
    insert(&view, "mine ", cx);
    fs::write(&path, "theirs\n").unwrap();
    cmd_s(cx);
    cx.run_until_parked();
    let (question, detail) = cx.pending_prompt().expect("asks");
    assert_eq!(question, "“a.txt” changed on disk since it was opened.");
    assert_eq!(detail, "Overwrite it with your version?");
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
    cx.simulate_prompt_answer("Overwrite");
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "mine one\n");
    assert!(!is_dirty(&view, cx));
    assert!(!cx.has_pending_prompt());

    // Overwrite writes the text as it was at cmd-s: typed after it, before
    // the executor ran or while asked, stays unsaved.
    insert(&view, "kept ", cx);
    fs::write(&path, "theirs\n").unwrap();
    cmd_s(cx);
    insert(&view, "early ", cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    insert(&view, "late ", cx);
    cx.simulate_prompt_answer("Overwrite");
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "mine kept one\n");
    assert!(is_dirty(&view, cx), "what was typed after cmd-s is unsaved");
    assert!(!is_saving(&view, cx));

    // Cancel leaves the other writer's file and the buffer dirty, and the
    // save fails.
    insert(&view, "again ", cx);
    fs::write(&path, "theirs again\n").unwrap();
    let saved = cx
        .update(|window, cx| view.update(cx, |view, cx| PaneItem::save(view, window, cx)))
        .expect("an editor saves");
    let saved = outcome(saved, cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs again\n");
    assert!(is_dirty(&view, cx));
    assert!(!is_saving(&view, cx));
    let error = saved.take().expect("done").unwrap_err();
    assert_eq!(error, "not saved: a.txt changed on disk");
    assert!(!cx.has_pending_prompt(), "a cancelled save shows no error");
}

#[cfg(unix)]
#[gpui::test]
fn failure_is_shown(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked");
    fs::create_dir(&locked).unwrap();
    let path = locked.join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (view, cx) = opened(cx, &path);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();

    insert(&view, "x", cx);
    cmd_s(cx);
    cx.run_until_parked();
    let shown = cx.pending_prompt();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let (question, detail) = shown.expect("the error is shown");
    assert!(question.contains("a.txt"), "{question}");
    assert!(detail.contains("ermission denied"), "{detail}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "one\n");
    assert!(is_dirty(&view, cx));
    assert!(!is_saving(&view, cx));
}

#[gpui::test]
fn reads_record_stamps(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let stamp = DiskStamp::of(&path).unwrap();
    assert!(stamp.is_some());

    // From fe-workbench's loader, as the launch file and Pane::open read.
    let buffer = fe_workbench::loader::read(path.clone())
        .unwrap()
        .expect("the file exists");
    assert!(buffer.disk_stamp_known());
    assert_eq!(buffer.disk_stamp(), stamp);

    // A restored tab.
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(fe_buffer::Buffer::new(""), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    let state = Value::from_iter([
        ("path", Value::from(path.to_str().unwrap())),
        ("caret", Value::from(4)),
    ]);
    let restored: Task<Option<Entity<EditorView>>> =
        cx.update(|window, cx| EditorView::restore(state, window, cx));
    let view = Rc::new(RefCell::new(None));
    cx.update(|_, cx| {
        let view = view.clone();
        cx.spawn(async move |_| *view.borrow_mut() = restored.await)
            .detach();
    });
    cx.run_until_parked();
    let view = view.take().expect("restored");
    view.update(cx, |view, _| {
        let buffer = view.model().buffer();
        assert!(buffer.disk_stamp_known());
        assert_eq!(buffer.disk_stamp(), stamp);
    });

    // So a save refuses a file changed since.
    fs::write(&path, "theirs\n").unwrap();
    insert(&view, "mine ", cx);
    let saved = cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    let saved = outcome(saved, cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
    assert!(saved.take().expect("done").is_err());
}
