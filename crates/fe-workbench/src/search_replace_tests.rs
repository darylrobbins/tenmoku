//! Replacing the project search's matches from the search panel: Replace
//! All asks first with the counts; open files get one undo step, left
//! unsaved; the others are written in their own encoding and line endings;
//! one file, or all but the dismissed ones; the files matched again as they
//! are now; and the results list's keys.

use std::{cell::RefCell, collections::HashMap, fs, path::Path, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand, set_vim_enabled};
use fe_project::SearchOptions;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{SearchPanel, Workbench, search_panel::ShownRow, tests::open_root};

thread_local! {
    /// The saves [`gate`] holds, by the file's name in the results.
    static SAVE_GATES: RefCell<HashMap<String, async_channel::Receiver<()>>> = RefCell::default();
}

/// Holds the save of the file the results name `rel`, once it's prepared,
/// until the sender sends or is dropped.
fn hold_save(rel: &str) -> async_channel::Sender<()> {
    let (release, held) = async_channel::bounded(1);
    SAVE_GATES.with(|gates| gates.borrow_mut().insert(rel.to_owned(), held));
    release
}

/// When [`write_hook`] runs, in the write of a file not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WriteStage {
    /// The new file is written beside it, not yet renamed over it.
    BeforeRename,
    /// It's renamed over it, and its stamp taken.
    AfterRename,
}

/// What [`on_write`] sets.
type WriteHook = Box<dyn FnMut(WriteStage, &Path)>;

thread_local! {
    /// What [`write_hook`] runs, if anything.
    static WRITE_HOOK: RefCell<Option<WriteHook>> = RefCell::default();
}

/// Runs `hook` at each stage of each write of a file not open, with the
/// file's path relative to the project.
fn on_write(hook: impl FnMut(WriteStage, &Path) + 'static) {
    WRITE_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

/// Runs the hook [`on_write`] set, if any.
pub(super) fn write_hook(stage: WriteStage, rel: &Path) {
    WRITE_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(stage, rel);
        }
    });
}

/// Waits while the save of `rel` is held.
pub(super) async fn gate(rel: &str) {
    let held = SAVE_GATES.with(|gates| gates.borrow_mut().remove(rel));
    if let Some(held) = held {
        held.recv().await.ok();
    }
}

/// A project with `files` (relative path, bytes) in a temporary directory.
fn project(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    dir
}

/// A workbench rooted at `root`, focused, with search shown on one walker
/// thread (so files come in path order) and `query` run in it.
fn searching<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    query: &str,
) -> (
    Entity<Workbench>,
    Entity<SearchPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    search.update(cx, |search, _| {
        search.set_options(SearchOptions {
            threads: 1,
            ..SearchOptions::default()
        })
    });
    cx.simulate_keystrokes("cmd-shift-f");
    search_for(query, cx);
    (workbench, search, cx)
}

/// Replaces the focused query with `query` and runs it.
fn search_for(query: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input(query);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs `command` as a key would: on the focused element, in the context
/// its key contexts make.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context: ContextStack = window
            .context_stack()
            .iter()
            .map(|context| ContextFrame::new(context.primary().map_or("", |entry| &entry.key)))
            .collect();
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(cx);
}

