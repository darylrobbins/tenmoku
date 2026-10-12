//! Open files kept in step with disk, in a headless workbench: watcher
//! batches handed to the project panel as the watcher would, and the window
//! deactivated and activated again.

use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use fe_buffer::{DiskStamp, ResolvedSelection};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_project::FsChange;
use gpui::{Entity, Focusable as _, Keystroke, PromptLevel, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

/// A workbench for `root`, in a headless window, focused as the app
/// focuses it.
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

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Opens `path` in the active pane, lets it load, and focuses it.
fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    let editor = editors(workbench, path, cx).pop().expect("open");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    editor
}

/// The open editors of `path`.
fn editors(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Vec<Entity<EditorView>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .filter(|editor| editor.read(cx).model().path() == Some(path))
            .collect()
    })
}

/// Hands `changes` to the project panel as a watcher batch.
fn batch(workbench: &Entity<Workbench>, changes: Vec<FsChange>, cx: &mut VisualTestContext) {
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
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

/// Rewrites `path` and hands the watcher's batch naming it over.
fn changed(workbench: &Entity<Workbench>, path: &Path, text: &str, cx: &mut VisualTestContext) {
    rewrite(path, text);
    batch(workbench, vec![FsChange::Path(path.to_owned())], cx);
    draw(cx);
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn dirty(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.model().is_dirty())
}

fn stamp(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<DiskStamp> {
    editor.read_with(cx, |editor, _| editor.model().buffer().disk_stamp())
}

fn caret(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> std::ops::Range<usize> {
    editor.read_with(cx, |editor, _| editor.model().primary_selection().range)
}

/// Types `text` at the caret, as the editor would.
fn type_text(editor: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert(text);
        cx.notify();
    });
}

fn prompt(cx: &mut VisualTestContext) -> Option<String> {
    cx.pending_prompt().map(|(question, _)| question)
}

fn answer(button: &str, cx: &mut VisualTestContext) {
    cx.simulate_prompt_answer(button);
    draw(cx);
}

/// Buffers' checks started, stats run (and those off the main thread),
/// reads run (and those off the main thread).
fn counts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (usize, (usize, usize), (usize, usize)) {
    workbench.read_with(cx, |workbench, _| {
        let test = &workbench.disk_sync.test;
        (
            test.checks,
            *test.stats.lock().unwrap(),
            *test.reads.lock().unwrap(),
        )
    })
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

#[gpui::test]
fn clean_buffer_reloads(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &path, cx);
    editor.update(cx, |editor, cx| {
        let caret = ResolvedSelection {
            range: 5..5,
            reversed: false,
        };
        editor.model_mut().set_selections([caret]);
        cx.notify();
    });
    dispatch("pane.split_right", cx);
    let shown = editors(&workbench, &path, cx);
    assert_eq!(shown.len(), 2, "split");
    let buffer = editor.read_with(cx, |editor, _| editor.model().buffer().id());

    changed(&workbench, &path, "one\ntwo\nTHREE\nfour\n", cx);
    assert_eq!(prompt(cx), None);
    for editor in &shown {
        assert_eq!(text(editor, cx), "one\ntwo\nTHREE\nfour\n");
        assert!(!dirty(editor, cx));
        assert_eq!(caret(editor, cx), 5..5, "on row 1, before the change");
        let id = editor.read_with(cx, |editor, _| editor.model().buffer().id());
        assert_eq!(id, buffer, "the same buffer");
    }
    assert_eq!(stamp(&editor, cx), DiskStamp::of(&path).unwrap());

    // Undo takes the reload back, in one step.
    editor.update(cx, |editor, cx| {
        editor.model_mut().undo().unwrap();
        cx.notify();
    });
    assert_eq!(text(&editor, cx), "one\ntwo\nthree\n");
}

#[gpui::test]
fn dirty_buffer_asks(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &path, cx);

    // Reload: the file's text, clean.
    type_text(&editor, "mine ", cx);
    changed(&workbench, &path, "theirs\n", cx);
    assert_eq!(prompt(cx).as_deref(), Some("“a.txt” changed on disk."));
    assert_eq!(
        text(&editor, cx),
        "mine one\ntwo\n",
        "nothing until answered"
    );
    answer("Reload", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(text(&editor, cx), "theirs\n");
    assert!(!dirty(&editor, cx));

    // Keep Mine, with the file changed again while it asks: asked again
    // once answered.
    type_text(&editor, "mine ", cx);
    let mine = text(&editor, cx);
    changed(&workbench, &path, "theirs again\n", cx);
    assert!(prompt(cx).is_some());
    changed(&workbench, &path, "theirs once more\n", cx);
    answer("Keep Mine", cx);
    assert!(prompt(cx).is_some(), "asked about the later change");
    answer("Keep Mine", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(text(&editor, cx), mine);
    assert!(dirty(&editor, cx));
    assert_eq!(stamp(&editor, cx), DiskStamp::of(&path).unwrap());

    // The next save writes them without asking.
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(prompt(cx), None, "no conflict");
    assert_eq!(fs::read_to_string(&path).unwrap(), mine);
    assert!(!dirty(&editor, cx));
}

#[gpui::test]
fn own_writes_and_touches(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &path, cx);

    // A save whose own batch comes while it runs, typed on meanwhile.
    type_text(&editor, "x", cx);
    let (checks, _, (reads, _)) = counts(&workbench, cx);
    cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse("cmd-s").unwrap(), cx);
    });
    assert!(editor.read_with(cx, |editor, _| editor.is_saving()));
    type_text(&editor, "y", cx);
    batch(&workbench, vec![FsChange::Path(path.clone())], cx);
    batch(&workbench, vec![FsChange::Rescan], cx);
    assert_eq!(counts(&workbench, cx).0, checks, "not while it saves");
    draw(cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(fs::read_to_string(&path).unwrap(), "xone\n");
    assert_eq!(text(&editor, cx), "xyone\n");
    assert!(dirty(&editor, cx));
    let (checks_after, _, (reads_after, _)) = counts(&workbench, cx);
    assert!(checks_after > checks, "checked once the save was over");
    assert_eq!(reads_after, reads, "the file was the save's");

    // The same text written again: only the stamp.
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert!(!dirty(&editor, cx));
    let version = editor.read_with(cx, |editor, _| editor.model().buffer().version());
    changed(&workbench, &path, "xyone\n", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(counts(&workbench, cx).2.0, reads + 1, "read once");
    assert_eq!(stamp(&editor, cx), DiskStamp::of(&path).unwrap());
    let now = editor.read_with(cx, |editor, _| editor.model().buffer().version());
    assert_eq!(now, version, "no edit");
    assert!(!dirty(&editor, cx));

    // Deleted: nothing changes but the stamp, and the next save
    // recreates it.
    fs::remove_file(&path).unwrap();
    batch(&workbench, vec![FsChange::Path(path.clone())], cx);
    draw(cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(text(&editor, cx), "xyone\n");
    assert!(!dirty(&editor, cx));
    assert_eq!(stamp(&editor, cx), None);
    type_text(&editor, "z", cx);
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(fs::read_to_string(&path).unwrap(), "xyzone\n");
    assert!(!dirty(&editor, cx));
}

#[gpui::test]
fn activation_and_rescan(cx: &mut TestAppContext) {
    let (root, outside) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b, out) = (
        root.path().join("a.txt"),
        root.path().join("b.txt"),
        outside.path().join("out.txt"),
    );
    for path in [&a, &b, &out] {
        fs::write(path, "old\n").unwrap();
    }
    let (workbench, cx) = open(cx, root.path());
    open_file(&workbench, &a, cx);
    dispatch("pane.split_right", cx);
    assert_eq!(editors(&workbench, &a, cx).len(), 2, "split");
    open_file(&workbench, &b, cx);
    let out_editor = open_file(&workbench, &out, cx);

    // Outside the root, so no batch names it: the window coming back does.
    rewrite(&out, "new\n");
    cx.deactivate_window();
    draw(cx);
    assert_eq!(text(&out_editor, cx), "old\n");
    cx.update(|window, _| window.activate_window());
    draw(cx);
    assert_eq!(text(&out_editor, cx), "new\n");
    assert!(!dirty(&out_editor, cx));

    // A rescan checks each buffer once, the split one included.
    let (checks, _, _) = counts(&workbench, cx);
    rewrite(&b, "newer\n");
    batch(&workbench, vec![FsChange::Rescan], cx);
    draw(cx);
    assert_eq!(counts(&workbench, cx).0 - checks, 3, "a, b and out.txt");
    assert_eq!(text(&editors(&workbench, &b, cx)[0], cx), "newer\n");

    let (_, stats, reads) = counts(&workbench, cx);
    assert!(stats.0 > 0 && reads.0 >= 2, "{stats:?} {reads:?}");
    assert_eq!(stats.1, stats.0, "stats off the main thread");
    assert_eq!(reads.1, reads.0, "reads off the main thread");
}

/// Two dirty buffers changed at once, with the prompt drawn in the window
/// (Linux's), which a second prompt would replace: they ask in turn. A
/// prompt replaced by another changes nothing, so the next save still asks.
#[gpui::test]
fn prompts_ask_one_at_a_time(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    fs::write(&a, "a\n").unwrap();
    fs::write(&b, "b\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    cx.update(|_, cx| cx.set_prompt_builder(fe_ui::prompt_renderer));
    let prompt_shows = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.has_active_prompt()
        })
    };
    let (a_editor, b_editor) = (open_file(&workbench, &a, cx), open_file(&workbench, &b, cx));
    type_text(&a_editor, "mine ", cx);
    type_text(&b_editor, "mine ", cx);
    rewrite(&a, "theirs a\n");
    rewrite(&b, "theirs b\n");
    batch(&workbench, vec![FsChange::Rescan], cx);
    draw(cx);

    // Enter answers Reload, the first button: one prompt, then the other.
    for reloaded in [1, 2] {
        assert!(prompt_shows(cx));
        cx.simulate_keystrokes("enter");
        draw(cx);
        let clean = [&a_editor, &b_editor].map(|editor| !dirty(editor, cx));
        assert_eq!(clean.iter().filter(|clean| **clean).count(), reloaded);
    }
    assert!(!prompt_shows(cx));
    for (editor, theirs) in [(&a_editor, "theirs a\n"), (&b_editor, "theirs b\n")] {
        assert_eq!(text(editor, cx), theirs);
        assert!(!dirty(editor, cx));
    }

    // Replaced by another prompt: no answer, nothing changes.
    type_text(&a_editor, "mine ", cx);
    let mine = text(&a_editor, cx);
    changed(&workbench, &a, "theirs again\n", cx);
    assert!(prompt_shows(cx));
    let before = stamp(&a_editor, cx);
    let other =
        cx.update(|window, cx| window.prompt(PromptLevel::Info, "Another", None, &["OK"], cx));
    draw(cx);
    assert_eq!(text(&a_editor, cx), mine);
    assert!(dirty(&a_editor, cx));
    assert_eq!(stamp(&a_editor, cx), before, "not kept");
    drop(other);
}

