//! The find bar's replace row: `find.replace_next` and `find.replace_all`
//! dispatched to the focused element as a key would, with the setup of
//! [`crate::tests::find_bar`].

use std::ops::Range;

use fe_buffer::Origin;
use fe_command::{CommandId, Value};
use fe_gpui::{EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Pane,
    tests::find_bar::{
        draw, editor, editor_focused, focus, input_focused, open, opened, pane, query, run, select,
        selection, text_label,
    },
};

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

/// The replace input's text.
fn replacement(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> String {
    pane.read_with(cx, |pane, cx| {
        let bar = pane.find_bar().unwrap();
        bar.replace_input().read(cx).text().to_owned()
    })
}

fn replacing(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, _| pane.find_bar().unwrap().is_replacing())
}

fn replace_focused(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    let input = pane.read_with(cx, |pane, _| {
        pane.find_bar().unwrap().replace_input().clone()
    });
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// Focuses the query input, or with `replace` the replace input, clears
/// it and types `text` there.
fn type_in(pane: &Entity<Pane>, replace: bool, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let bar = pane.read(cx).find_bar().unwrap();
        let input = match replace {
            true => bar.replace_input(),
            false => bar.input(),
        };
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);
    run("text_input.clear", cx);
    cx.simulate_input(text);
    draw(cx);
}

#[gpui::test]
fn replace_next(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo bar foo\nfoo\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    select(&editor, 0..3, cx);

    // Shown with the replace row, the selection seeds the query and the
    // replace input has focus.
    run("find.show_replace", cx);
    assert!(open(&pane, cx));
    assert!(replacing(&pane, cx));
    assert!(replace_focused(&pane, cx));
    assert_eq!(query(&pane, cx), "foo");
    assert!(cx.debug_bounds("find-replace-all").is_some());
    let in_replace_input = cx.update(|window, _| {
        let stack = window.context_stack();
        let has = |key: &str| stack.iter().any(|context| context.contains(key));
        (has("find_bar"), has("replace_input"), has("text_input"))
    });
    assert_eq!(in_replace_input, (true, true, true));
    cx.simulate_input("baz");
    draw(cx);
    assert_eq!(replacement(&pane, cx), "baz");
    assert_eq!(query(&pane, cx), "foo", "typing went to the replace input");
    assert_eq!(selection(&editor, cx), 0..3);

    // The selection is a match: it's replaced, and the next one selected.
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "baz bar foo\nfoo\n");
    assert_eq!(selection(&editor, cx), 8..11);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));
    assert!(replace_focused(&pane, cx));

    // A selection that's no match only selects the next match from it.
    select(&editor, 0..2, cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "baz bar foo\nfoo\n");
    assert_eq!(selection(&editor, cx), 8..11);

    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "baz bar baz\nfoo\n");
    assert_eq!(selection(&editor, cx), 12..15);
    // The last: nothing's left to select, and the caret is after it.
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "baz bar baz\nbaz\n");
    assert_eq!(selection(&editor, cx), 15..15);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("No results"));

    // Each replace was its own undo step.
    focus(&editor, cx);
    run("editor.undo", cx);
    assert_eq!(text(&editor, cx), "baz bar baz\nfoo\n");
}