/// Sets the replace field's text.
fn replace_with(search: &Entity<SearchPanel>, text: &str, cx: &mut VisualTestContext) {
    let input = search.read_with(cx, |search, _| search.replace_input().clone());
    input.update(cx, |input, cx| input.set_text(text, cx));
    draw(cx);
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

/// Clicks the button `selector` names on result row `row`.
fn click_row_button(selector: &str, row: usize, cx: &mut VisualTestContext) {
    let selector: &'static str = format!("{selector}:{row}").leak();
    click(selector, cx);
}

fn prompt(cx: &mut VisualTestContext) -> Option<String> {
    cx.pending_prompt().map(|(question, _)| question)
}

fn answer(button: &str, cx: &mut VisualTestContext) {
    cx.simulate_prompt_answer(button);
    draw(cx);
}

fn status(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Option<String> {
    search.read_with(cx, |search, _| search.status_text())
}

/// The files with rows, in order.
fn files(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    let rows = search.read_with(cx, |search, _| search.shown_rows());
    (rows.into_iter())
        .filter_map(|row| match row {
            ShownRow::File(rel, _) => Some(rel),
            ShownRow::Line(..) => None,
        })
        .collect()
}

fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap()
}

/// Opens `name` in the active pane, focused.
fn open(
    workbench: &Entity<Workbench>,
    dir: &Path,
    name: &str,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.join(name);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    active_editor(workbench, cx)
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows")
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn dirty(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.model().is_dirty())
}

fn focus(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

fn focus_search(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) {
    let input = search.read_with(cx, |search, _| search.input().clone());
    cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    draw(cx);
}

fn tab_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
}

#[gpui::test]
fn confirm_counts(cx: &mut TestAppContext) {
    let a = "foo foo\nfoo\n";
    let b = "x foo\nfoo y\n";
    let dir = project(&[("a.rs", a.as_bytes()), ("b.rs", b.as_bytes())]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    run("search.show_replace", cx);
    let replace_focused = cx.update(|window, cx| {
        let input = search.read(cx).replace_input().clone();
        input.focus_handle(cx).is_focused(window)
    });
    assert!(replace_focused, "show_replace focuses the replace field");
    replace_with(&search, "bar", cx);
    let question = Some("Replace 5 occurrences across 2 files with “bar”?".to_owned());

    // From the panel's button.
    click("search-replace-all", cx);
    assert_eq!(prompt(cx), question);
    answer("Cancel", cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(
        (read(dir.path(), "a.rs"), read(dir.path(), "b.rs")),
        (a.into(), b.into())
    );
    assert_eq!(status(&search, cx).as_deref(), Some("5 results in 2 files"));

    // From the palette, with an editor focused: the workbench's command.
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    focus(&editor, cx);
    run("search.replace_all", cx);
    assert_eq!(prompt(cx), question);
    answer("Cancel", cx);
    assert_eq!(
        (read(dir.path(), "a.rs"), read(dir.path(), "b.rs")),
        (a.into(), b.into())
    );
    assert_eq!(text(&editor, cx), a);
    assert!(!dirty(&editor, cx));

    // Replace does it.
    run("search.replace_all", cx);
    assert_eq!(prompt(cx), question);
    answer("Replace", cx);
    assert_eq!(text(&editor, cx), "bar bar\nbar\n");
    assert_eq!(read(dir.path(), "b.rs"), "x bar\nbar y\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 5 occurrences in 2 files")
    );
}

/// One run of `open_and_unopened`: a.rs open (typed in first if `dirty`),
/// b.rs not, in UTF-16LE with a BOM and CRLF line endings; one undo, cmd-z
/// or vim's `u`, takes a.rs's replacement back.
fn open_and_unopened_case(dirty_first: bool, vim: bool, cx: &mut TestAppContext) {
    let a = "let foo = foo();\n";
    let mut b = vec![0xFF, 0xFE];
    for unit in "foo\r\nnot é\r\nfoo foo\r\n".encode_utf16() {
        b.extend(unit.to_le_bytes());
    }
    let dir = project(&[("a.rs", a.as_bytes()), ("b.rs", &b)]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    assert_eq!(files(&search, cx), ["a.rs", "b.rs"]);
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    focus(&editor, cx);
    let before = if dirty_first {
        cx.simulate_input("// ");
        draw(cx);
        format!("// {a}")
    } else {
        a.to_owned()
    };
    assert_eq!(text(&editor, cx), before);
    assert_eq!(dirty(&editor, cx), dirty_first);
    if vim {
        cx.update(|_, cx| set_vim_enabled(cx, true));
        draw(cx);
    }
    let tabs = tab_count(&workbench, cx);

    focus_search(&search, cx);
    run("search.show_replace", cx);
    replace_with(&search, "bar", cx);
    run("search.replace_all", cx);
    assert_eq!(
        prompt(cx).as_deref(),
        Some("Replace 5 occurrences across 2 files with “bar”?")
    );
    answer("Replace", cx);
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 5 occurrences in 2 files"),
        "dirty {dirty_first}, vim {vim}"
    );

    // a.rs: its buffer, left unsaved.
    assert_eq!(text(&editor, cx), before.replace("foo", "bar"));
    assert!(dirty(&editor, cx));
    assert_eq!(read(dir.path(), "a.rs"), a);
    // b.rs: written in its encoding, BOM and line endings, and not opened.
    let mut expected = vec![0xFF, 0xFE];
    for unit in "bar\r\nnot é\r\nbar bar\r\n".encode_utf16() {
        expected.extend(unit.to_le_bytes());
    }
    assert_eq!(fs::read(dir.path().join("b.rs")).unwrap(), expected);
    assert_eq!(tab_count(&workbench, cx), tabs);

    // One undo takes the whole file's replacement back.
    focus(&editor, cx);
    if vim {
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.vim_mode()),
            Some("normal")
        );
        cx.simulate_keystrokes("u");
    } else {
        cx.simulate_keystrokes("cmd-z");
    }
    draw(cx);
    assert_eq!(text(&editor, cx), before, "dirty {dirty_first}, vim {vim}");
    assert_eq!(dirty(&editor, cx), dirty_first);
    if vim {
        cx.update(|_, cx| set_vim_enabled(cx, false));
    }
}

#[gpui::test]
fn open_and_unopened(cx: &mut TestAppContext) {
    open_and_unopened_case(false, false, cx);
    open_and_unopened_case(true, false, cx);
    open_and_unopened_case(true, true, cx);
    open_and_unopened_case(false, true, cx);
}

