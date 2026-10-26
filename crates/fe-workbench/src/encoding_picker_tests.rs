//! Reopen Editor with Encoding… and Save with Encoding…, from their
//! commands and from the status bar's encoding item.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_buffer::{ResolvedSelection, Text};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{EncodingAction, choice, map_offset};
use crate::{Launch, StatusItem, StatusSide, Workbench, workbench::toast::Level};

/// `files` (name, bytes) in a temporary directory, and their paths.
fn project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = files
        .iter()
        .map(|(name, bytes)| {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench rooted at `dir` showing `path`, focused.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
    path: &Path,
) -> (
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown");
    (workbench, editor, cx)
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

/// What an encoding picker shows: what its encodings are for (`None` for
/// the actions) and its rows' labels, each with whether it's marked current.
type Shown = (Option<EncodingAction>, Vec<(&'static str, bool)>);

/// What the encoding picker showing shows.
fn picker(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Shown> {
    let picker = workbench.read_with(cx, |workbench, cx| workbench.encoding_picker(cx))?;
    Some(picker.read_with(cx, |picker, _| {
        (picker.delegate.action(), picker.delegate.labels())
    }))
}

/// Types `query` into the picker showing and picks the first match.
fn pick(query: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(query);
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// What the status bar's encoding item says.
fn label(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Right))
        .into_iter()
        .find(|(id, _)| id == StatusItem::ENCODING)
        .map(|(_, text)| text.to_string())
        .expect("the encoding item is drawn")
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn selections(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<ResolvedSelection> {
    editor.read_with(cx, |editor, _| editor.model().selections())
}

/// Each toast shown: its level, message and detail.
fn toasts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Level, String, Option<String>)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                (toast.level, toast.message.clone(), toast.detail.clone())
            })
            .collect()
    })
}

/// How many times a toast saying `message` has shown, repeats counted.
fn shown(workbench: &Entity<Workbench>, message: &str, cx: &mut VisualTestContext) -> usize {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .filter(|entry| entry.toast.message == message)
            .map(|entry| entry.count)
            .sum()
    })
}

fn editor_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn reopen_with_encoding(cx: &mut TestAppContext) {
    let bytes = b"caf\xE9 na\xEFve\nsecond line\n";
    let (dir, paths) = project(&[("latin.txt", bytes)]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let correct = "café naïve\nsecond line\n";
    assert_eq!(text(&editor, cx), correct);
    assert_eq!(label(&workbench, cx), "windows-1252");

    // Mis-read as Shift_JIS, as a wrong guess would have it.
    dispatch("workbench.reopen_with_encoding", cx);
    let (action, labels) = picker(&workbench, cx).expect("the encodings show");
    assert_eq!(action, Some(EncodingAction::Reopen));
    let current: Vec<_> = labels.iter().filter(|(_, current)| *current).collect();
    assert_eq!(current, [&("Western (Windows 1252)", true)]);
    pick("shift", cx);
    assert!(picker(&workbench, cx).is_none());
    assert!(editor_focused(&editor, cx));
    let mojibake = text(&editor, cx);
    assert!(!mojibake.contains('é'), "{mojibake:?}");
    assert!(mojibake.ends_with("\nsecond line\n"), "{mojibake:?}");
    assert_eq!(label(&workbench, cx), "Shift_JIS");

    // Selections over unchanged text before, between and after the two
    // runs that decode differently on the first row, a caret just past the
    // second run, and one on the next row.
    let line = &mojibake[..mojibake.find('\n').unwrap()];
    let na = line.find(" na").unwrap();
    let e = line.rfind('e').unwrap();
    let second = mojibake.find("second").unwrap();
    let selection = |range, reversed| ResolvedSelection { range, reversed };
    editor.update(cx, |editor, _| {
        editor.model_mut().set_selections([
            selection(0..3, false),
            selection(na..na + 3, true),
            selection(e..e, false),
            selection(second..second + "second".len(), true),
        ])
    });

    dispatch("workbench.reopen_with_encoding", cx);
    let (_, labels) = picker(&workbench, cx).expect("the encodings show");
    let current: Vec<_> = labels.iter().filter(|(_, current)| *current).collect();
    assert_eq!(current, [&("Japanese (Shift JIS)", true)]);
    pick("windows 1252", cx);
    assert_eq!(text(&editor, cx), correct);
    let na = correct.find(" na").unwrap();
    let e = correct.find("e\n").unwrap();
    let second = correct.find("second").unwrap();
    assert_eq!(
        selections(&editor, cx),
        [
            selection(0..3, false),
            selection(na..na + 3, true),
            selection(e..e, false),
            selection(second..second + "second".len(), true),
        ]
    );
    assert_eq!(label(&workbench, cx), "windows-1252");
    let clean = editor.read_with(cx, |editor, _| !editor.model().is_dirty());
    assert!(clean);
    assert!(toasts(&workbench, cx).is_empty());
    // Nothing was written.
    assert_eq!(fs::read(&paths[0]).unwrap(), bytes);
}

#[gpui::test]
fn reopen_refuses_dirty(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("notes.txt", "café\n".as_bytes())]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    editor.update(cx, |editor, _| editor.model_mut().insert("mine "));
    let before = text(&editor, cx);

    dispatch("workbench.reopen_with_encoding", cx);
    pick("windows 1252", cx);
    assert_eq!(text(&editor, cx), before);
    assert_eq!(label(&workbench, cx), "UTF-8");
    let dirty = editor.read_with(cx, |editor, _| editor.model().is_dirty());
    assert!(dirty);
    assert_eq!(
        toasts(&workbench, cx),
        [(
            Level::Warning,
            "“notes.txt” has unsaved changes".to_owned(),
            Some("Save or undo them before reopening it with windows-1252.".to_owned())
        )]
    );
}