/// A stat or a read that lands after a save finished is stale: the check
/// starts over rather than clear the stamp, or reload the old file.
#[gpui::test]
fn stale_results_start_over(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &path, cx);
    let hold = |stats: bool, cx: &mut VisualTestContext| {
        workbench.update(cx, |workbench, _| match stats {
            true => workbench.disk_sync.test.hold_stats = true,
            false => workbench.disk_sync.test.hold_reads = true,
        })
    };
    let release = |cx: &mut VisualTestContext| {
        workbench.update(cx, |workbench, _| workbench.disk_sync.test.release());
        draw(cx);
    };

    // The file deleted, its stat held, then recreated by a save.
    fs::remove_file(&path).unwrap();
    hold(true, cx);
    batch(&workbench, vec![FsChange::Path(path.clone())], cx);
    draw(cx);
    type_text(&editor, "x", cx);
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert!(!dirty(&editor, cx));
    release(cx);
    assert_eq!(stamp(&editor, cx), DiskStamp::of(&path).unwrap(), "kept");

    // Changed under unsaved edits, its read held, then overwritten by a
    // save.
    type_text(&editor, "mine ", cx);
    let mine = text(&editor, cx);
    hold(false, cx);
    let (_, _, (reads, _)) = counts(&workbench, cx);
    changed(&workbench, &path, "theirs\n", cx);
    assert_eq!(counts(&workbench, cx).2.0, reads + 1, "read, and held");
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(
        prompt(cx).as_deref(),
        Some("“a.txt” changed on disk since it was opened.")
    );
    answer("Overwrite", cx);
    assert!(!dirty(&editor, cx));
    release(cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(text(&editor, cx), mine, "not the old file");
    assert_eq!(fs::read_to_string(&path).unwrap(), mine);
    assert!(!dirty(&editor, cx));
    assert_eq!(stamp(&editor, cx), DiskStamp::of(&path).unwrap());
}

