use std::sync::Arc;

use fe_command::{Invocation, Value};
use fe_text::Text;
use proptest::prelude::*;

use crate::{
    EditorModel, Movement,
    commands::{self, EditorTarget, HeadlessHost},
    tests::{parse, render},
    viewer::{Outcome, ViewerModel},
};

const NEXT: &str = "select_next_occurrence";

fn run(model: &mut EditorModel, name: &str) {
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of(&format!("editor.{name}"))
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap();
}

/// `marked` with its `|`s out, and their offsets there.
fn stops(marked: &str) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut stops = Vec::new();
    for ch in marked.chars() {
        if ch == '|' {
            stops.push(text.len());
        } else {
            text.push(ch);
        }
    }
    (text, stops)
}

/// Walks a caret from the start by `movement` to the end, or back: it stops at
/// each `|`, in order.
fn walk(marked: &str, movement: Movement) {
    let (text, expected) = stops(marked);
    let forward = movement == Movement::SubwordRight;
    let mut model = EditorModel::new(fe_buffer::Buffer::new(&text), None);
    let (from, to) = if forward {
        (0, text.len())
    } else {
        (text.len(), 0)
    };
    model.set_cursor(from);
    let mut seen = Vec::new();
    while model.primary_selection().range.start != to {
        model.move_cursors(movement, false);
        seen.push(model.primary_selection().range.start);
        assert!(seen.len() <= text.len(), "stuck at {seen:?} in {marked:?}");
    }
    if !forward {
        seen.reverse();
    }
    assert_eq!(seen, expected, "{marked:?}");
}

fn right(marked: &str) {
    walk(marked, Movement::SubwordRight);
}

/// As [`right`], with the marks read back from the end.
fn left(marked: &str) {
    walk(marked, Movement::SubwordLeft);
}

fn moved(marked: &str, movement: Movement) -> String {
    let mut model = parse(marked);
    model.move_cursors(movement, false);
    render(&model)
}

#[test]
fn moves_stop_at_each_part() {
    // camelCase and PascalCase.
    right("foo|Bar|Baz|");
    left("|foo|Bar|Baz");
    right("Foo|Bar|");
    left("|Foo|Bar");
    right("a|B|");
    left("|a|B");
    // A run of capitals gives its last one to the next part.
    right("HTTP|Server|");
    left("|HTTP|Server");
    right("parse|HTTP|Server|Name|");
    left("|parse|HTTP|Server|Name");
    right("A|Bc|");
    left("|A|Bc");
    right("HTT|Ps|");
    left("|HTT|Ps");
    right("HTTP|");
    left("|HTTP");
    // Letters and digits.
    right("utf|8|Decode|");
    left("|utf|8|Decode");
    right("abc|123|def|");
    left("|abc|123|def");
    right("HTTP|2|Server|");
    left("|HTTP|2|Server");
    right("123|");
    left("|123");
    right("x|1|y|2|");
    left("|x|1|y|2");
    // `_` is skipped on the way to a part.
    right("snake|_case|_name|");
    left("|snake_|case_|name");
    right("SCREAMING|_CASE|");
    left("|SCREAMING_|CASE");
    right("_private|");
    left("|_|private");
    right("__dunder|__|");
    left("|__|dunder__");
    right("a|__b|");
    left("|a__|b");
    right("foo|_Bar|Baz|");
    left("|foo_|Bar|Baz");
    right("___|");
    left("|___");
    // Unicode case, and letters with none, which split from neither case.
    right("éa|Éb|");
    left("|éa|Éb");
    right("ÉCOLE|Étude|");
    left("|ÉCOLE|Étude");
    right("日本語Foo|Bar|");
    left("|日本語Foo|Bar");
    right("Foo日本語|");
    left("|Foo日本語");
    right("日本語|8|");
    left("|日本語|8");
    // Punctuation is a run of its own, as in a word move.
    right("foo|.|bar|Baz|(|qux|)|");
    left("|foo|.|bar|Baz|(|qux|)");
    right("a|::|b|");
    left("|a|::|b");
    // So is whitespace, which goes with what follows it.
    right("foo|  bar|Baz|");
    left("|foo  |bar|Baz");
    right("a|  --| b|");
    left("|a  |-- |b");
    right("foo| _bar|");
    left("|foo |_|bar");
    right("foo|_|  |");
    left("|foo_  ");
    // The row's ends: a move steps over the break, and only that.
    right("foo|Bar|\n|baz|Qux|");
    left("|foo|Bar|\n|baz|Qux");
    right("a|  |\n|b|");
    left("|a  |\n|b");
    right("\n|\n|");
    left("|\n|\n");

    // From inside a part: the stops are the text's, wherever the caret starts.
    let (r, l) = (Movement::SubwordRight, Movement::SubwordLeft);
    assert_eq!(moved("fo|oBar", r), "foo|Bar");
    assert_eq!(moved("HT|TPServer", r), "HTTP|Server");
    assert_eq!(moved("HTTP|Server", r), "HTTPServer|");
    assert_eq!(moved("fooB|ar", l), "foo|Bar");
    assert_eq!(moved("HTTPS|erver", l), "HTTP|Server");
    assert_eq!(moved("HTTP|Server", l), "|HTTPServer");
    assert_eq!(moved("snake|_case", r), "snake_case|");
    assert_eq!(moved("snake_|case", r), "snake_case|");
    assert_eq!(moved("snake_|case", l), "|snake_case");
    assert_eq!(moved("snake|_case", l), "|snake_case");
    // The document's ends.
    assert_eq!(moved("|", r), "|");
    assert_eq!(moved("|", l), "|");
    assert_eq!(moved("foo|", r), "foo|");
    assert_eq!(moved("|foo", l), "|foo");
    // A selection collapses to where its head moves, as for a word move.
    assert_eq!(moved("[foo]BarBaz", r), "fooBar|Baz");
    // Every caret moves.
    assert_eq!(moved("|fooBar\n|bazQux", r), "foo|Bar\nbaz|Qux");
}