#[gpui::test]
fn save_with_encoding(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "café".as_bytes()), ("b.txt", "a€".as_bytes())]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    assert_eq!(label(&workbench, cx), "UTF-8");

    dispatch("workbench.save_with_encoding", cx);
    let (action, labels) = picker(&workbench, cx).expect("the encodings show");
    assert_eq!(action, Some(EncodingAction::Save));
    assert_eq!(labels[0], ("UTF-8", true));
    pick("windows 1252", cx);
    assert_eq!(fs::read(&paths[0]).unwrap(), b"caf\xE9");
    assert_eq!(label(&workbench, cx), "windows-1252");
    let clean = editor.read_with(cx, |editor, _| !editor.model().is_dirty());
    assert!(clean);
    assert!(toasts(&workbench, cx).is_empty());

    // ISO-8859-2 has no '€'.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = paths[1].clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    dispatch("workbench.save_with_encoding", cx);
    pick("8859-2", cx);
    assert_eq!(fs::read(&paths[1]).unwrap(), "a€".as_bytes());
    assert_eq!(label(&workbench, cx), "UTF-8");
    let clean = editor.read_with(cx, |editor, _| !editor.model().is_dirty());
    assert!(clean);
    assert_eq!(
        toasts(&workbench, cx),
        [(
            Level::Error,
            "“b.txt” wasn't saved in ISO-8859-2".to_owned(),
            Some("'€' (U+20AC) on line 1, column 2 can't be saved in ISO-8859-2".to_owned())
        )]
    );

    // A save that fails once the encoding is set puts it back: here the
    // folder has gone read-only, so the file can't be replaced.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |mode| fs::Permissions::from_mode(mode);
        fs::set_permissions(dir.path(), mode(0o555)).unwrap();
        dispatch("workbench.save_with_encoding", cx);
        pick("utf-16 le", cx);
        fs::set_permissions(dir.path(), mode(0o755)).unwrap();
        assert_eq!(fs::read(&paths[1]).unwrap(), "a€".as_bytes());
        assert_eq!(label(&workbench, cx), "UTF-8");
        let clean = editor.read_with(cx, |editor, _| !editor.model().is_dirty());
        assert!(clean);
        let shown = toasts(&workbench, cx);
        assert_eq!(shown.len(), 2, "{shown:?}");
        assert_eq!(shown[1].1, "“b.txt” wasn't saved in UTF-16LE");
    }
}

