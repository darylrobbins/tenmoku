//! A snippet's session: inserted with its first stop selected, then
//! `editor.tab` and `editor.outdent` (shift-tab) walk its stops, typing
//! fills a stop's mirrors together, and leaving the stops ends it.

// The selections are compared as ranges, often one.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::{
    EditorModel, IndentStyle,
    commands::{self, EditorTarget, HeadlessHost},
    completion::{CompletionItem, CompletionList, CompletionSession},
    snippet::Snippet,
};
use serde_json::json;

fn model(text: &str) -> EditorModel {
    let mut editor = EditorModel::new(Buffer::new(text), None);
    editor.set_indent_style(Some(IndentStyle::Tabs));
    editor
}

fn run(editor: &mut EditorModel, id: &str) {
    run_with(editor, id, serde_json::Value::Null);
}

fn run_with(editor: &mut EditorModel, id: &str, args: serde_json::Value) {
    let invocation = Invocation::of(id)
        .unwrap()
        .args(args)
        .context(commands::context());
    commands::registry()
        .dispatch(
            &mut EditorTarget::new(editor, &mut HeadlessHost::new()),
            &invocation,
        )
        .unwrap();
}

fn tab(editor: &mut EditorModel) {
    run(editor, "editor.tab");
}

fn shift_tab(editor: &mut EditorModel) {
    run(editor, "editor.outdent");
}

/// Types `text` a char at a time, as keys do.
fn type_text(editor: &mut EditorModel, text: &str) {
    for ch in text.chars() {
        run_with(editor, "editor.insert", json!({ "text": ch.to_string() }));
    }
}

fn ranges(editor: &EditorModel) -> Vec<Range<usize>> {
    editor
        .selections()
        .into_iter()
        .map(|selection| selection.range)
        .collect()
}

fn text(editor: &EditorModel) -> String {
    editor.text().to_string()
}

#[test]
fn tab_and_shift_tab_walk_the_tabstops() {
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("fn ${1:name}(${2:arg}) {$0}"));
    assert_eq!(text(&editor), "fn name(arg) {}");
    assert_eq!(ranges(&editor), [3..7]);
    assert!(editor.in_snippet());

    type_text(&mut editor, "foo");
    assert_eq!(text(&editor), "fn foo(arg) {}");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [7..10]);

    // Back to the first, with what was typed there selected; at the first,
    // it stays.
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [3..6]);
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [3..6]);
    assert_eq!(text(&editor), "fn foo(arg) {}");

    // Typing over the second, then on to `$0`, which ends it.
    tab(&mut editor);
    type_text(&mut editor, "x: u8");
    assert_eq!(text(&editor), "fn foo(x: u8) {}");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [15..15]);
    assert!(!editor.in_snippet());

    tab(&mut editor);
    assert_eq!(text(&editor), "fn foo(x: u8) {\t}");
    assert_eq!(ranges(&editor), [16..16]);

    // Without `$0`, the last tab goes to the snippet's end.
    let mut editor = model("()");
    editor.insert_snippet(1..1, &Snippet::parse("${1:a}, ${2:b}"));
    assert_eq!(ranges(&editor), [1..2]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [4..5]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [5..5]);
    assert!(!editor.in_snippet());

    // A snippet with only `$0` puts the caret there, with no session.
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("[$0]"));
    assert_eq!(ranges(&editor), [1..1]);
    assert!(!editor.in_snippet());
}

#[test]
fn mirrors_are_typed_together() {
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a} = $1;$0"));
    assert_eq!(text(&editor), "a = a;");
    assert_eq!(ranges(&editor), [0..1, 4..5]);

    type_text(&mut editor, "xy");
    assert_eq!(text(&editor), "xy = xy;");
    assert_eq!(ranges(&editor), [2..2, 7..7]);
    run(&mut editor, "editor.backspace");
    type_text(&mut editor, "z");
    assert_eq!(text(&editor), "xz = xz;");

    // Back on the stop, both are selected, grown with the typing.
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [0..2, 5..7]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [8..8]);
}

