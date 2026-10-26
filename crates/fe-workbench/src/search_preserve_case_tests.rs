//! The search panel's Preserve Case: `search.toggle_preserve_case` and the
//! `AB` toggle, which Replace All across files follows, open or not.

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_project::SearchOptions;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{SearchPanel, tests::open_root};

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

/// Focuses the query, replaces it with `query` and runs it.
fn search_for(search: &Entity<SearchPanel>, query: &str, cx: &mut VisualTestContext) {
    let input = search.read_with(cx, |search, _| search.input().clone());
    cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    draw(cx);
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input(query);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn replace_with(search: &Entity<SearchPanel>, text: &str, cx: &mut VisualTestContext) {
    let input = search.read_with(cx, |search, _| search.replace_input().clone());
    input.update(cx, |input, cx| input.set_text(text, cx));
    draw(cx);
}

/// Replace All, answering its question, which it returns.
fn replace_all(cx: &mut VisualTestContext) -> Option<String> {
    run("search.replace_all", cx);
    let question = cx.pending_prompt().map(|(question, _)| question);
    cx.simulate_prompt_answer("Replace");
    draw(cx);
    question
}

fn preserving(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> bool {
    search.read_with(cx, |search, _| search.preserve_case)
}

fn status(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Option<String> {
    search.read_with(cx, |search, _| search.status_text())
}

fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap()
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

#[gpui::test]
fn project_replace(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "foo Foo\n").unwrap();
    fs::write(dir.path().join("b.rs"), "FOO foo_bar Foo_Bar\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    // a.rs open, b.rs not.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.path().join("a.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a.rs shows");
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    search.update(cx, |search, _| {
        search.set_options(SearchOptions {
            threads: 1,
            ..SearchOptions::default()
        })
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-f");

    search_for(&search, "foo", cx);
    run("search.show_replace", cx);
    replace_with(&search, "bar", cx);

    // The toggle, beside the replace field, turns it on.
    assert!(!preserving(&search, cx));
    let toggle = cx.debug_bounds("search-preserve-case").expect("the toggle");
    cx.simulate_click(toggle.center(), Modifiers::none());
    draw(cx);
    assert!(preserving(&search, cx));

    // The question counts as before; each file's matches take their case.
    assert_eq!(
        replace_all(cx).as_deref(),
        Some("Replace 5 occurrences across 2 files with “bar”?")
    );
    assert_eq!(text(&editor, cx), "bar Bar\n");
    assert_eq!(read(dir.path(), "b.rs"), "BAR bar_bar Bar_Bar\n");
    assert_eq!(
        status(&search, cx).as_deref(),
        Some("Replaced 5 occurrences in 2 files")
    );

    // With a regex, the groups expand first. The search reads the files on
    // disk, where a.rs's replacement isn't saved.
    search_for(&search, "b(a)(r)", cx);
    run("search.toggle_regex", cx);
    run("search.show_replace", cx);
    replace_with(&search, "$2$1x", cx);
    assert_eq!(
        replace_all(cx).as_deref(),
        Some("Replace 5 occurrences across 1 file with “$2$1x”?")
    );
    assert_eq!(read(dir.path(), "b.rs"), "RAX rax_rax Rax_Rax\n");

    // The key turns it off: as written.
    run("search.toggle_preserve_case", cx);
    assert!(!preserving(&search, cx));
    run("search.toggle_regex", cx);
    search_for(&search, "rax", cx);
    run("search.show_replace", cx);
    replace_with(&search, "qux", cx);
    replace_all(cx);
    assert_eq!(text(&editor, cx), "bar Bar\n");
    assert_eq!(read(dir.path(), "b.rs"), "qux qux_qux qux_qux\n");
}