#[gpui::test]
fn the_status_bar_item_lists_the_actions_then_the_encodings(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "plain\n".as_bytes())]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);

    let item = cx
        .debug_bounds("status-item:encoding")
        .expect("the encoding item is drawn");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    assert_eq!(
        picker(&workbench, cx),
        Some((
            None,
            vec![
                ("Reopen with Encoding", false),
                ("Save with Encoding", false)
            ]
        ))
    );

    pick("save", cx);
    let (action, labels) = picker(&workbench, cx).expect("the encodings show");
    assert_eq!(action, Some(EncodingAction::Save));
    let all: Vec<_> = super::choices().iter().map(|choice| choice.label).collect();
    assert_eq!(
        labels.iter().map(|(label, _)| *label).collect::<Vec<_>>(),
        all
    );
    let current: Vec<_> = labels.iter().filter(|(_, current)| *current).collect();
    assert_eq!(current, [&("UTF-8", true)]);

    // Dismissed, nothing changes and the editor has focus back.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(picker(&workbench, cx).is_none());
    assert!(editor_focused(&editor, cx));
    assert_eq!(label(&workbench, cx), "UTF-8");

    // The actions' Reopen lists the encodings to reopen with.
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    pick("reopen", cx);
    let (action, _) = picker(&workbench, cx).expect("the encodings show");
    assert_eq!(action, Some(EncodingAction::Reopen));
}

#[test]
fn the_encodings_offered_are_encoding_rs_s() {
    let names: Vec<_> = super::choices()
        .iter()
        .map(|choice| (choice.label, choice.encoding.name(), choice.encoding.bom))
        .collect();
    assert_eq!(names.len(), 21);
    for expected in [
        ("UTF-8", "UTF-8", false),
        ("UTF-8 with BOM", "UTF-8", true),
        ("UTF-16 LE", "UTF-16LE", true),
        ("UTF-16 BE", "UTF-16BE", true),
        ("Western (Windows 1252)", "windows-1252", false),
        ("Central European (ISO 8859-2)", "ISO-8859-2", false),
        ("Western (ISO 8859-15)", "ISO-8859-15", false),
        ("Cyrillic (KOI8-R)", "KOI8-R", false),
        ("Japanese (Shift JIS)", "Shift_JIS", false),
        ("Simplified Chinese (GB18030)", "gb18030", false),
        ("Korean (EUC-KR)", "EUC-KR", false),
    ] {
        assert!(names.contains(&expected), "{expected:?} in {names:?}");
    }
    assert_eq!(
        choice("UTF-8 with BOM").unwrap().encoding.label(),
        "UTF-8 with BOM"
    );
    // The WHATWG ISO-8859-1 is windows-1252, offered once.
    let windows_1252 = names.iter().filter(|(_, name, _)| *name == "windows-1252");
    assert_eq!(windows_1252.count(), 1);
}

/// Runs `run` on the workbench and `editor` in one update: what it starts
/// in the background has yet to run when it returns.
fn at_once(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    run: impl FnOnce(
        &mut Workbench,
        &Entity<EditorView>,
        &mut gpui::Window,
        &mut gpui::Context<Workbench>,
    ),
) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| run(workbench, editor, window, cx))
    });
}

fn windows_1252() -> super::EncodingChoice {
    choice("Western (Windows 1252)").unwrap()
}

#[gpui::test]
fn a_read_is_dropped_if_the_buffer_changed_meanwhile(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", b"caf\xE9\n"), ("b.txt", b"caf\xE9\n")]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    assert_eq!(label(&workbench, cx), "windows-1252");
    let utf8 = choice("UTF-8").unwrap();

    // Edited while the file is read.
    at_once(&workbench, &editor, cx, |workbench, editor, _, cx| {
        workbench.reopen_with_encoding(editor, utf8, cx);
        editor.update(cx, |editor, _| editor.model_mut().insert("x"));
    });
    draw(cx);
    assert_eq!(text(&editor, cx), "xcafé\n");
    assert_eq!(label(&workbench, cx), "windows-1252");
    let changed = "“a.txt” changed while it was read";
    assert_eq!(shown(&workbench, changed, cx), 1);
    editor.update(cx, |editor, _| editor.model_mut().undo().unwrap());
    let clean = editor.read_with(cx, |editor, _| !editor.model().is_dirty());
    assert!(clean);

    // Saved elsewhere while the file is read: the read was of the old file.
    let other = dir.path().join("c.txt");
    at_once(&workbench, &editor, cx, |workbench, editor, _, cx| {
        workbench.reopen_with_encoding(editor, utf8, cx);
        editor.update(cx, |editor, _| {
            editor.model_mut().set_path(Some(other.clone()))
        });
    });
    draw(cx);
    assert_eq!(text(&editor, cx), "café\n");
    assert_eq!(label(&workbench, cx), "windows-1252");
    assert_eq!(shown(&workbench, changed, cx), 2);
}