#[test]
fn the_snippet_is_indented_as_its_row() {
    let mut editor = EditorModel::new(Buffer::new("fn f() {\n    \n}"), None);
    editor.set_indent_style(Some(IndentStyle::Spaces(4)));
    editor.insert_snippet(13..13, &Snippet::parse("if ${1:cond} {\n\t$0\n}"));
    assert_eq!(text(&editor), "fn f() {\n    if cond {\n        \n    }\n}");
    assert_eq!(ranges(&editor), [16..20]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [31..31]);
}

#[test]
fn leaving_the_stops_ends_the_session() {
    // A click elsewhere.
    let mut editor = model("x\n");
    editor.insert_snippet(2..2, &Snippet::parse("f(${1:a}, ${2:b})"));
    assert!(editor.in_snippet());
    editor.set_cursor(0);
    assert!(!editor.in_snippet());
    tab(&mut editor);
    assert_eq!(text(&editor), "\tx\nf(a, b)");

    // A caret moved inside the stop keeps it; one moved out ends it.
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("f(${1:ab}, ${2:c})"));
    run(&mut editor, "editor.move_right");
    run(&mut editor, "editor.move_left");
    assert!(editor.in_snippet());
    run(&mut editor, "editor.move_left");
    run(&mut editor, "editor.move_left");
    assert!(!editor.in_snippet());

    // An edit outside the stop.
    let mut editor = model("x");
    editor.insert_snippet(1..1, &Snippet::parse(" ${1:a} ${2:b}"));
    editor
        .buffer_mut()
        .edit([(0..0, "y")], fe_buffer::Origin::User);
    editor.sync_display();
    assert!(!editor.in_snippet());

    // Escape.
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a} $1 ${2:b}"));
    run(&mut editor, "editor.single_selection");
    assert!(!editor.in_snippet());
    tab(&mut editor);
    // The primary selection, the mirror, stays, and tab replaces it.
    assert_eq!(text(&editor), "a \t b");
}

#[test]
fn accepting_a_snippet_completion_starts_a_session() {
    let mut editor = model("    let v = ve\n");
    editor.set_cursor(14);
    let item = CompletionItem {
        label: "vec!".into(),
        edit: (12..14, "vec![\n\t${1:elem}\n]$0".into()),
        snippet: true,
        ..CompletionItem::default()
    };
    let list = CompletionList {
        items: vec![item],
        is_incomplete: false,
    };
    let session = CompletionSession::new(editor.snapshot().clone(), 14, list);
    // Typed since the request: the accept replaces it too.
    type_text(&mut editor, "c");
    session.accept(&mut editor).unwrap().expect("accepted");
    assert_eq!(text(&editor), "    let v = vec![\n    \telem\n    ]\n");
    assert_eq!(ranges(&editor), [23..27]);
    assert!(editor.in_snippet());
    tab(&mut editor);
    assert_eq!(ranges(&editor), [33..33]);

    run(&mut editor, "editor.undo");
    assert_eq!(text(&editor), "    let v = vec\n");
    assert!(!editor.in_snippet());

    // A plain item goes in as it is.
    let mut editor = model("ve");
    editor.set_cursor(2);
    let item = CompletionItem {
        label: "vec!".into(),
        edit: (0..2, "vec![$1]".into()),
        ..CompletionItem::default()
    };
    let list = CompletionList {
        items: vec![item],
        is_incomplete: false,
    };
    let session = CompletionSession::new(editor.snapshot().clone(), 2, list);
    session.accept(&mut editor).unwrap();
    assert_eq!(text(&editor), "vec![$1]");
    assert!(!editor.in_snippet());
    tab(&mut editor);
    assert_eq!(text(&editor), "vec![$1]\t");
}