#[gpui::test]
fn per_file_and_dismiss(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a.rs", b"foo a\n"),
        ("b.rs", b"foo b\nfoo\n"),
        ("c.rs", b"foo c\n"),
    ]);
    let (_, search, cx) = searching(cx, dir.path(), "foo");
    assert_eq!(files(&search, cx), ["a.rs", "b.rs", "c.rs"]);
    click("search-replace-toggle", cx);
    replace_with(&search, "bar", cx);

    // Rows: a.rs and its line, b.rs and its two, c.rs and its line. b.rs's
    // Replace touches b.rs only, asking nothing.
    click_row_button("search-replace-file", 2, cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(read(dir.path(), "b.rs"), "bar b\nbar\n");
    assert_eq!(read(dir.path(), "a.rs"), "foo a\n");
    assert_eq!(read(dir.path(), "c.rs"), "foo c\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 2 occurrences in 1 file")
    );
    // The search ran again.
    assert_eq!(files(&search, cx), ["a.rs", "c.rs"]);

    // Dismissing a.rs drops it, and Replace All skips it.
    click_row_button("search-dismiss", 0, cx);
    assert_eq!(files(&search, cx), ["c.rs"]);
    click("search-replace-all", cx);
    assert_eq!(
        prompt(cx).as_deref(),
        Some("Replace 1 occurrence across 1 file with “bar”?")
    );
    answer("Replace", cx);
    assert_eq!(read(dir.path(), "c.rs"), "bar c\n");
    assert_eq!(read(dir.path(), "a.rs"), "foo a\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 1 occurrence in 1 file")
    );
    // The search ran again, and a.rs still matches.
    assert_eq!(files(&search, cx), ["a.rs"]);
}

#[gpui::test]
fn stale_and_errors(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = project(&[
        ("a.rs", b"foo\n"),
        ("b.rs", b"foo foo\n"),
        ("c.rs", b"foo\n"),
        ("d.rs", b"foo d\n"),
    ]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    assert_eq!(files(&search, cx), ["a.rs", "b.rs", "c.rs", "d.rs"]);

    // Since the search: a.rs's buffer and b.rs on disk changed, moving and
    // adding matches, and c.rs can't be read.
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    focus(&editor, cx);
    cx.simulate_input("zz ");
    draw(cx);
    assert_eq!(text(&editor, cx), "zz foo\n");
    fs::write(dir.path().join("b.rs"), "xx\nfoo, foo\nyy foo\n").unwrap();
    let c = dir.path().join("c.rs");
    fs::set_permissions(&c, fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads it anyway.
    let unreadable = fs::read(&c).is_err();
    assert!(unreadable || unsafe { libc::geteuid() } == 0);

    focus_search(&search, cx);
    run("search.toggle_replace", cx);
    replace_with(&search, "bar", cx);
    run("search.replace_all", cx);
    assert_eq!(
        prompt(cx).as_deref(),
        Some("Replace 5 occurrences across 4 files with “bar”?")
    );
    answer("Replace", cx);

    assert_eq!(text(&editor, cx), "zz bar\n");
    assert_eq!(read(dir.path(), "b.rs"), "xx\nbar, bar\nyy bar\n");
    assert_eq!(read(dir.path(), "d.rs"), "bar d\n");
    let status = status(&search, cx).unwrap();
    if unreadable {
        let prefix = "Replaced 5 occurrences in 3 files; skipped c.rs (can't read it: ";
        assert!(status.starts_with(prefix), "{status}");
        let shown = search.read_with(cx, |search, _| search.status()).unwrap();
        assert!(shown.1, "shown as an error");
        fs::set_permissions(&c, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read(dir.path(), "c.rs"), "foo\n");
    }
}

#[gpui::test]
fn results_keys(cx: &mut TestAppContext) {
    let dir = project(&[("a.rs", b"one foo\n"), ("b.rs", b"foo\nx\nfoo two\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    // Rows: a.rs, its line, b.rs, its two lines.
    let selected = |cx: &mut VisualTestContext| search.read_with(cx, |search, _| search.selected());
    let results_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| search.read(cx).results_focused(window))
    };
    let context_has = |identifier: &str, cx: &mut VisualTestContext| {
        cx.update(|window, _| {
            window
                .context_stack()
                .iter()
                .any(|context| context.contains("search_results") && context.contains(identifier))
        })
    };

    run("search.focus_results", cx);
    assert!(results_focused(cx));
    assert!(context_has("search_results", cx));
    assert!(!context_has("vim", cx));
    assert_eq!(selected(cx), Some(1));
    run("search.select_next", cx);
    assert_eq!(selected(cx), Some(3));
    run("search.select_next", cx);
    assert_eq!(selected(cx), Some(4));
    run("search.select_next", cx);
    assert_eq!(selected(cx), Some(4), "the last stays");
    run("search.select_previous", cx);
    assert_eq!(selected(cx), Some(3));
    run("search.select_previous", cx);
    assert_eq!(selected(cx), Some(1));
    run("search.select_previous", cx);
    assert_eq!(selected(cx), Some(1), "the first stays");

    run("search.select_next", cx);
    run("search.open_selected", cx);
    let editor = active_editor(&workbench, cx);
    let (path, selection) = editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.selections()[0].range.clone(),
        )
    });
    assert_eq!(path, dir.path().join("b.rs"));
    assert_eq!(selection, 0..3);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // With vim on: the results carry the flag, and the editor opens in
    // normal mode. Down from the query focuses the results.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.simulate_keystrokes("cmd-shift-f");
    draw(cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    assert!(results_focused(cx));
    assert!(context_has("vim", cx));
    assert_eq!(selected(cx), Some(3), "the selection stayed");
    run("search.select_next", cx);
    run("search.open_selected", cx);
    let editor = active_editor(&workbench, cx);
    let (mode, caret) = editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (editor.vim_mode(), model.selections()[0].range.clone())
    });
    assert_eq!(mode, Some("normal"));
    let at = "foo\nx\n".len();
    assert!(caret.start == at, "{caret:?}");
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
}