#[gpui::test]
fn one_encoding_operation_runs_per_buffer(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "café\n".as_bytes())]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let (utf16, shift_jis) = (
        choice("UTF-16 LE").unwrap(),
        choice("Japanese (Shift JIS)").unwrap(),
    );

    // A save in windows-1252 runs; a reopen and another save asked for
    // meanwhile are refused, and its save writes in its own encoding.
    at_once(&workbench, &editor, cx, |workbench, editor, window, cx| {
        workbench.save_with_encoding(editor, windows_1252(), window, cx);
        workbench.reopen_with_encoding(editor, shift_jis, cx);
        workbench.save_with_encoding(editor, utf16, window, cx);
    });
    draw(cx);
    assert_eq!(fs::read(&paths[0]).unwrap(), b"caf\xE9\n");
    assert_eq!(label(&workbench, cx), "windows-1252");
    assert_eq!(text(&editor, cx), "café\n");
    let running = "“a.txt” is already changing its encoding";
    assert_eq!(shown(&workbench, running, cx), 2);
    assert_eq!(toasts(&workbench, cx).len(), 1);

    // Once it's done, the next one runs.
    at_once(&workbench, &editor, cx, |workbench, editor, window, cx| {
        workbench.save_with_encoding(editor, utf16, window, cx);
    });
    draw(cx);
    assert_eq!(label(&workbench, cx), "UTF-16LE");
    assert_eq!(&fs::read(&paths[0]).unwrap()[..2], b"\xFF\xFE");

    // None starts while the buffer saves.
    at_once(&workbench, &editor, cx, |workbench, editor, window, cx| {
        let saving = editor.update(cx, |editor, cx| editor.save(window, cx));
        saving.detach();
        workbench.save_with_encoding(editor, windows_1252(), window, cx);
        workbench.reopen_with_encoding(editor, shift_jis, cx);
    });
    draw(cx);
    assert_eq!(label(&workbench, cx), "UTF-16LE");
    assert_eq!(shown(&workbench, "“a.txt” is being saved", cx), 2);
    assert_eq!(shown(&workbench, running, cx), 2);
}

#[gpui::test]
fn an_untitled_buffer_takes_the_encoding_for_its_first_save(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "plain\n".as_bytes())]);
    let (workbench, _, cx) = opened(cx, &dir, &paths[0]);
    dispatch("workbench.new_file", cx);
    let untitled = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    untitled.update(cx, |editor, _| editor.model_mut().insert("café"));

    dispatch("workbench.save_with_encoding", cx);
    pick("windows 1252", cx);
    assert_eq!(label(&workbench, cx), "windows-1252");
    let path = untitled.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned));
    assert_eq!(path, None);
    assert!(toasts(&workbench, cx).is_empty());
    // No save panel was asked for.
    let mut out = Vec::new();
    untitled.read_with(cx, |editor, _| {
        editor.model().buffer().write_to(&mut out).unwrap()
    });
    assert_eq!(out, b"caf\xE9");
}

#[test]
fn offsets_map_through_the_runs_that_decode_differently() {
    let old = Text::new("caf\u{FFFD} na\u{6F7E}e\nsecond\n");
    let new = Text::new("café naïve\nsecond\n");
    let at = |s: &str, needle: &str| s.find(needle).unwrap();
    let (old_s, new_s) = (old.to_string(), new.to_string());
    // Before, between and after the two runs.
    assert_eq!(map_offset(&old, &new, 2), 2);
    assert_eq!(map_offset(&old, &new, at(&old_s, " na")), at(&new_s, " na"));
    assert_eq!(
        map_offset(&old, &new, at(&old_s, "a\u{6F7E}")),
        at(&new_s, "aï")
    );
    assert_eq!(map_offset(&old, &new, at(&old_s, "e\n")), at(&new_s, "e\n"));
    // A run's edges go to the new run's edges.
    assert_eq!(map_offset(&old, &new, 3), 3);
    assert_eq!(
        map_offset(&old, &new, at(&old_s, "\u{6F7E}")),
        at(&new_s, "ïv")
    );
    // Later rows by row and column.
    assert_eq!(
        map_offset(&old, &new, at(&old_s, "cond")),
        at(&new_s, "cond")
    );
    assert_eq!(map_offset(&old, &new, old.len()), new.len());

    // A longer run: a column inside it goes as far in, at most to its end.
    let old = Text::new("a\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}b");
    let new = Text::new("aéb");
    assert_eq!(map_offset(&old, &new, "a\u{FFFD}".len()), "aé".len());
    assert_eq!(
        map_offset(&old, &new, "a\u{FFFD}\u{FFFD}\u{FFFD}".len()),
        "aé".len()
    );
    assert_eq!(map_offset(&old, &new, old.len()), new.len());

    // A row the new text lacks clips to its end.
    let old = Text::new("a\nb\nc");
    let new = Text::new("a\u{0}");
    assert_eq!(map_offset(&old, &new, 4), new.len());
}