#[gpui::test]
fn replace_all_one_undo(cx: &mut TestAppContext) {
    let original = "a=1\nbb=22 x=9\nccc=333\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    run("find.show_replace", cx);
    type_in(&pane, false, r"(\w+)=(\d+)", cx);
    run("find.toggle_regex", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 4"));
    type_in(&pane, true, "$2=$1", cx);

    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "1=a\n22=bb 9=x\n333=ccc\n");
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 4"));
    assert_eq!(selection(&editor, cx), 21..21, "after the last");
    assert!(replace_focused(&pane, cx));

    // A change to the query counts again.
    type_in(&pane, false, r"(\d+)=", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 4"));

    // One undo takes every replacement back.
    focus(&editor, cx);
    run("editor.undo", cx);
    assert_eq!(text(&editor, cx), original);
    run("editor.redo", cx);
    assert_eq!(text(&editor, cx), "1=a\n22=bb 9=x\n333=ccc\n");
}

#[gpui::test]
fn toggles(cx: &mut TestAppContext) {
    let original = "Foo foo foobar\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "[$1]", cx);
    run("find.toggle_whole_word", cx);

    // Match case off: Foo and foo; whole word: not foobar; regex off: $1
    // is literal.
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "[$1] [$1] foobar\n");
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 2"));
    focus(&editor, cx);
    run("editor.undo", cx);
    assert_eq!(text(&editor, cx), original);

    // Match case on: foo only, by replace all and by replace.
    type_in(&pane, false, "foo", cx);
    run("find.toggle_case", cx);
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "Foo [$1] foobar\n");
    focus(&editor, cx);
    run("editor.undo", cx);
    type_in(&pane, false, "foo", cx);
    assert_eq!(selection(&editor, cx), 4..7);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "Foo [$1] foobar\n");

    // Whole word off: foobar's too.
    run("find.toggle_whole_word", cx);
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "Foo [$1] [$1]bar\n");
    // Regex on: $1 is group 1.
    focus(&editor, cx);
    run("editor.undo", cx);
    type_in(&pane, false, "fo(o)", cx);
    run("find.toggle_regex", cx);
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "Foo [$1] [o]bar\n");
}

#[gpui::test]
fn vim(cx: &mut TestAppContext) {
    let original = "a1 b1\nc1\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    let mode = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.vim_mode());
    assert_eq!(mode(cx), Some("normal"));

    // A query first, from normal mode and back.
    run("find.show", cx);
    cx.simulate_keystrokes("1");
    draw(cx);
    assert_eq!(query(&pane, cx), "1");
    run("find.close", cx);
    assert!(editor_focused(&editor, cx));
    assert_eq!(mode(cx), Some("normal"));

    run("find.show_replace", cx);
    assert!(replace_focused(&pane, cx));
    assert_eq!(query(&pane, cx), "1");
    let vim_keys = cx.update(|window, _| {
        (window.context_stack().iter()).any(|context| context.contains("vim_mode"))
    });
    assert!(!vim_keys, "the replace input's context has no vim.mode");
    // Keys that would edit in normal mode only type.
    cx.simulate_keystrokes("x d d");
    draw(cx);
    assert_eq!(replacement(&pane, cx), "xdd");
    assert_eq!(text(&editor, cx), original);
    assert_eq!(mode(cx), Some("normal"));

    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "axdd bxdd\ncxdd\n");
    assert_eq!(mode(cx), Some("normal"));
    // After the last replacement, on its last character in normal mode.
    assert_eq!(selection(&editor, cx), 13..13);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 3"));

    run("find.close", cx);
    assert!(!open(&pane, cx));
    assert!(editor_focused(&editor, cx));
    assert!(!input_focused(&pane, cx));
    assert_eq!(mode(cx), Some("normal"));
    cx.simulate_keystrokes("u");
    draw(cx);
    assert_eq!(text(&editor, cx), original);
    assert_eq!(mode(cx), Some("normal"));
}

#[gpui::test]
fn vim_replace_next_takes_the_match_at_the_caret(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "one jd\njd\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    let mode = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.vim_mode());

    run("find.show_replace", cx);
    type_in(&pane, false, "jd", cx);
    assert_eq!(selection(&editor, cx), 4..4, "the caret is on the match");
    type_in(&pane, true, "k", cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "one k\njd\n");
    assert_eq!(selection(&editor, cx), 6..6);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "one k\nk\n");
    // No match is left: the caret is on the replacement's last character.
    assert_eq!(selection(&editor, cx), 6..6);
    assert_eq!(mode(cx), Some("normal"));
}

