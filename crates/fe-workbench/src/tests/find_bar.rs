//! The pane's find bar: `find.*` commands dispatched to the focused element
//! in the context its key contexts make, as a key would be.

use std::{fs, ops::Range, path::PathBuf};

use fe_buffer::Origin;
use fe_command::{CommandId, ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Pane, Workbench};

/// A workbench rooted at a temporary directory holding `files`, opened in
/// order in its pane, the last shown and focused.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<PathBuf> = files
        .iter()
        .map(|(name, text)| {
            let path = dir.path().join(name);
            fs::write(&path, text).unwrap();
            path
        })
        .collect();
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
    let pane = pane(&workbench, cx);
    for path in paths {
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    (dir, workbench, cx)
}

/// Lets background work (reads, counts) land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn editor(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    pane.read_with(cx, |pane, _| pane.active_editor().cloned())
        .expect("a tab is shown")
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

/// Shows `pane`'s previous tab (`back`) or next one, as a click on it
/// would.
fn cycle(pane: &Entity<Pane>, back: bool, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.cycle(back, window, cx)));
    draw(cx);
}

fn select(editor: &Entity<EditorView>, range: Range<usize>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.select_and_reveal(range, cx));
    draw(cx);
}

fn selection(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    editor.read_with(cx, |editor, _| editor.model().primary_selection().range)
}

fn focus(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

fn editor_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Whether the pane's find bar is open.
fn open(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, _| {
        pane.find_bar().is_some_and(|bar| bar.is_open())
    })
}