#[gpui::test]
fn saves_queued_behind_a_failed_conversion_keep_the_encoding(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "a€\n".as_bytes())]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let latin2 = choice("Central European (ISO 8859-2)").unwrap();
    let copy = dir.path().join("copy.txt");

    // The conversion fails on '€' as it writes; a Save and a Save As asked
    // for while it runs wait for it, then write in UTF-8.
    let saved = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.model_mut().insert("b");
            let saved = editor.save_in_encoding(latin2.encoding, window, cx);
            assert_eq!(
                editor.model().buffer().encoding(),
                fe_buffer::FileEncoding::UTF8
            );
            editor.save(window, cx).detach();
            editor.save_as(copy.clone(), window, cx).detach();
            saved
        })
    });
    let failed = std::rc::Rc::new(std::cell::Cell::new(None));
    let set = failed.clone();
    cx.update(|_, cx| {
        cx.spawn(async move |_| set.set(Some(saved.await.is_err())))
            .detach()
    });
    draw(cx);
    assert_eq!(failed.get(), Some(true));
    assert_eq!(fs::read(&paths[0]).unwrap(), "ba€\n".as_bytes());
    assert_eq!(fs::read(&copy).unwrap(), "ba€\n".as_bytes());
    assert_eq!(label(&workbench, cx), "UTF-8");
    let clean = editor.read_with(cx, |editor, _| !editor.model().is_dirty());
    assert!(clean);
}

#[gpui::test]
fn an_untitled_buffer_edited_while_checked_keeps_its_encoding(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "plain\n".as_bytes())]);
    let (workbench, _, cx) = opened(cx, &dir, &paths[0]);
    dispatch("workbench.new_file", cx);
    let untitled = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    untitled.update(cx, |editor, _| editor.model_mut().insert("café"));

    at_once(
        &workbench,
        &untitled,
        cx,
        |workbench, editor, window, cx| {
            workbench.save_with_encoding(editor, windows_1252(), window, cx);
            editor.update(cx, |editor, _| editor.model_mut().insert("😀"));
        },
    );
    draw(cx);
    assert_eq!(label(&workbench, cx), "UTF-8");
    let changed = "“untitled” changed while it was checked";
    assert_eq!(shown(&workbench, changed, cx), 1);
}

#[test]
fn offsets_on_long_rows_keep_the_text_around_the_differences() {
    let long = "x".repeat(20_000);
    let old = Text::new(&format!("{long}caf\u{FFFD} na\u{6F7E}e\n"));
    let new = Text::new(&format!("{long}café naïve\n"));
    let (old_s, new_s) = (old.to_string(), new.to_string());
    let at = |s: &str, needle: &str| s.find(needle).unwrap();
    assert_eq!(map_offset(&old, &new, 5), 5);
    assert_eq!(map_offset(&old, &new, at(&old_s, " na")), at(&new_s, " na"));
    assert_eq!(
        map_offset(&old, &new, at(&old_s, "a\u{6F7E}")),
        at(&new_s, "aï")
    );
    assert_eq!(map_offset(&old, &new, at(&old_s, "e\n")), at(&new_s, "e\n"));

    // Differences far apart, with shared text between them longer than a
    // diff's window: the text between keeps its place.
    let old = Text::new(&format!("caf\u{FFFD} na\u{6F7E}e{long}\u{FFFD}\n"));
    let new = Text::new(&format!("café naïve{long}é\n"));
    let (old_s, new_s) = (old.to_string(), new.to_string());
    assert_eq!(map_offset(&old, &new, at(&old_s, "ex")), at(&new_s, "ex"));
    assert_eq!(map_offset(&old, &new, at(&old_s, " na")), at(&new_s, " na"));
    let middle = at(&old_s, "x") + 10_000;
    assert_eq!(map_offset(&old, &new, middle), at(&new_s, "x") + 10_000);
    assert_eq!(
        map_offset(&old, &new, at(&old_s, "\u{FFFD}\n")),
        at(&new_s, "é\n")
    );
    assert_eq!(map_offset(&old, &new, old.len()), new.len());

    // Differences all along a row longer than a window: every shared char
    // keeps its place.
    let old: String = (0..20_000)
        .map(|i| if i % 7 == 0 { "\u{FFFD}" } else { "a" })
        .collect();
    let new: String = (0..20_000)
        .map(|i| if i % 7 == 0 { "é" } else { "a" })
        .collect();
    let (old, new) = (Text::new(&old), Text::new(&new));
    for column in [1, 3_001, 12_345, 19_999] {
        let offset = old.to_string().char_indices().nth(column).unwrap().0;
        let mapped = new.to_string().char_indices().nth(column).unwrap().0;
        assert_eq!(map_offset(&old, &new, offset), mapped, "column {column}");
    }
}