#[gpui::test]
fn the_chevron_toggles_the_replace_row(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo\n")]);
    let pane = pane(&workbench, cx);
    run("find.show", cx);
    assert!(!replacing(&pane, cx));
    assert_eq!(cx.debug_bounds("find-replace-all"), None);
    let one_row = cx.debug_bounds("pane-find-bar").expect("drawn");

    let chevron = cx.debug_bounds("find-replace-toggle").expect("drawn");
    cx.simulate_click(chevron.center(), gpui::Modifiers::none());
    draw(cx);
    assert!(replacing(&pane, cx));
    assert!(input_focused(&pane, cx), "focus stays in the query");
    let two_rows = cx.debug_bounds("pane-find-bar").expect("drawn");
    assert!(two_rows.size.height > one_row.size.height);
    let content = pane.read_with(cx, |pane, _| pane.content_bounds());
    assert_eq!(content.top(), two_rows.bottom(), "the editor is under it");

    // Hidden from the replace input, focus goes to the query.
    type_in(&pane, true, "x", cx);
    run("find.toggle_replace", cx);
    assert!(!replacing(&pane, cx));
    assert!(input_focused(&pane, cx));
    assert_eq!(cx.debug_bounds("find-replace-all"), None);
    // Shown again, it keeps its text.
    run("find.toggle_replace", cx);
    assert_eq!(replacement(&pane, cx), "x");
}

/// The workbench's two panes after `pane.split_right`, each showing the
/// file, in one buffer.
fn split(
    workbench: &Entity<crate::Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<Pane>, Entity<Pane>) {
    run("pane.split_right", cx);
    let panes: Vec<Entity<Pane>> = workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().into_iter().cloned()).collect()
    });
    let [a, b] = <[_; 2]>::try_from(panes).expect("two panes");
    let (left, right) = (editor(&a, cx), editor(&b, cx));
    let shared =
        cx.update(|_, cx| (left.read(cx).model()).shares_buffer_with(right.read(cx).model()));
    assert!(shared);
    (a, b)
}

fn vim_mode(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    editor.read_with(cx, |editor, _| editor.vim_mode())
}

#[gpui::test]
fn replace_next_catches_up_with_another_view_first(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo foo\n")]);
    let (pane, other) = split(&workbench, cx);
    let (editor, peer) = (editor(&pane, cx), editor(&other, cx));
    focus(&editor, cx);
    select(&editor, 0..3, cx);
    run("find.show_replace", cx);
    type_in(&pane, true, "bar", cx);

    // An edit through the other view, and no frame before the replace:
    // it doesn't notify, which would draw one first.
    peer.update(cx, |peer, _| {
        let edit = [(0..0, "X ")];
        peer.model_mut().buffer_mut().edit(edit, Origin::User);
    });
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "X bar foo\n");
    assert_eq!(selection(&editor, cx), 6..9);
}

#[gpui::test]
fn empty_matches(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "a\nb")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, false, "$", cx);
    assert_eq!(selection(&editor, cx), 1..1);
    type_in(&pane, true, "x", cx);

    // Each replace goes on to the next row's end, not the same one again.
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "ax\nb");
    assert_eq!(selection(&editor, cx), 4..4);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "ax\nbx");
    assert_eq!(selection(&editor, cx), 2..2, "round to the first row");

    // An empty replacement changes nothing, and still goes on.
    type_in(&pane, true, "", cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "ax\nbx");
    assert_eq!(selection(&editor, cx), 5..5);
    run("find.replace_next", cx);
    assert_eq!(selection(&editor, cx), 2..2);

    // `^|b` on `bx`: the regex steps past the `b` after the empty match at
    // the row's start, but once `y` goes in there the `b` is a match of its
    // own, the next in the text as the replace leaves it.
    type_in(&pane, false, "^|b", cx);
    focus(&editor, cx);
    select(&editor, 3..3, cx);
    run("find.show_replace", cx);
    assert_eq!(selection(&editor, cx), 3..3, "the second row's start");
    type_in(&pane, true, "y", cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "ax\nybx");
    assert_eq!(selection(&editor, cx), 4..5, "the b");
}