/// Turns the regex option on, rerunning the query.
fn regex_on(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) {
    focus_search(search, cx);
    run("search.toggle_regex", cx);
}

/// Replaces all with `template`, answering Replace.
fn replace_all(search: &Entity<SearchPanel>, template: &str, cx: &mut VisualTestContext) {
    replace_with(search, template, cx);
    focus_search(search, cx);
    run("search.replace_all", cx);
    assert!(prompt(cx).is_some());
    answer("Replace", cx);
}

#[gpui::test]
fn byte_regex_matches_inside_a_char(cx: &mut TestAppContext) {
    // `(?-u:.)` matches each byte of é: both widen to the char, and the
    // two are one replacement there, open file or not.
    let dir = project(&[("a.rs", "é\n".as_bytes()), ("b.rs", "é\n".as_bytes())]);
    let (workbench, search, cx) = searching(cx, dir.path(), "(?-u:.)");
    regex_on(&search, cx);
    assert_eq!(files(&search, cx), ["a.rs", "b.rs"]);
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    replace_all(&search, "x", cx);
    assert_eq!(text(&editor, cx), "xx\n");
    assert_eq!(read(dir.path(), "b.rs"), "xx\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 4 occurrences in 2 files")
    );
    focus(&editor, cx);
    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    assert_eq!(text(&editor, cx), "é\n");
}