#[test]
fn a_caret_before_inserted_chars_stays_with_the_text_after_them() {
    // UTF-16LE "ab" read as UTF-8, then as UTF-16LE: a NUL after each.
    let old = Text::new("a\u{0}b\u{0}");
    let new = Text::new("ab");
    assert_eq!(map_offset(&old, &new, 2), 1);
    // And back: the caret before 'b' goes past the NUL inserted before it.
    let (old, new) = (new, old);
    assert_eq!(map_offset(&old, &new, 1), 2);
    assert_eq!(map_offset(&old, &new, 0), 0);
    assert_eq!(map_offset(&old, &new, 2), 4);
}

#[gpui::test]
fn an_untitled_buffer_saving_as_while_checked_keeps_its_encoding(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "plain\n".as_bytes())]);
    let (workbench, _, cx) = opened(cx, &dir, &paths[0]);
    dispatch("workbench.new_file", cx);
    let untitled = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    untitled.update(cx, |editor, _| editor.model_mut().insert("café"));
    let path = dir.path().join("saved.txt");

    // Save As starts while windows-1252 is checked: it writes UTF-8, and the
    // check finding it running sets nothing.
    at_once(
        &workbench,
        &untitled,
        cx,
        |workbench, editor, window, cx| {
            workbench.save_with_encoding(editor, windows_1252(), window, cx);
            let saving = editor.update(cx, |editor, cx| editor.save_as(path.clone(), window, cx));
            saving.detach();
        },
    );
    draw(cx);
    assert_eq!(fs::read(&path).unwrap(), "café".as_bytes());
    assert_eq!(label(&workbench, cx), "UTF-8");
    let clean = untitled.read_with(cx, |editor, _| !editor.model().is_dirty());
    assert!(clean);
}

#[gpui::test]
fn saves_queued_behind_a_conversion_write_in_its_encoding(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "café\n".as_bytes())]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let copy = dir.path().join("copy.txt");

    // A Save and a Save As asked for while the conversion runs wait for it,
    // then write in windows-1252, the buffer's by then.
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor
                .save_in_encoding(windows_1252().encoding, window, cx)
                .detach();
            assert_eq!(
                editor.model().buffer().encoding(),
                fe_buffer::FileEncoding::UTF8
            );
            editor.model_mut().insert("b");
            editor.save(window, cx).detach();
            editor.save_as(copy.clone(), window, cx).detach();
        })
    });
    draw(cx);
    assert_eq!(fs::read(&paths[0]).unwrap(), b"bcaf\xE9\n");
    assert_eq!(fs::read(&copy).unwrap(), b"bcaf\xE9\n");
    assert_eq!(label(&workbench, cx), "windows-1252");
    let clean = editor.read_with(cx, |editor, _| !editor.model().is_dirty());
    assert!(clean);
}