#[gpui::test]
fn vim_a_replace_keeps_a_typing_session_open(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo foo\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    // Replace All in insert mode: the session stays open, and the
    // replacements are an undo step of their own.
    cx.simulate_keystrokes("i z");
    draw(cx);
    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "bar", cx);
    assert_eq!(vim_mode(&editor, cx), Some("insert"));
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "zbar bar\n");
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 2"));
    assert_eq!(vim_mode(&editor, cx), Some("insert"));
    run("find.close", cx);
    assert_eq!(vim_mode(&editor, cx), Some("insert"));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(vim_mode(&editor, cx), Some("normal"));
    assert_eq!(text(&editor, cx), "zbar bar\n");
    cx.simulate_keystrokes("u");
    draw(cx);
    assert_eq!(text(&editor, cx), "zfoo foo\n");
    cx.simulate_keystrokes("u");
    draw(cx);
    assert_eq!(text(&editor, cx), "foo foo\n");

    // Replace in insert mode, the caret on a match.
    cx.simulate_keystrokes("i z");
    draw(cx);
    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    assert_eq!(selection(&editor, cx), 1..1);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "zbar foo\n");
    assert_eq!(selection(&editor, cx), 5..5);
    assert_eq!(vim_mode(&editor, cx), Some("insert"));
    run("find.close", cx);
    cx.simulate_keystrokes("escape u");
    draw(cx);
    assert_eq!(text(&editor, cx), "zfoo foo\n");
    cx.simulate_keystrokes("u");
    draw(cx);
    assert_eq!(text(&editor, cx), "foo foo\n");

    // Replace mode likewise.
    cx.simulate_keystrokes("0 R q");
    draw(cx);
    assert_eq!(text(&editor, cx), "qoo foo\n");
    run("find.show_replace", cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "qoo bar\n");
    assert_eq!(vim_mode(&editor, cx), Some("replace"));
    run("find.replace_all", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 0"));
    assert_eq!(vim_mode(&editor, cx), Some("replace"));
}

#[gpui::test]
fn vim_a_counted_insert_is_left_to_its_session(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo foo\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    // `2iz`: the replace makes just the replacement, and puts in no copy.
    cx.simulate_keystrokes("2 i z");
    draw(cx);
    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "bar", cx);
    assert_eq!(selection(&editor, cx), 1..1);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "zbar foo\n");
    assert_eq!(selection(&editor, cx), 5..5);
    assert_eq!(vim_mode(&editor, cx), Some("insert"));
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "zbar bar\n");
    assert_eq!(vim_mode(&editor, cx), Some("insert"));

    // The replaces moved the caret, which in vim drops the count, as an
    // arrow key in insert mode does: `Esc` puts no copy in. Each replace is
    // an undo step of its own.
    run("find.close", cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(vim_mode(&editor, cx), Some("normal"));
    assert_eq!(text(&editor, cx), "zbar bar\n");
    for undone in ["zbar foo\n", "zfoo foo\n", "foo foo\n"] {
        cx.simulate_keystrokes("u");
        draw(cx);
        assert_eq!(text(&editor, cx), undone);
    }
}

#[gpui::test]
fn vim_indentation_a_session_made_is_replaced_as_it_is(cx: &mut TestAppContext) {
    let original = "    foo\nfoo\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    // `o` indents the new row; replaced, it's the replacement, the session
    // still open.
    for command in ["find.replace_next", "find.replace_all"] {
        cx.simulate_keystrokes("o");
        draw(cx);
        assert_eq!(text(&editor, cx), "    foo\n    \nfoo\n");
        run("find.show_replace", cx);
        if command == "find.replace_next" {
            run("find.toggle_regex", cx);
            type_in(&pane, false, "^ +$", cx);
            type_in(&pane, true, "-", cx);
        }
        assert_eq!(selection(&editor, cx), 8..8, "{command}");
        run(command, cx);
        assert_eq!(text(&editor, cx), "    foo\n-\nfoo\n", "{command}");
        assert_eq!(vim_mode(&editor, cx), Some("insert"), "{command}");
        run("find.close", cx);
        cx.simulate_keystrokes("escape u u");
        draw(cx);
        assert_eq!(text(&editor, cx), original, "{command}");
        cx.simulate_keystrokes("g g");
        draw(cx);
    }
}