#[test]
fn line_breaks_are_inserted_as_the_buffer_keeps_them() {
    let mut editor = model("  ");
    editor.insert_snippet(2..2, &Snippet::parse("a\r\n${1:b}\rc$0"));
    assert_eq!(text(&editor), "  a\n  b\n  c");
    assert_eq!(ranges(&editor), [6..7]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [11..11]);

    let mut editor = model("");
    let item = CompletionItem {
        label: "a".into(),
        edit: (0..0, "a\r\n${1:b}$0".into()),
        snippet: true,
        ..CompletionItem::default()
    };
    let list = CompletionList {
        items: vec![item],
        is_incomplete: false,
    };
    let session = CompletionSession::new(editor.snapshot().clone(), 0, list);
    session.accept(&mut editor).unwrap();
    assert_eq!(text(&editor), "a\nb");
    assert_eq!(ranges(&editor), [2..3]);
}

#[test]
fn adjacent_and_coincident_mirrors_stay_apart() {
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a}$1$0"));
    assert_eq!(ranges(&editor), [0..1, 1..2]);
    type_text(&mut editor, "xy");
    assert_eq!(text(&editor), "xyxy");
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [0..2, 2..4]);

    let mut editor = model("()");
    editor.insert_snippet(1..1, &Snippet::parse("$1$1$0"));
    type_text(&mut editor, "xy");
    assert_eq!(text(&editor), "(xyxy)");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [5..5]);
}

#[test]
fn typing_in_one_mirror_fills_the_others() {
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a} = $1;$0"));
    editor.set_cursor(1);
    assert!(editor.in_snippet());
    type_text(&mut editor, "bc");
    assert_eq!(text(&editor), "abc = abc;");
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [0..3, 6..9]);

    // In the mirror, and undone with what typing there did.
    editor.set_cursor(9);
    run(&mut editor, "editor.backspace");
    assert_eq!(text(&editor), "ab = ab;");
    run(&mut editor, "editor.undo");
    assert_eq!(text(&editor), "abc = abc;");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [10..10]);
}

#[test]
fn editing_a_nested_stop_keeps_its_placeholder() {
    // Typing over the nested stop resizes the one around it.
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a${2:b}c}$0"));
    assert_eq!(ranges(&editor), [0..3]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [1..2]);
    type_text(&mut editor, "xy");
    shift_tab(&mut editor);
    assert_eq!(text(&editor), "axyc");
    assert_eq!(ranges(&editor), [0..4]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [1..3]);

    // Typing at the start of the one around it moves the nested one.
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:${2:b}c}$0"));
    editor.set_cursor(0);
    type_text(&mut editor, "x");
    assert_eq!(text(&editor), "xbc");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [1..2]);

    // Typing over the one around it drops the nested one.
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a${2:b}c} $0"));
    type_text(&mut editor, "z");
    tab(&mut editor);
    assert_eq!(text(&editor), "z ");
    assert_eq!(ranges(&editor), [2..2]);
    assert!(!editor.in_snippet());
}

#[test]
fn other_stops_dont_grow_with_typing_next_to_them() {
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${2:foo}$1$0"));
    assert_eq!(ranges(&editor), [3..3]);
    type_text(&mut editor, "x");
    assert_eq!(text(&editor), "foox");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [0..3]);

    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("$2${1:a}$3$0"));
    editor.set_cursor(0);
    type_text(&mut editor, "x");
    editor.set_cursor(2);
    type_text(&mut editor, "y");
    assert_eq!(text(&editor), "xay");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [0..0]);
    tab(&mut editor);
    assert_eq!(ranges(&editor), [3..3]);
}

#[test]
fn a_snippet_completion_is_indented_as_its_row_is_when_accepted() {
    let mut editor = model("    let v = ve\n");
    editor.set_cursor(14);
    let item = CompletionItem {
        label: "vec!".into(),
        edit: (12..14, "vec![\n\t${1:x}\n]".into()),
        snippet: true,
        ..CompletionItem::default()
    };
    let list = CompletionList {
        items: vec![item],
        is_incomplete: false,
    };
    let session = CompletionSession::new(editor.snapshot().clone(), 14, list);
    editor
        .buffer_mut()
        .edit([(0..2, "")], fe_buffer::Origin::User);
    editor.set_cursor(12);
    assert!(session.is_live(&editor));
    session.accept(&mut editor).unwrap().expect("accepted");
    assert_eq!(text(&editor), "  let v = vec![\n  \tx\n  ]\n");
    assert_eq!(ranges(&editor), [19..20]);
}