#[test]
fn offsets_on_long_varied_rows_keep_their_shifts() {
    // Shared text longer than a diff's window, of varied chars, between and
    // after differences that change the row's length.
    let digits = "0123456789".repeat(2000);
    let old = Text::new(&format!("caf\u{FFFD} na\u{6F7E}e{digits}\u{FFFD} end"));
    let new = Text::new(&format!("café naïve{digits}é end"));
    let (old_s, new_s) = (old.to_string(), new.to_string());
    let at = |s: &str, needle: &str| s.find(needle).unwrap();
    assert_eq!(map_offset(&old, &new, at(&old_s, "e0")), at(&new_s, "e0"));
    for ahead in [1, 2_047, 2_048, 2_049, 4_095, 4_096, 9_999, 19_999] {
        let (from, to) = (at(&old_s, "e0") + 1 + ahead, at(&new_s, "e0") + 1 + ahead);
        assert_eq!(map_offset(&old, &new, from), to, "{ahead} digits in");
    }
    assert_eq!(
        map_offset(&old, &new, at(&old_s, " end")),
        at(&new_s, " end")
    );
    assert_eq!(map_offset(&old, &new, at(&old_s, "nd")), at(&new_s, "nd"));

    // Differences every few chars along a varied row longer than a window.
    let words: Vec<String> = (0..3000).map(|i| format!("w{i}")).collect();
    let old: String = words.iter().map(|w| format!("{w}\u{FFFD}")).collect();
    let new: String = words.iter().map(|w| format!("{w}éé")).collect();
    let (old, new) = (Text::new(&old), Text::new(&new));
    let (old_s, new_s) = (old.to_string(), new.to_string());
    for i in [1, 700, 1500, 2999] {
        let word = format!("w{i}\u{FFFD}");
        let (from, to) = (at(&old_s, &word), at(&new_s, &format!("w{i}éé")));
        assert_eq!(map_offset(&old, &new, from), to, "word {i}");
        let (from, to) = (from + word.len(), to + format!("w{i}éé").len());
        assert_eq!(map_offset(&old, &new, from), to, "after word {i}");
    }
}

#[gpui::test]
fn a_conversion_replaced_on_disk_before_its_recorded_keeps_the_encoding(cx: &mut TestAppContext) {
    let (dir, paths) = project(&[("a.txt", "café\n".as_bytes())]);
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);

    // Step until the conversion's write lands, then replace the file before
    // the save is recorded.
    let saved = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.save_in_encoding(windows_1252().encoding, window, cx)
        })
    });
    let failed = std::rc::Rc::new(std::cell::Cell::new(None));
    let set = failed.clone();
    cx.update(|_, cx| {
        cx.spawn(async move |_| set.set(Some(saved.await.map_err(|error| format!("{error:#}")))))
            .detach()
    });
    let mut steps = 0;
    while fs::read(&paths[0]).unwrap() != b"caf\xE9\n" {
        assert!(cx.executor().tick(), "the write never landed");
        steps += 1;
        assert!(steps < 10_000);
    }
    // A new inode, as another writer's rename gives it.
    let theirs = dir.path().join("theirs.tmp");
    fs::write(&theirs, "theirs\n").unwrap();
    fs::rename(&theirs, &paths[0]).unwrap();
    draw(cx);
    let error = failed.take().expect("the save finished").unwrap_err();
    assert!(
        error.contains("changed on disk after it was written in windows-1252"),
        "{error}"
    );
    assert_eq!(label(&workbench, cx), "UTF-8");
}

#[gpui::test]
fn a_vim_visual_selection_keeps_its_place(cx: &mut TestAppContext) {
    let bytes = b"caf\xE9 na\xEFve\n";
    let (dir, paths) = project(&[("latin.txt", bytes)]);
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        fe_gpui::set_vim_enabled(cx, true);
    });
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let shift_jis = choice("Japanese (Shift JIS)").unwrap();
    at_once(&workbench, &editor, cx, |workbench, editor, _, cx| {
        workbench.reopen_with_encoding(editor, shift_jis, cx)
    });
    draw(cx);
    let mojibake = text(&editor, cx);
    let na = mojibake.find(" na").unwrap();

    // `v` over " na", between the two runs that decode differently, the
    // cursor at its start.
    editor.update(cx, |editor, cx| {
        editor.update_vim(
            |vim, model| vim.select_visual(model, fe_vim::VisualKind::Char, na + 2, na),
            cx,
        );
    });
    draw(cx);
    at_once(&workbench, &editor, cx, |workbench, editor, _, cx| {
        workbench.reopen_with_encoding(editor, windows_1252(), cx)
    });
    draw(cx);
    let correct = text(&editor, cx);
    assert_eq!(correct, "café naïve\n");
    let na = correct.find(" na").unwrap();
    let (mode, anchor, cursor) = editor.read_with(cx, |editor, _| {
        let vim = editor.vim_state().unwrap();
        let model = editor.model();
        (
            vim.mode(),
            vim.visual_anchor(model),
            vim.visual_cursor(model),
        )
    });
    assert_eq!(mode, fe_vim::Mode::Visual(fe_vim::VisualKind::Char));
    assert_eq!((anchor, cursor), (Some(na + 2), Some(na)));
    assert_eq!(
        selections(&editor, cx),
        [ResolvedSelection {
            range: na..na + 3,
            reversed: true,
        }]
    );
}