#[gpui::test]
fn vim_insert_in_another_view_stays_its_own_undo_step(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo foo\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let (pane, other) = split(&workbench, cx);
    let (editor, peer) = (editor(&pane, cx), editor(&other, cx));

    // The other view types, its insert session left open.
    focus(&peer, cx);
    cx.simulate_keystrokes("i z");
    draw(cx);
    assert_eq!(text(&editor, cx), "zfoo foo\n");

    focus(&editor, cx);
    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "bar", cx);
    assert_eq!(vim_mode(&peer, cx), Some("insert"));

    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "zbar foo\n");
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "zbar bar\n");

    // Each undo takes one replace back, and leaves the other view's typing.
    run("find.close", cx);
    cx.simulate_keystrokes("u");
    draw(cx);
    assert_eq!(text(&editor, cx), "zbar foo\n");
    cx.simulate_keystrokes("u");
    draw(cx);
    assert_eq!(text(&editor, cx), "zfoo foo\n");
}

#[test]
fn the_replace_commands_are_in_the_catalog() {
    let catalog = crate::command_catalog();
    for id in [
        "find.show_replace",
        "find.toggle_replace",
        "find.replace_next",
        "find.replace_all",
    ] {
        let command = CommandId::parse(id).unwrap();
        assert_eq!(catalog.check(&command, &Value::Null), Ok(()), "{id}");
    }
}

/// The selections `find.replace_next` makes, from where `query` (a regex)
/// selects, replacing with nothing.
fn replace_steps(
    text: &str,
    query: &str,
    steps: usize,
    cx: &mut TestAppContext,
) -> Vec<Range<usize>> {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", text)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, false, query, cx);
    let mut seen = vec![selection(&editor, cx)];
    for _ in 0..steps {
        run("find.replace_next", cx);
        seen.push(selection(&editor, cx));
    }
    seen
}

#[gpui::test]
fn empty_matches_and_an_empty_replacement_go_round(cx: &mut TestAppContext) {
    // `x*` matches nothing everywhere replace all would replace: each
    // replace goes on to the next of those, round to the first.
    let seen = replace_steps("ab\n", "x*", 4, cx);
    assert_eq!(seen, [0..0, 1..1, 3..3, 0..0, 1..1]);
    // Not inside the grapheme e + U+0301.
    let seen = replace_steps("e\u{301}b\n", "x*", 3, cx);
    assert_eq!(seen, [0..0, 3..3, 5..5, 0..0]);
}

#[gpui::test]
fn vim_a_replace_that_replaces_nothing_leaves_the_text(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "    foo\nfoo\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);

    // A counted insert, whose `Esc` would put copies in.
    cx.simulate_keystrokes("2 i z");
    draw(cx);
    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "bar", cx);
    // A caret on no match: Replace only selects the next.
    select(&editor, 0..0, cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "z    foo\nfoo\n");
    assert_eq!(selection(&editor, cx), 5..5);
    assert_eq!(vim_mode(&editor, cx), Some("insert"));
    // No match at all.
    type_in(&pane, false, "nothing", cx);
    run("find.replace_next", cx);
    run("find.replace_all", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 0"));
    assert_eq!(text(&editor, cx), "z    foo\nfoo\n");
    assert_eq!(vim_mode(&editor, cx), Some("insert"));

    // Indentation `o` put in, which `Esc` would take out unused.
    run("find.close", cx);
    cx.simulate_keystrokes("escape u");
    draw(cx);
    assert_eq!(text(&editor, cx), "    foo\nfoo\n");
    cx.simulate_keystrokes("o");
    draw(cx);
    assert_eq!(text(&editor, cx), "    foo\n    \nfoo\n");
    run("find.show_replace", cx);
    run("find.replace_next", cx);
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "    foo\n    \nfoo\n");
    assert_eq!(vim_mode(&editor, cx), Some("insert"));
}