#[gpui::test]
fn swapped_files_are_skipped(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a.rs", b"foo a\n"),
        ("b.rs", b"foo b\n"),
        ("d.rs", b"foo d\n"),
        ("sub/c.rs", b"foo c\n"),
    ]);
    let outside = project(&[("b.rs", b"foo out\n"), ("sub/c.rs", b"foo out\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    assert_eq!(files(&search, cx), ["a.rs", "b.rs", "d.rs", "sub/c.rs"]);

    // Since the search: b.rs is a symlink out of the project, whose target
    // is open in an editor; d.rs a FIFO; and sub a symlink to a folder out
    // of it.
    let out_b = outside.path().join("b.rs");
    let out_editor = {
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        let path = out_b.clone();
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
        active_editor(&workbench, cx)
    };
    let path = |name: &str| dir.path().join(name);
    fs::remove_file(path("b.rs")).unwrap();
    std::os::unix::fs::symlink(&out_b, path("b.rs")).unwrap();
    fs::remove_file(path("d.rs")).unwrap();
    let fifo = std::ffi::CString::new(path("d.rs").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);
    fs::rename(path("sub"), path("old_sub")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("sub"), path("sub")).unwrap();

    replace_all(&search, "bar", cx);
    let status = status(&search, cx).unwrap();
    assert!(
        status.starts_with("Replaced 1 occurrence in 1 file; skipped "),
        "{status}"
    );
    for skipped in [
        "b.rs (it's no longer a regular file)",
        "d.rs (it's no longer a regular file)",
        "sub/c.rs (it's no longer in the project)",
    ] {
        assert!(status.contains(skipped), "{skipped} in {status}");
    }
    assert_eq!(read(dir.path(), "a.rs"), "bar a\n");
    assert_eq!(read(outside.path(), "b.rs"), "foo out\n");
    assert_eq!(read(outside.path(), "sub/c.rs"), "foo out\n");
    assert_eq!(text(&out_editor, cx), "foo out\n");
    assert!(!dirty(&out_editor, cx));
}

// Each seed orders the pane's read and the replacement's steps its own way:
// some open b.rs before it's read (replaced in the editor), some read it
// before it's saved and land after (reloaded).
#[gpui::test(iterations = 40)]
fn opened_while_replacing(cx: &mut TestAppContext) {
    let dir = project(&[("a.rs", b"foo a\n"), ("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    replace_with(&search, "bar", cx);
    run("search.replace_all", cx);
    assert!(prompt(cx).is_some());

    // b.rs opens as the replacement runs: it's replaced in its editor, or
    // saved and its editor brought up to the file; one undo takes it back.
    cx.simulate_prompt_answer("Replace");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let b = dir.path().join("b.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(b, window, cx)));
    for _ in 0..10 {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    draw(cx);
    let editor = active_editor(&workbench, cx);
    assert_eq!(text(&editor, cx), "bar b\n");
    assert_eq!(read(dir.path(), "a.rs"), "bar a\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 2 occurrences in 2 files")
    );
    focus(&editor, cx);
    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    assert_eq!(text(&editor, cx), "foo b\n");
}

#[gpui::test]
fn a_binary_open_buffer_is_skipped(cx: &mut TestAppContext) {
    let dir = project(&[("a.rs", b"foo a\n"), ("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    editor.update(cx, |view, _| view.model_mut().insert("\0"));
    replace_all(&search, "bar", cx);
    assert_eq!(text(&editor, cx), "\0foo a\n");
    assert_eq!(read(dir.path(), "b.rs"), "bar b\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 1 occurrence in 1 file; skipped a.rs (it's binary)")
    );
}

#[gpui::test]
fn a_file_open_in_two_windows_counts_once(cx: &mut TestAppContext) {
    let dir = project(&[("a.rs", b"foo foo\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    let launch = crate::Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let other = cx.cx.add_window(move |window, cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        Workbench::new(
            launch,
            theme,
            fe_gpui::EditorSettings::default(),
            window,
            cx,
        )
    });
    let other_workbench = other.root(&mut cx.cx).unwrap();
    let mut other_cx = VisualTestContext::from_window(other.into(), &cx.cx);
    let other_editor = open(&other_workbench, dir.path(), "a.rs", &mut other_cx);

    replace_all(&search, "bar", cx);
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 2 occurrences in 1 file")
    );
    assert_eq!(text(&editor, cx), "bar bar\n");
    assert_eq!(text(&other_editor, &mut other_cx), "bar bar\n");
    assert_eq!(read(dir.path(), "a.rs"), "foo foo\n");
}

#[gpui::test]
fn a_stale_click_selects_nothing(cx: &mut TestAppContext) {
    let dir = project(&[("a.rs", b"foo a\n"), ("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    // Rows: a.rs, its line, b.rs, its line. A click on b.rs's line in a
    // frame from before a.rs was dismissed.
    let layout = search.read_with(cx, |search, _| search.layout);
    search.update(cx, |search, cx| search.dismiss(layout, 0, cx));
    cx.update(|window, cx| {
        search.update(cx, |search, cx| search.click_line(layout, 3, window, cx))
    });
    draw(cx);
    assert_eq!(search.read_with(cx, |search, _| search.selected()), None);
    assert_eq!(tab_count(&workbench, cx), 0);

    // A selection past the rows moves nowhere it can't.
    search.update(cx, |search, cx| {
        search.selected = Some(10);
        search.select_next(false, cx);
        search.select_next(true, cx);
    });
    assert_eq!(search.read_with(cx, |search, _| search.selected()), Some(1));
}

#[gpui::test]
fn show_replace_seeds_the_query(cx: &mut TestAppContext) {
    let dir = project(&[("a.rs", b"let foo = 1;\nfoo();\n"), ("b.rs", b"bar\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "bar");
    assert_eq!(files(&search, cx), ["b.rs"]);
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    let query =
        |cx: &mut VisualTestContext| search.read_with(cx, |search, cx| search.query(cx).pattern);

    // A selection across rows seeds nothing.
    editor.update(cx, |editor, cx| editor.select_and_reveal(4..17, cx));
    focus(&editor, cx);
    run("search.show_replace", cx);
    assert_eq!(query(cx), "bar");

    // One within a row does, and runs it, with the replace field focused.
    editor.update(cx, |editor, cx| editor.select_and_reveal(4..7, cx));
    focus(&editor, cx);
    run("search.show_replace", cx);
    assert_eq!(query(cx), "foo");
    assert_eq!(files(&search, cx), ["a.rs"]);
    let replace_focused = cx.update(|window, cx| {
        let input = search.read(cx).replace_input().clone();
        input.focus_handle(cx).is_focused(window)
    });
    assert!(replace_focused);
}

/// Starts Replace All with `template`, answering Replace, and runs until
/// it's done or held.
fn start_replace_all(search: &Entity<SearchPanel>, template: &str, cx: &mut VisualTestContext) {
    replace_with(search, template, cx);
    focus_search(search, cx);
    run("search.replace_all", cx);
    answer("Replace", cx);
}

#[gpui::test]
fn opened_during_an_earlier_save(cx: &mut TestAppContext) {
    // b.rs, read but not yet saved, opens and is typed in while a.rs saves:
    // it's replaced in its editor, unsaved.
    let dir = project(&[("a.rs", b"foo a\n"), ("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let release = hold_save("a.rs");
    start_replace_all(&search, "bar", cx);
    assert_eq!(read(dir.path(), "a.rs"), "foo a\n", "held");
    let editor = open(&workbench, dir.path(), "b.rs", cx);
    focus(&editor, cx);
    cx.simulate_input("// ");
    draw(cx);
    drop(release);
    draw(cx);

    assert_eq!(read(dir.path(), "a.rs"), "bar a\n");
    assert_eq!(read(dir.path(), "b.rs"), "foo b\n");
    assert_eq!(text(&editor, cx), "// bar b\n");
    assert!(dirty(&editor, cx));
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 2 occurrences in 2 files")
    );
    focus(&editor, cx);
    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    assert_eq!(text(&editor, cx), "// foo b\n");
}

#[gpui::test]
fn opened_during_its_own_save(cx: &mut TestAppContext) {
    // b.rs opens, loading the text from before its save, and is typed in:
    // the save lands, and the editor gets the replacement on its text,
    // unsaved. Edited, it isn't what's on disk, so it still knows the file
    // as it loaded it: saving it asks before overwriting the replacement.
    let dir = project(&[("a.rs", b"foo a\n"), ("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let release = hold_save("b.rs");
    start_replace_all(&search, "bar", cx);
    assert_eq!(read(dir.path(), "a.rs"), "bar a\n");
    let editor = open(&workbench, dir.path(), "b.rs", cx);
    assert_eq!(text(&editor, cx), "foo b\n");
    focus(&editor, cx);
    cx.simulate_input("// ");
    draw(cx);
    drop(release);
    draw(cx);

    assert_eq!(read(dir.path(), "b.rs"), "bar b\n");
    assert_eq!(text(&editor, cx), "// bar b\n");
    assert!(dirty(&editor, cx));
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 2 occurrences in 2 files")
    );
    focus(&editor, cx);
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(
        prompt(cx).as_deref(),
        Some("“b.rs” changed on disk since it was opened.")
    );
    answer("Overwrite", cx);
    assert_eq!(read(dir.path(), "b.rs"), "// bar b\n");
    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    assert_eq!(text(&editor, cx), "// foo b\n");
}

#[gpui::test]
fn an_opening_slower_than_five_seconds_is_waited_for(cx: &mut TestAppContext) {
    // A pane starts reading b.rs while its save is held, and the read lands,
    // with the text from before, long after the save.
    let dir = project(&[("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let release = hold_save("b.rs");
    start_replace_all(&search, "bar", cx);
    let path = dir.path().join("b.rs");
    let before = crate::loader::read(path.clone()).unwrap().unwrap();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    pane.update(cx, |pane, cx| pane.opening(path.clone(), cx));
    drop(release);
    draw(cx);
    assert_eq!(read(dir.path(), "b.rs"), "bar b\n");
    assert_eq!(status(&search, cx).as_deref(), Some("Replacing…"));
    for _ in 0..70 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replacing…"),
        "still waiting"
    );

    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.file_read(path, Ok(before.into()), window, cx)
        })
    });
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    let editor = active_editor(&workbench, cx);
    assert_eq!(text(&editor, cx), "bar b\n");
    assert!(!dirty(&editor, cx), "it's what's on disk");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 1 occurrence in 1 file")
    );
}

#[gpui::test]
fn a_moved_project_folder_is_skipped(cx: &mut TestAppContext) {
    // The project folder's path is a symlink to another folder with the
    // same files since the search.
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("a.rs"), "foo a\n").unwrap();
    let other = project(&[("a.rs", b"foo other\n")]);
    let (_, search, cx) = searching(cx, &root, "foo");
    assert_eq!(files(&search, cx), ["a.rs"]);
    fs::rename(&root, parent.path().join("old")).unwrap();
    std::os::unix::fs::symlink(other.path(), &root).unwrap();

    replace_all(&search, "bar", cx);
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced nothing; skipped a.rs (the project folder has moved)")
    );
    assert_eq!(read(other.path(), "a.rs"), "foo other\n");
    assert_eq!(read(&parent.path().join("old"), "a.rs"), "foo a\n");
}

#[gpui::test]
fn swaps_between_reading_and_saving_are_skipped(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a.rs", b"foo a\n"),
        ("d.rs", b"foo d\n"),
        ("sub/c.rs", b"foo c\n"),
    ]);
    let outside = project(&[("sub/c.rs", b"foo out\n")]);
    let (_, search, cx) = searching(cx, dir.path(), "foo");
    assert_eq!(files(&search, cx), ["a.rs", "d.rs", "sub/c.rs"]);

    // While d.rs's save is held, all three read: d.rs changes, and sub
    // becomes a symlink out of the project.
    let release = hold_save("d.rs");
    start_replace_all(&search, "bar", cx);
    let path = |name: &str| dir.path().join(name);
    fs::write(path("d.rs"), "foo d, again\n").unwrap();
    fs::rename(path("sub"), path("old_sub")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("sub"), path("sub")).unwrap();
    drop(release);
    draw(cx);

    assert_eq!(
        status(&search, cx).as_deref(),
        Some(
            "Replaced 1 occurrence in 1 file; skipped d.rs (it changed on disk since it was \
             read), sub/c.rs (it's no longer in the project)"
        )
    );
    assert_eq!(read(dir.path(), "a.rs"), "bar a\n");
    assert_eq!(read(dir.path(), "d.rs"), "foo d, again\n");
    assert_eq!(read(outside.path(), "sub/c.rs"), "foo out\n");
    assert_eq!(read(dir.path(), "old_sub/c.rs"), "foo c\n");
}

#[gpui::test]
fn seeding_runs_an_unsubmitted_query(cx: &mut TestAppContext) {
    // The query field reads foo, typed but not run: the results are bar's.
    // Seeding it with foo runs foo, so Replace All replaces foo.
    let dir = project(&[("a.rs", b"let foo = 1;\n"), ("b.rs", b"bar\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "bar");
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("foo");
    draw(cx);
    assert_eq!(files(&search, cx), ["b.rs"]);
    let editor = open(&workbench, dir.path(), "a.rs", cx);
    editor.update(cx, |editor, cx| editor.select_and_reveal(4..7, cx));
    focus(&editor, cx);
    run("search.show_replace", cx);
    assert_eq!(files(&search, cx), ["a.rs"]);
    replace_all(&search, "baz", cx);
    assert_eq!(text(&editor, cx), "let baz = 1;\n");
    assert_eq!(read(dir.path(), "b.rs"), "bar\n");
}

#[gpui::test]
fn a_folder_swapped_during_the_write_is_refused(cx: &mut TestAppContext) {
    // Once sub/c.rs is written beside itself and checked, sub moves out of
    // the project and a symlink to it takes its place: the rename into it
    // is refused, and nothing out of the project is written.
    let dir = project(&[("a.rs", b"foo a\n"), ("sub/c.rs", b"foo c\n")]);
    let outside = tempfile::tempdir().unwrap();
    let (_, search, cx) = searching(cx, dir.path(), "foo");
    assert_eq!(files(&search, cx), ["a.rs", "sub/c.rs"]);
    let (sub, moved) = (dir.path().join("sub"), outside.path().join("sub"));
    on_write({
        let (sub, moved) = (sub.clone(), moved.clone());
        move |stage, rel| {
            if stage == WriteStage::BeforeRename && rel == Path::new("sub/c.rs") {
                fs::rename(&sub, &moved).unwrap();
                std::os::unix::fs::symlink(&moved, &sub).unwrap();
            }
        }
    });
    replace_all(&search, "bar", cx);

    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 1 occurrence in 1 file; skipped sub/c.rs (it's no longer in the project)")
    );
    assert_eq!(read(dir.path(), "a.rs"), "bar a\n");
    assert_eq!(read(&moved, "c.rs"), "foo c\n");
    let left: Vec<_> = fs::read_dir(&moved)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["c.rs"], "no new file is left behind");
}

#[gpui::test]
fn the_save_s_own_stamp_is_kept(cx: &mut TestAppContext) {
    // b.rs opens during its save, unedited; another writer changes it just
    // after the save. The editor knows the file as the save wrote it, so
    // saving it again asks before overwriting the other writer's change.
    let dir = project(&[("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let path = dir.path().join("b.rs");
    on_write({
        let path = path.clone();
        move |stage, _| {
            if stage == WriteStage::AfterRename {
                fs::write(&path, "another writer's text\n").unwrap();
            }
        }
    });
    let release = hold_save("b.rs");
    start_replace_all(&search, "bar", cx);
    let editor = open(&workbench, dir.path(), "b.rs", cx);
    drop(release);
    draw(cx);
    assert_eq!(text(&editor, cx), "bar b\n");
    assert_eq!(read(dir.path(), "b.rs"), "another writer's text\n");

    focus(&editor, cx);
    cx.simulate_input("x");
    draw(cx);
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert!(prompt(cx).is_some(), "asks before overwriting");
    assert_eq!(read(dir.path(), "b.rs"), "another writer's text\n");
}

#[gpui::test]
fn a_clean_editor_opened_during_the_save_matches_the_disk(cx: &mut TestAppContext) {
    // b.rs opens during its save, unedited: once replaced it's what's on
    // disk, so it's clean; one undo makes it differ from the disk, dirty,
    // and closing it asks.
    let dir = project(&[("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let release = hold_save("b.rs");
    start_replace_all(&search, "bar", cx);
    let editor = open(&workbench, dir.path(), "b.rs", cx);
    assert_eq!(text(&editor, cx), "foo b\n");
    drop(release);
    draw(cx);
    assert_eq!(read(dir.path(), "b.rs"), "bar b\n");
    assert_eq!(text(&editor, cx), "bar b\n");
    assert!(!dirty(&editor, cx));

    focus(&editor, cx);
    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    assert_eq!(text(&editor, cx), "foo b\n");
    assert!(dirty(&editor, cx), "it differs from the disk");
    run("workbench.close_tab", cx);
    assert_eq!(
        prompt(cx).as_deref(),
        Some("Discard unsaved changes to b.rs?")
    );
    answer("Cancel", cx);
    assert_eq!(tab_count(&workbench, cx), 1);
}

#[gpui::test]
fn an_editor_opened_meanwhile_is_checked_first(cx: &mut TestAppContext) {
    // Since the search, b.rs's text on disk has no match, and c.rs has a
    // byte that isn't UTF-8 past the 64 KiB its encoding is told by, so it
    // can't be written back as it reads. Both open while
    // a.rs's save is held, and b.rs is typed into with a match: their
    // editors are replaced all the same.
    let dir = project(&[
        ("a.rs", b"foo a\n"),
        ("b.rs", b"foo b\n"),
        ("c.rs", b"foo c\n"),
    ]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    fs::write(dir.path().join("b.rs"), "baz b\n").unwrap();
    let mut bad = b"foo c\n".to_vec();
    bad.extend("x".repeat(70_000).bytes());
    bad.extend(b"\xff\n");
    fs::write(dir.path().join("c.rs"), &bad).unwrap();
    let release = hold_save("a.rs");
    start_replace_all(&search, "bar", cx);
    let b = open(&workbench, dir.path(), "b.rs", cx);
    focus(&b, cx);
    cx.simulate_input("foo ");
    draw(cx);
    let c = open(&workbench, dir.path(), "c.rs", cx);
    drop(release);
    draw(cx);

    assert_eq!(read(dir.path(), "a.rs"), "bar a\n");
    assert_eq!(text(&b, cx), "bar baz b\n");
    assert_eq!(read(dir.path(), "b.rs"), "baz b\n");
    assert!(text(&c, cx).starts_with("bar c\n"));
    assert_eq!(fs::read(dir.path().join("c.rs")).unwrap(), bad);
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 3 occurrences in 3 files")
    );
}

#[gpui::test]
fn a_binary_editor_opened_during_the_save_is_named(cx: &mut TestAppContext) {
    // b.rs opens during its save and gets a NUL and another match typed in:
    // the file is saved, but its editor is left as it is, said so, and
    // knows the file as it read it.
    let dir = project(&[("b.rs", b"foo b\n")]);
    let (workbench, search, cx) = searching(cx, dir.path(), "foo");
    let release = hold_save("b.rs");
    start_replace_all(&search, "bar", cx);
    let editor = open(&workbench, dir.path(), "b.rs", cx);
    editor.update(cx, |view, _| view.model_mut().insert("\0foo "));
    drop(release);
    draw(cx);
    assert_eq!(read(dir.path(), "b.rs"), "bar b\n");
    assert_eq!(text(&editor, cx), "\0foo foo b\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some(
            "Replaced 1 occurrence in 1 file; skipped b.rs (it's binary: saved, but not in its editor)"
        )
    );
}

#[gpui::test]
fn a_folder_above_the_project_swapped_during_the_write_is_refused(cx: &mut TestAppContext) {
    // Once b.rs is written beside itself, the folder holding the project
    // moves out, and a symlink to it takes its place: the project's path
    // leads to the same folder, but not the way the search found it, so
    // the rename is refused, and nothing is written there.
    let parent = tempfile::tempdir().unwrap();
    let container = parent.path().join("container");
    let root = container.join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.rs"), "foo a\n").unwrap();
    fs::write(root.join("b.rs"), "foo b\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let moved = outside.path().join("container");
    let (_, search, cx) = searching(cx, &root, "foo");
    assert_eq!(files(&search, cx), ["a.rs", "b.rs"]);
    on_write({
        let (container, moved) = (container.clone(), moved.clone());
        move |stage, rel| {
            if stage == WriteStage::BeforeRename && rel == Path::new("b.rs") {
                fs::rename(&container, &moved).unwrap();
                std::os::unix::fs::symlink(&moved, &container).unwrap();
            }
        }
    });
    replace_all(&search, "bar", cx);

    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 1 occurrence in 1 file; skipped b.rs (the project folder has moved)")
    );
    let project = moved.join("project");
    assert_eq!(read(&project, "a.rs"), "bar a\n");
    assert_eq!(read(&project, "b.rs"), "foo b\n");
    let mut left: Vec<_> = fs::read_dir(&project)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    left.sort();
    assert_eq!(left, ["a.rs", "b.rs"], "no new file is left behind");
}

#[gpui::test]
fn a_project_beneath_a_folder_that_can_only_be_searched(cx: &mut TestAppContext) {
    // A folder above the project can be passed through but not listed. On
    // macOS the search reaches the project through it (`O_SEARCH`), so the
    // replacement must too, open file or not. (On Linux the search's walk
    // opens folders to read, so this runs the replacement as the panel
    // would hand it the search's files.)
    use std::{os::unix::fs::PermissionsExt as _, rc::Rc, sync::Arc};

    use fe_project::{Replacer, SearchQuery};

    use super::{Boundary, Outcome, Target};

    let parent = tempfile::tempdir().unwrap();
    let locked = parent.path().join("locked");
    let root = locked.join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.rs"), "foo a\n").unwrap();
    fs::write(root.join("b.rs"), "foo b\n").unwrap();
    let (workbench, _, cx) = searching(cx, &root, "foo");
    let editor = open(&workbench, &root, "a.rs", cx);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o111)).unwrap();
    let listable = fs::read_dir(&locked).is_ok();

    let boundary = Boundary::of(&root).unwrap();
    let targets = ["a.rs", "b.rs"].map(|name| Target {
        path: root.join(name),
        rel: name.into(),
    });
    let replacer = Arc::new(Replacer::new(&SearchQuery::literal("foo"), "bar").unwrap());
    let outcome: Rc<RefCell<Option<Outcome>>> = Rc::default();
    cx.update(|_, cx| {
        let outcome = outcome.clone();
        cx.spawn(async move |cx| {
            let replaced = super::replace(boundary, targets.to_vec(), replacer, cx).await;
            *outcome.borrow_mut() = Some(replaced);
        })
        .detach()
    });
    draw(cx);
    let outcome = outcome.borrow_mut().take().expect("done");
    let (a, b) = (text(&editor, cx), read(&root, "b.rs"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        !listable || unsafe { libc::geteuid() } == 0,
        "only searchable"
    );
    assert_eq!(outcome.message().0, "Replaced 2 occurrences in 2 files");
    assert_eq!(a, "bar a\n");
    assert_eq!(b, "bar b\n");
}