#[test]
fn a_snippet_without_tabstops_puts_the_caret_at_its_end() {
    for (source, inserted) in [("hello", "hello"), ("", ""), ("${A:x}y", "xy")] {
        let mut editor = model("()");
        editor.insert_snippet(1..1, &Snippet::parse(source));
        assert_eq!(text(&editor), format!("({inserted})"));
        let end = 1 + inserted.len();
        assert_eq!(ranges(&editor), [end..end]);
        assert!(!editor.in_snippet());

        let mut editor = model("");
        let item = CompletionItem {
            label: "x".into(),
            edit: (0..0, source.into()),
            snippet: true,
            ..CompletionItem::default()
        };
        let list = CompletionList {
            items: vec![item],
            is_incomplete: false,
        };
        let session = CompletionSession::new(editor.snapshot().clone(), 0, list);
        session.accept(&mut editor).unwrap();
        assert_eq!(text(&editor), inserted);
        assert_eq!(ranges(&editor), [inserted.len()..inserted.len()]);
    }
}

#[test]
fn wrapping_adjacent_mirrors_in_a_pair_wraps_each() {
    let pairs = [('(', ')')];
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a}$1$0"));
    editor.insert_typed('(', &pairs);
    assert_eq!(text(&editor), "(a)(a)");
    assert_eq!(ranges(&editor), [1..2, 4..5]);
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [0..3, 3..6]);
}

#[test]
fn completions_and_row_edits_in_one_mirror_fill_the_others() {
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a} = $1;$0"));
    editor.set_cursor(1);
    let item = CompletionItem {
        label: "abc".into(),
        edit: (0..1, "abc".into()),
        ..CompletionItem::default()
    };
    let list = CompletionList {
        items: vec![item],
        is_incomplete: false,
    };
    let session = CompletionSession::new(editor.snapshot().clone(), 1, list);
    session.accept(&mut editor).unwrap().expect("accepted");
    assert_eq!(text(&editor), "abc = abc;");
    assert!(editor.in_snippet());
    run(&mut editor, "editor.undo");
    assert_eq!(text(&editor), "a = a;");

    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a\nb}\n$1$0"));
    assert_eq!(text(&editor), "a\nb\na\nb");
    editor.set_cursor(2);
    run(&mut editor, "editor.indent");
    assert_eq!(text(&editor), "a\n\tb\na\n\tb");
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [0..4, 5..9]);
}

#[test]
fn closers_typed_in_a_later_mirror_still_close() {
    let pairs = [('(', ')')];
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a} $1$0"));
    editor.set_cursor(3);
    editor.insert_typed('(', &pairs);
    assert_eq!(text(&editor), "a() a()");
    assert_eq!(ranges(&editor), [6..6]);
    editor.insert_typed(')', &pairs);
    assert_eq!(text(&editor), "a() a()");
    assert_eq!(ranges(&editor), [7..7]);

    editor.insert_typed('(', &pairs);
    assert_eq!(text(&editor), "a()() a()()");
    run(&mut editor, "editor.backspace");
    assert_eq!(text(&editor), "a() a()");
}

#[test]
fn editing_a_nested_stop_fills_the_mirrors_around_it() {
    let mut editor = model("");
    editor.insert_snippet(0..0, &Snippet::parse("${1:a${2:b}c} = $1;$0"));
    assert_eq!(text(&editor), "abc = abc;");
    tab(&mut editor);
    assert_eq!(ranges(&editor), [1..2]);
    type_text(&mut editor, "xy");
    assert_eq!(text(&editor), "axyc = axyc;");
    shift_tab(&mut editor);
    assert_eq!(ranges(&editor), [0..4, 7..11]);
    run(&mut editor, "editor.undo");
    assert_eq!(text(&editor), "abc = abc;");
}