/// A workbench with vim on showing `bytes`, reopened as Shift JIS and drawn,
/// its editor focused.
fn vim_reopened_as_shift_jis<'a>(
    cx: &'a mut TestAppContext,
    bytes: &[u8],
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let (dir, paths) = project(&[("latin.txt", bytes)]);
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        fe_gpui::set_vim_enabled(cx, true);
    });
    let (workbench, editor, cx) = opened(cx, &dir, &paths[0]);
    let shift_jis = choice("Japanese (Shift JIS)").unwrap();
    at_once(&workbench, &editor, cx, |workbench, editor, _, cx| {
        workbench.reopen_with_encoding(editor, shift_jis, cx)
    });
    draw(cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    (dir, workbench, editor, cx)
}

fn reopen_as_windows_1252(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) {
    at_once(workbench, editor, cx, |workbench, editor, _, cx| {
        workbench.reopen_with_encoding(editor, windows_1252(), cx)
    });
    draw(cx);
}

#[gpui::test]
fn reopen_keeps_search_suspended_visual(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) =
        vim_reopened_as_shift_jis(cx, b"\x82\xA0\x82\xA2\x82\xA4\nabcdef\n");
    cx.simulate_keystrokes("v l /");
    draw(cx);
    let held = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            let vim = editor.vim_state().unwrap();
            let model = editor.model();
            (
                matches!(vim.mode(), fe_vim::Mode::CmdLine(_)),
                vim.visual_anchor(model),
                vim.visual_cursor(model),
                model.selections(),
            )
        })
    };
    let before = held(cx);
    assert!(before.0, "the search command line is open");
    assert_eq!(before.3.len(), 1);

    reopen_as_windows_1252(&workbench, &editor, cx);
    assert_eq!(
        text(&editor, cx),
        "\u{201a}\u{a0}\u{201a}\u{a2}\u{201a}\u{a4}\nabcdef\n"
    );
    let after = held(cx);
    assert!(after.0, "the command line is still open");
    // The ends sit in text the reopen rewrote: they stay where they were.
    assert_eq!(after.1, before.1);
    assert_eq!(after.2, before.2);
    assert_eq!(after.3.len(), 1);
    // Cancelling the search returns to the visual selection, remapped.
    cx.simulate_keystrokes("escape");
    draw(cx);
    let resumed = held(cx);
    assert!(!resumed.0, "the command line closed");
    assert_eq!(resumed.1, after.1);
    assert_eq!(resumed.2, after.2);
    assert_eq!(resumed.3, after.3);
}

#[gpui::test]
fn reopen_keeps_block_dollar_goal(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = vim_reopened_as_shift_jis(cx, b"abcdef\nabc\ncaf\xE9\n");
    cx.simulate_keystrokes("l ctrl-v j $");
    draw(cx);
    let before = selections(&editor, cx);
    assert_eq!(before.len(), 2, "one range per row");
    assert_eq!(before[0].range, 1..6);
    assert_eq!(before[1].range, 8..10);

    reopen_as_windows_1252(&workbench, &editor, cx);
    assert_eq!(text(&editor, cx), "abcdef\nabc\ncafé\n");
    assert_eq!(selections(&editor, cx), before);
    let mode = editor.read_with(cx, |editor, _| editor.vim_state().unwrap().mode());
    assert_eq!(mode, fe_vim::Mode::Visual(fe_vim::VisualKind::Block));
}

#[gpui::test]
fn reopen_keeps_viewport(cx: &mut TestAppContext) {
    let mut bytes = b"top \xE9\n".to_vec();
    for row in 0..200 {
        bytes.extend_from_slice(format!("row {row}\n").as_bytes());
    }
    let (_dir, workbench, editor, cx) = vim_reopened_as_shift_jis(cx, &bytes);
    cx.simulate_keystrokes("v l");
    draw(cx);
    // Scrolled away from the visual selection at the top.
    editor.update(cx, |editor, cx| editor.set_scroll_top(100., cx));
    draw(cx);
    let top = editor.read_with(cx, |editor, _| editor.scroll_top());
    assert!(top > 50., "scrolled to {top}");

    reopen_as_windows_1252(&workbench, &editor, cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.scroll_top()), top);
}
