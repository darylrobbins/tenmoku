//! A disk check in flight while its buffer's file is renamed: what it found
//! at the old path is no longer about the buffer, so it neither clears the
//! buffer's stamp nor asks to reload; the check starts over at the new path.

use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use fe_project::FsChange;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("open")
}

/// The stats run so far.
fn stats(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| {
        workbench.disk_sync.test.stats.lock().unwrap().0
    })
}

fn checks(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.disk_sync.test.checks)
}

#[gpui::test]
fn a_check_of_the_old_path_landing_after_a_rename_starts_over(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let (old, new) = (root.join("notes.txt"), root.join("todo.txt"));
    fs::write(&old, "hello\n").unwrap();
    let (workbench, cx) = open(cx, &root);
    let editor = open_file(&workbench, &old, cx);
    // Unsaved changes, which a Reload/Keep Mine question would be about.
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert("mine ");
        cx.notify();
    });
    let stamp = editor.read_with(cx, |editor, _| editor.model().buffer().disk_stamp());
    assert!(stamp.is_some());
    let (stats_before, checks_before) = (stats(&workbench, cx), checks(&workbench, cx));

    // A check starts for notes.txt, then, before its stat runs, the file is
    // renamed and the editor follows, as a tree rename does in one turn.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.disk_sync.test.hold_stats = true;
        workbench.check_open_files(window, cx);
        fs::rename(&old, &new).unwrap();
        workbench.follow_rename(&old, &new, cx);
    });
    cx.run_until_parked();
    // The stat found nothing at notes.txt, and waits to land.
    assert_eq!(stats(&workbench, cx), stats_before + 1);
    assert_eq!(checks(&workbench, cx), checks_before + 1);
    let path = editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned));
    assert_eq!(path, Some(new.clone()));

    // Landed: the stamp stays, nothing asks, and the check runs again, at
    // todo.txt, which is the buffer's file as it was.
    workbench.update(cx, |workbench, _| workbench.disk_sync.test.release());
    draw(cx);
    let after = editor.read_with(cx, |editor, _| editor.model().buffer().disk_stamp());
    assert_eq!(after, stamp, "the stamp isn't cleared");
    assert!(!cx.has_pending_prompt());
    assert_eq!(checks(&workbench, cx), checks_before + 2, "checked again");
    assert_eq!(stats(&workbench, cx), stats_before + 2);
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "mine hello\n");

    // The checks follow todo.txt from now on: changed there behind the
    // dirty buffer, the question names it.
    fs::write(&new, "theirs\n").unwrap();
    let file = fs::File::options().write(true).open(&new).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(10))
        .unwrap();
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    panel.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(new.clone())], cx)
    });
    draw(cx);
    let (question, _) = cx.pending_prompt().expect("asked about todo.txt");
    assert_eq!(question, "“todo.txt” changed on disk.");
    cx.simulate_prompt_answer("Keep Mine");
    draw(cx);
}