#[gpui::test]
fn replace_next_steps_over_rows(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo\nx\n\nx foo\nx\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    select(&editor, 0..3, cx);
    run("find.show_replace", cx);
    type_in(&pane, true, "bar", cx);
    // Past rows with no match to the next.
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "bar\nx\n\nx foo\nx\n");
    assert_eq!(selection(&editor, cx), 9..12);
    // From a caret on the last row, round to the first match.
    select(&editor, 13..13, cx);
    run("find.replace_next", cx);
    assert_eq!(selection(&editor, cx), 9..12);
    assert_eq!(text(&editor, cx), "bar\nx\n\nx foo\nx\n");
}

#[gpui::test]
fn a_replacement_that_takes_away_the_next_match(cx: &mut TestAppContext) {
    // `bar` isn't a match once `x` is before it: the next match is found
    // in the text as the replace leaves it.
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo bar\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, false, r"foo |\bbar", cx);
    assert_eq!(selection(&editor, cx), 0..4);
    type_in(&pane, true, "x", cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "xbar\n");
    assert_eq!(selection(&editor, cx), 1..1, "no match is left");
}

#[gpui::test]
fn a_replacement_that_makes_the_next_match(cx: &mut TestAppContext) {
    // `b` starts a row once a line break replaces `a`.
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "ab\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, false, "^a|^b", cx);
    assert_eq!(selection(&editor, cx), 0..1);
    type_in(&pane, true, r"\n", cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "\nb\n");
    assert_eq!(selection(&editor, cx), 1..2);
}

#[gpui::test]
fn a_carriage_return_in_the_replacement_is_a_line_break(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "ab\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, false, "a|^y", cx);
    assert_eq!(selection(&editor, cx), 0..1);
    // As pasted: a lone carriage return, which the buffer makes a `\n`,
    // so `y` starts a row and is the next match.
    pane.update(cx, |pane, cx| {
        let input = pane.find_bar().unwrap().replace_input().clone();
        input.update(cx, |input, cx| input.set_text("x\ry", cx));
    });
    draw(cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "x\nyb\n");
    assert_eq!(selection(&editor, cx), 2..3);
}

#[gpui::test]
fn vim_a_replace_at_a_row_s_end_leaves_the_caret_on_a_grapheme(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "x foo\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    type_in(&pane, false, "foo", cx);
    // e and a combining acute accent: one grapheme, three bytes.
    type_in(&pane, true, "e\u{301}", cx);
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "x e\u{301}\n");
    assert_eq!(vim_mode(&editor, cx), Some("normal"));
    assert_eq!(selection(&editor, cx), 2..2, "on the e, not the accent");
}

#[gpui::test]
fn vim_a_replace_that_finds_nothing_leaves_visual_mode(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "one\ntwo\n")]);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    type_in(&pane, false, "nothing", cx);
    run("find.close", cx);

    for command in ["find.replace_next", "find.replace_all"] {
        cx.simulate_keystrokes("v j");
        draw(cx);
        assert_eq!(vim_mode(&editor, cx), Some("visual"));
        run("find.show_replace", cx);
        run(command, cx);
        run("find.close", cx);
        assert_eq!(vim_mode(&editor, cx), Some("normal"), "{command}");
        assert_eq!(text(&editor, cx), "one\ntwo\n");
    }
}

#[gpui::test]
fn replace_all_over_many_empty_matches(cx: &mut TestAppContext) {
    let rows = 100_000;
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", &"a\n".repeat(rows))]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show_replace", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, false, "$", cx);
    type_in(&pane, true, "x", cx);
    run("find.replace_all", cx);
    // Each row's end, the last, empty row's too.
    let replaced = format!("Replaced {}", rows + 1);
    assert_eq!(text_label(&pane, cx), Some(replaced));
    let expected = format!("{}x", "ax\n".repeat(rows));
    assert!(text(&editor, cx) == expected);
}