/// A check waiting for a save still runs when the editor that saved
/// closes before it's over, another split still showing the buffer.
#[gpui::test]
fn a_save_in_a_closed_split_still_checks(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let left = open_file(&workbench, &path, cx);
    dispatch("pane.split_right", cx);
    let right = (editors(&workbench, &path, cx).into_iter())
        .find(|editor| *editor != left)
        .unwrap();
    cx.update(|window, cx| window.focus(&right.focus_handle(cx), cx));

    type_text(&right, "x", cx);
    let (checks, _, _) = counts(&workbench, cx);
    cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse("cmd-s").unwrap(), cx);
    });
    batch(&workbench, vec![FsChange::Rescan], cx);
    assert_eq!(counts(&workbench, cx).0, checks, "waits for the save");
    cx.update(|window, cx| {
        let run = RunCommand(
            Invocation::of("workbench.close_tab")
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        window.dispatch_action(Box::new(run), cx);
    });
    drop(right);
    draw(cx);
    assert_eq!(editors(&workbench, &path, cx), std::slice::from_ref(&left));
    cx.executor().advance_clock(super::SAVE_POLL * 2);
    draw(cx);
    assert_eq!(
        counts(&workbench, cx).0,
        checks + 1,
        "checked once it's over"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "xone\n");
    assert_eq!(
        stamp(&left, cx),
        DiskStamp::of(&path).unwrap(),
        "the save's file"
    );
}

/// A batch naming a symlink to an open file, by another name, checks it.
#[cfg(unix)]
#[gpui::test]
fn a_batch_naming_an_alias_checks_its_target(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (path, alias) = (dir.path().join("a.txt"), dir.path().join("alias.txt"));
    fs::write(&path, "one\n").unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &path, cx);

    rewrite(&path, "two\n");
    batch(&workbench, vec![FsChange::Path(alias)], cx);
    draw(cx);
    assert_eq!(text(&editor, cx), "two\n");
}