fn query(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> String {
    pane.read_with(cx, |pane, cx| {
        pane.find_bar().unwrap().input().read(cx).text().to_owned()
    })
}

fn input_focused(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    let input = pane.read_with(cx, |pane, _| pane.find_bar().unwrap().input().clone());
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// The bar's label, and whether it's an error.
fn label(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Option<(String, bool)> {
    pane.read_with(cx, |pane, cx| {
        let editor = pane.active_editor().map(|editor| editor.read(cx));
        let label = pane.find_bar().unwrap().label(editor);
        label.map(|(text, error)| (text.to_string(), error))
    })
}

fn text_label(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Option<String> {
    let label = label(pane, cx);
    assert!(
        !label.as_ref().is_some_and(|(_, error)| *error),
        "{label:?}"
    );
    label.map(|(text, _)| text)
}

/// How many matches the editor's find has, once counted; `None` with no
/// find.
fn matches(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<usize> {
    editor.read_with(cx, |editor, _| editor.find_count().map(|count| count.total))
}

mod toggle_focus;

#[gpui::test]
fn show_prefill_and_type(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo one\nfoo two\nfoo one\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    assert!(editor_focused(&editor, cx));
    select(&editor, 0..3, cx);

    run("find.show", cx);
    assert!(open(&pane, cx));
    let toolbar = cx.debug_bounds("pane-toolbar").expect("drawn");
    let bar = cx.debug_bounds("pane-find-bar").expect("drawn");
    assert_eq!(bar.top(), toolbar.bottom(), "the bar is under the toolbar");
    let content = pane.read_with(cx, |pane, _| pane.content_bounds());
    assert_eq!(content.top(), bar.bottom(), "the editor is under the bar");
    assert_eq!(query(&pane, cx), "foo");
    assert!(input_focused(&pane, cx));
    assert_eq!(selection(&editor, cx), 0..3);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 3"));

    // Typing finds from the anchor, the selection's start, taking a match
    // there.
    cx.simulate_input(" o");
    draw(cx);
    assert_eq!(query(&pane, cx), "foo o");
    assert_eq!(selection(&editor, cx), 0..5);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));

    // Shown again from the second row: the selection replaces the query,
    // and typing selects the first match after the new anchor.
    focus(&editor, cx);
    select(&editor, 8..11, cx);
    run("find.show", cx);
    assert_eq!(query(&pane, cx), "foo");
    assert!(input_focused(&pane, cx));
    assert_eq!(text_label(&pane, cx).as_deref(), Some("2 of 3"));
    // An edit before the anchor moves it, past a match now at its old
    // offset.
    editor.update(cx, |editor, cx| {
        let edit = [(0..0, "x foo o\n")];
        editor.model_mut().buffer_mut().edit(edit, Origin::User);
        cx.notify();
    });
    assert_eq!(selection(&editor, cx), 16..19);
    draw(cx);
    cx.simulate_input(" o");
    draw(cx);
    assert_eq!(selection(&editor, cx), 24..29);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("3 of 3"));

    cx.simulate_input("x");
    draw(cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("No results"));
    assert_eq!(selection(&editor, cx), 24..29, "the selection stays");

    // A selection moved since is the anchor.
    select(&editor, 2..2, cx);
    run("text_input.backspace", cx);
    assert_eq!(selection(&editor, cx), 2..7);

    // A caret, or a selection over two rows, keeps the query.
    focus(&editor, cx);
    select(&editor, 4..12, cx);
    run("find.show", cx);
    assert_eq!(query(&pane, cx), "foo o");
    select(&editor, 5..5, cx);
    run("find.show", cx);
    assert_eq!(query(&pane, cx), "foo o");
    assert!(input_focused(&pane, cx));
}

#[gpui::test]
fn next_previous_and_toggles(cx: &mut TestAppContext) {
    let text = "foo Foo\nfoobar foo\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", text)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    // No query yet: nothing to step with.
    select(&editor, 2..2, cx);
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 2..2);
    assert!(pane.read_with(cx, |pane, _| pane.find_bar().is_none()));

    select(&editor, 0..3, cx);
    run("find.show", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 4"));

    // From the input, wrapping both ways.
    let mut steps = Vec::new();
    for _ in 0..4 {
        run("find.next", cx);
        steps.push(selection(&editor, cx));
    }
    assert_eq!(steps, [4..7, 8..11, 15..18, 0..3]);
    run("find.previous", cx);
    assert_eq!(selection(&editor, cx), 15..18);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("4 of 4"));
    assert!(input_focused(&pane, cx));

    // From the editor.
    focus(&editor, cx);
    run("find.previous", cx);
    assert_eq!(selection(&editor, cx), 8..11);
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 15..18);
    assert!(editor_focused(&editor, cx));
    // Toggles apply only in the bar.
    run("find.toggle_case", cx);
    assert_eq!(matches(&editor, cx), Some(4));

    // Each toggle finds again from the anchor, the match stepped to last.
    cx.update(|window, cx| {
        let input = pane.read(cx).find_bar().unwrap().input().clone();
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);
    run("find.toggle_case", cx);
    assert_eq!(matches(&editor, cx), Some(3), "Foo is out");
    assert_eq!(selection(&editor, cx), 15..18);
    run("find.toggle_whole_word", cx);
    assert_eq!(matches(&editor, cx), Some(2), "foobar is out");
    assert_eq!(text_label(&pane, cx).as_deref(), Some("2 of 2"));
    run("find.toggle_case", cx);
    assert_eq!(matches(&editor, cx), Some(3), "Foo is back");
    run("find.toggle_case", cx);

    // A regex.
    run("text_input.clear", cx);
    assert_eq!(query(&pane, cx), "");
    assert_eq!(label(&pane, cx), None);
    assert_eq!(matches(&editor, cx), None, "an empty query clears the find");
    cx.simulate_input("fo+");
    draw(cx);
    assert_eq!(matches(&editor, cx), Some(0), "no foo+ in the text");
    assert_eq!(text_label(&pane, cx).as_deref(), Some("No results"));
    run("find.toggle_regex", cx);
    assert_eq!(matches(&editor, cx), Some(2));
    assert_eq!(selection(&editor, cx), 15..18);
    run("text_input.clear", cx);
    cx.simulate_input("(");
    draw(cx);
    let (error, is_error) = label(&pane, cx).expect("an error shows");
    assert!(is_error);
    assert!(error.starts_with("invalid regex: "), "{error}");
    assert_eq!(matches(&editor, cx), None, "the highlight is cleared");
    assert_eq!(selection(&editor, cx), 15..18);
    run("text_input.backspace", cx);
    cx.simulate_input("fo+");
    draw(cx);
    assert_eq!(matches(&editor, cx), Some(2), "foo and foo, matching case");
    run("find.toggle_whole_word", cx);
    assert_eq!(matches(&editor, cx), Some(3), "and foobar's");

    // Closed, the last query steps without highlighting.
    run("find.close", cx);
    assert!(!open(&pane, cx));
    assert_eq!(selection(&editor, cx), 15..18);
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 0..3);
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 8..11);
    run("find.previous", cx);
    assert_eq!(selection(&editor, cx), 0..3);
    assert_eq!(matches(&editor, cx), None);
    assert!(!open(&pane, cx));
    assert!(editor_focused(&editor, cx));

    // With the query emptied, nothing.
    run("find.show", cx);
    run("text_input.clear", cx);
    run("find.close", cx);
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 0..3);
}