/// As for a word move: right goes past folded rows, left back to their header.
#[test]
fn a_move_into_a_fold_leaves_it_on_the_side_it_was_going() {
    let mut model = parse("a:|\n  b\nc");
    model.fold(&|_| None);
    model.move_cursors(Movement::SubwordRight, false);
    assert_eq!(render(&model), "a:\n  b\n|c");
    model.move_cursors(Movement::SubwordLeft, false);
    assert_eq!(render(&model), "a:|\n  b\nc");
}

/// Runs `name` on `marked`: it gives `expected`, as one undo step.
fn one_step(marked: &str, name: &str, expected: &str) {
    let mut model = parse(marked);
    run(&mut model, name);
    assert_eq!(render(&model), expected, "{name} on {marked:?}");
    model.undo().unwrap();
    assert_eq!(
        model.text().to_string(),
        parse(marked).text().to_string(),
        "undoing {name}"
    );
    assert_eq!(model.undo(), Ok(None), "{name} on {marked:?} is one step");
}

#[test]
fn select_and_delete_variants_follow_the_moves() {
    // Selecting extends, keeping the tail.
    let mut model = parse("|fooBarBaz");
    run(&mut model, "select_subword_right");
    assert_eq!(render(&model), "[foo]BarBaz");
    run(&mut model, "select_subword_right");
    assert_eq!(render(&model), "[fooBar]Baz");
    run(&mut model, "select_subword_left");
    assert_eq!(render(&model), "[foo]BarBaz");
    let mut model = parse("fooBarBaz|");
    run(&mut model, "select_subword_left");
    assert_eq!(render(&model), "fooBar[Baz]");
    run(&mut model, "select_subword_left");
    assert_eq!(render(&model), "foo[BarBaz]");
    let mut model = parse("|fooBar\n|bazQux");
    run(&mut model, "select_subword_right");
    assert_eq!(render(&model), "[foo]Bar\n[baz]Qux");

    let (l, r) = ("delete_subword_left", "delete_subword_right");
    // Each part.
    one_step("fooBar|Baz", l, "foo|Baz");
    one_step("foo|BarBaz", r, "foo|Baz");
    one_step("HTTPServer|", l, "HTTP|");
    one_step("|HTTPServer", r, "|Server");
    one_step("utf8|Decode", l, "utf|Decode");
    one_step("utf|8Decode", r, "utf|Decode");
    // `_` goes with the part it was skipped on the way to.
    one_step("snake_|case", l, "|case");
    one_step("snake|_case", r, "snake|");
    one_step("snake_case|", l, "snake_|");
    one_step("|snake_case", r, "|_case");
    // Whitespace and punctuation.
    one_step("foo  |bar", l, "|bar");
    one_step("foo|  bar", r, "foo|");
    one_step("foo.|bar", l, "foo|bar");
    // At a row's edge the row break goes.
    one_step("fooBar\n|baz", l, "fooBar|baz");
    one_step("fooBar|\nbaz", r, "fooBar|baz");
    // Nothing at the document's.
    let mut at_start = parse("|foo");
    run(&mut at_start, l);
    assert_eq!(render(&at_start), "|foo");
    assert_eq!(at_start.undo(), Ok(None));
    let mut at_end = parse("foo|");
    run(&mut at_end, r);
    assert_eq!(render(&at_end), "foo|");
    assert_eq!(at_end.undo(), Ok(None));
    // Several carets, and carets whose ranges touch.
    one_step("fooBar| bazQux|", l, "foo| baz|");
    one_step("|fooBar |bazQux", r, "|Bar |Qux");
    one_step("fooBar|Baz|", l, "foo|");
    // A selection goes whole, and a caret beside it goes by its part.
    one_step("a[bC]d", l, "a|d");
    one_step("a[bC]d", r, "a|d");
    one_step("foo[Bar] baz|Qux", l, "foo| |Qux");

    // Exactly what the move goes over.
    let marked = ["fooBar_baz9 HTTPServer", "  x.yZ\nq_R"].join("\n");
    let (text, _) = stops(&marked);
    for offset in 0..=text.len() {
        let model = |at: usize| {
            let mut model = EditorModel::new(fe_buffer::Buffer::new(&text), None);
            model.set_cursor(at);
            model
        };
        let mut moved = model(offset);
        moved.move_cursors(Movement::SubwordLeft, false);
        let to = moved.primary_selection().range.start;
        let mut deleted = model(offset);
        deleted.delete_subword_left();
        assert_eq!(
            deleted.text().to_string(),
            format!("{}{}", &text[..to], &text[offset..]),
            "left from {offset}"
        );
        let mut moved = model(offset);
        moved.move_cursors(Movement::SubwordRight, false);
        let to = moved.primary_selection().range.start;
        let mut deleted = model(offset);
        deleted.delete_subword_right();
        assert_eq!(
            deleted.text().to_string(),
            format!("{}{}", &text[..offset], &text[to..]),
            "right from {offset}"
        );
    }

    // A move can stop inside a cluster, at its combining mark; a delete takes
    // the whole of it, on both sides.
    assert_eq!(
        moved("|cafe\u{301}Bar", Movement::SubwordRight),
        "cafe|\u{301}Bar"
    );
    one_step("|e\u{301}x", r, "|x");
    one_step("e\u{301}|x", l, "|x");
    one_step("cafe|\u{301}Bar", r, "caf|Bar");
    one_step("cafe|\u{301}Bar", l, "|Bar");
    one_step("caf|e\u{301}Bar", r, "caf|Bar");
    one_step("👍🏽|fooBar", l, "|fooBar");
    one_step("👍|🏽foo", r, "|foo");

    // So does a selection that ends inside one, as selecting by subword can.
    for name in [l, r] {
        let mut model = parse("|e\u{301}x");
        run(&mut model, "select_subword_right");
        assert_eq!(render(&model), "[e]\u{301}x");
        run(&mut model, name);
        assert_eq!(render(&model), "|x", "{name}");
        model.undo().unwrap();
        assert_eq!(model.text().to_string(), "e\u{301}x", "undoing {name}");
        assert_eq!(model.undo(), Ok(None), "{name} is one step");

        let mut model = parse("e\u{301}|x");
        run(&mut model, "select_subword_left");
        assert_eq!(render(&model), "e[\u{301}]x");
        run(&mut model, name);
        assert_eq!(render(&model), "|x", "{name}");

        one_step("a[be]\u{301}x [e\u{301}]y", name, "a|x |y");
        one_step("[e][\u{301}]x", name, "|x");
        // A selection of whole clusters takes no more than itself.
        one_step("a[e\u{301}]x", name, "a|x");
    }
}