#[gpui::test]
fn close_tabs_and_panes(cx: &mut TestAppContext) {
    let files = [("a.txt", "x foo\nfoo\n"), ("b.txt", "foo b foo\n")];
    let (_dir, workbench, cx) = opened(cx, &files);
    run("pane.split_right", cx);
    let panes: Vec<Entity<Pane>> = workbench.read_with(cx, |workbench, _| {
        workbench
            .pane_group()
            .panes()
            .into_iter()
            .cloned()
            .collect()
    });
    assert_eq!(panes.len(), 2);
    let (pane, other) = (&panes[0], &panes[1]);
    let other_editor = editor(other, cx);
    select(&other_editor, 1..2, cx);
    let b = editor(pane, cx);
    focus(&b, cx);
    select(&b, 6..9, cx);

    run("find.show", cx);
    assert!(open(pane, cx));
    assert_eq!(text_label(pane, cx).as_deref(), Some("2 of 2"));
    assert_eq!(matches(&b, cx), Some(2));
    run("find.previous", cx);
    assert_eq!(selection(&b, cx), 0..3);

    // Another tab: its caret stays, and it gets the find.
    let a = pane.read_with(cx, |pane, _| pane.tabs()[0].editor.clone());
    select(&a, 1..1, cx);
    cycle(pane, true, cx);
    assert_eq!(editor(pane, cx), a);
    assert!(open(pane, cx));
    assert_eq!(matches(&b, cx), None, "the old tab's find is cleared");
    assert_eq!(matches(&a, cx), Some(2));
    assert_eq!(selection(&a, cx), 1..1);
    assert_eq!(text_label(pane, cx).as_deref(), Some("? of 2"));
    // The anchor moved with it.
    run("find.next", cx);
    assert_eq!(selection(&a, cx), 2..5);
    assert_eq!(text_label(pane, cx).as_deref(), Some("1 of 2"));

    // The other pane has no find.
    assert!(other.read_with(cx, |other, _| other.find_bar().is_none()));
    assert_eq!(matches(&other_editor, cx), None);
    assert_eq!(selection(&other_editor, cx), 1..2);

    focus(&a, cx);
    run("find.show", cx);
    run("find.close", cx);
    assert!(!open(pane, cx));
    assert_eq!(cx.debug_bounds("pane-find-bar"), None);
    assert_eq!(matches(&a, cx), None, "the find is cleared");
    assert!(editor_focused(&a, cx));
    assert_eq!(selection(&a, cx), 2..5, "the match stays selected");
    cycle(pane, false, cx);
    assert_eq!(matches(&b, cx), None, "a closed bar follows no tab");
    assert_eq!(matches(&other_editor, cx), None);
    assert_eq!(selection(&other_editor, cx), 1..2);
}

#[gpui::test]
fn vim_types_in_the_bar(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "one jd\njd two jd\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    let mode = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.vim_mode());
    assert_eq!(mode(cx), Some("normal"));
    assert_eq!(selection(&editor, cx), 0..0);

    run("find.show", cx);
    assert!(input_focused(&pane, cx));
    assert_eq!(query(&pane, cx), "");
    let vim_keys = cx.update(|window, _| {
        window
            .context_stack()
            .iter()
            .any(|context| context.contains("vim_mode"))
    });
    assert!(!vim_keys, "the bar's context has no vim.mode");
    cx.simulate_keystrokes("j d");
    draw(cx);
    assert_eq!(query(&pane, cx), "jd");
    assert_eq!(mode(cx), Some("normal"), "no operator is pending");
    assert_eq!(
        selection(&editor, cx),
        4..4,
        "the caret is on the match's start"
    );
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 3"));

    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 7..7);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("2 of 3"));
    focus(&editor, cx);
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 14..14);
    assert_eq!(mode(cx), Some("normal"));
}

#[gpui::test]
fn a_capped_count_reads_10000_plus(cx: &mut TestAppContext) {
    let text = "x\n".repeat(10_001);
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", &text)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    select(&editor, 2..3, cx);
    run("find.show", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("2 of 10000+"));
}

#[test]
fn the_commands_are_in_the_catalog() {
    let catalog = crate::command_catalog();
    for id in [
        "find.show",
        "find.next",
        "find.previous",
        "find.close",
        "find.toggle_case",
        "find.toggle_whole_word",
        "find.toggle_regex",
    ] {
        let command = CommandId::parse(id).unwrap();
        assert_eq!(catalog.check(&command, &Value::Null), Ok(()), "{id}");
    }
}