#[test]
fn deleting_a_subword_in_a_linked_range_mirrors_and_keeps_the_primary() {
    let mut model = parse("<fooBar|></fooBar>");
    model.set_linked_ranges(vec![1..7, 10..16]);
    model.delete_subword_left();
    assert_eq!(model.text().to_string(), "<foo></foo>");
    assert_eq!(render(&model), "<foo|></foo>");
    assert_eq!(model.linked_ranges(), Some(&[1..4, 7..10][..]));

    // From the second range, rightwards.
    let mut model = parse("<fooBar></|fooBar>");
    model.set_linked_ranges(vec![1..7, 10..16]);
    model.delete_subword_right();
    assert_eq!(model.text().to_string(), "<Bar></Bar>");
    assert_eq!(render(&model), "<Bar></|Bar>");
    assert_eq!(model.linked_ranges(), Some(&[1..4, 7..10][..]));

    // The selection cmd-d last added stays primary, though it's not the
    // bottommost.
    let mut model = parse("aa b [aa]");
    run(&mut model, NEXT);
    assert_eq!(model.primary_selection().range, 0..2);
    model.insert("xY");
    assert_eq!(render(&model), "xY| b xY|");
    assert_eq!(model.primary_selection().range, 2..2);
    model.delete_subword_left();
    assert_eq!(render(&model), "x| b x|");
    assert_eq!(model.primary_selection().range, 1..1);

    // Rightwards too.
    let mut model = parse("aaYz b [aa]Yz");
    run(&mut model, NEXT);
    assert_eq!(model.primary_selection().range, 0..2);
    model.insert("x");
    assert_eq!(render(&model), "x|Yz b x|Yz");
    assert_eq!(model.primary_selection().range, 1..1);
    model.delete_subword_right();
    assert_eq!(render(&model), "x| b x|");
    assert_eq!(model.primary_selection().range, 1..1);
}

#[test]
fn the_commands_have_their_titles_and_edit_only_when_they_delete() {
    let registry = commands::registry();
    for (id, title, mutates) in [
        ("editor.move_subword_left", "Move Subword Left", false),
        ("editor.move_subword_right", "Move Subword Right", false),
        ("editor.select_subword_left", "Select Subword Left", false),
        ("editor.select_subword_right", "Select Subword Right", false),
        ("editor.delete_subword_left", "Delete Subword Left", true),
        ("editor.delete_subword_right", "Delete Subword Right", true),
    ] {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        let meta = meta.unwrap_or_else(|| panic!("{id}"));
        assert_eq!(meta.title, title, "{id}");
        assert_eq!(meta.mutates_buffer, mutates, "{id}");
    }
}

#[test]
fn the_editor_move_command_takes_the_motions() {
    let mut model = parse("|fooBarBaz");
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of("editor.move")
        .unwrap()
        .args(serde_json::json!({ "motion": "subword_right", "count": 2 }))
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
        .unwrap();
    assert_eq!(render(&model), "fooBar|Baz");
}

/// A read-only viewer has no parts: it moves by whole words, and won't delete.
#[test]
fn a_viewer_moves_by_words_and_refuses_the_deletes() {
    let viewer = || ViewerModel::new(Arc::new(Text::new("fooBar bazQux")), 4);
    let mut model = viewer();
    assert_eq!(
        model.run("editor.move_subword_right", &Value::Null),
        Outcome::Changed
    );
    assert_eq!(model.selection(), 6..6);
    assert_eq!(
        model.run("editor.select_subword_right", &Value::Null),
        Outcome::Changed
    );
    assert_eq!(model.selection(), 6..13);
    let mut model = viewer();
    model.set_selection(13, 13);
    assert_eq!(
        model.run("editor.move_subword_left", &Value::Null),
        Outcome::Changed
    );
    assert_eq!(model.selection(), 7..7);
    assert_eq!(
        model.run("editor.select_subword_left", &Value::Null),
        Outcome::Changed
    );
    assert_eq!(model.selection(), 0..7);
    let mut model = viewer();
    model.set_selection(13, 13);
    let motion = serde_json::json!({ "motion": "subword_left" });
    assert_eq!(model.run("editor.move", &motion), Outcome::Changed);
    assert_eq!(model.selection(), 7..7);
    for id in ["editor.delete_subword_left", "editor.delete_subword_right"] {
        assert_eq!(model.run(id, &Value::Null), Outcome::ReadOnly, "{id}");
    }
}

proptest! {
    /// Each move gets somewhere, on a char boundary, within a step of its
    /// row, and never past what a word move reaches.
    #[test]
    fn a_move_always_gets_somewhere_within_the_word_move(
        text in "([a-zA-Z0-9_ .é日\u{301}\n]){0,40}",
        at in 0usize..60,
    ) {
        let mut at = at.min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        let model = EditorModel::new(fe_buffer::Buffer::new(&text), None);
        let right = model.next_subword(at);
        prop_assert!(text.is_char_boundary(right));
        prop_assert!(right > at || at == text.len());
        prop_assert!(right <= model.next_word(at));
        let left = model.prev_subword(at);
        prop_assert!(text.is_char_boundary(left));
        prop_assert!(left < at || at == 0);
        prop_assert!(left >= model.prev_word(at));
    }
}
