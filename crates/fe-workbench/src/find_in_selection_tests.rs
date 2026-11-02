//! The find bar's Find in Selection: `find.toggle_in_selection` and its
//! toggle limit what's found and replaced to the selections captured, with
//! the setup of [`crate::tests::find_bar`].

// A scope of one range is what these mean, not a mistyped `Vec` of offsets.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use fe_buffer::{Origin, ResolvedSelection};
use fe_vim::search::{regex_calls, set_call_limit};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    Pane,
    tests::find_bar::{
        draw, editor as editor_of, focus, label as find_label, opened, pane, run, select,
        selection, text_label,
    },
};

fn text(editor: &Entity<fe_gpui::EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn on(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, _| pane.find_bar().unwrap().in_selection())
}

fn scope(
    editor: &Entity<fe_gpui::EditorView>,
    cx: &mut VisualTestContext,
) -> Option<Vec<Range<usize>>> {
    editor.read_with(cx, |editor, _| editor.find_scope())
}

/// Focuses the query input, or with `replace` the replace input, clears it
/// and types `text` there.
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

/// Sets the selections to `ranges`, the last the primary one.
fn select_all_of(
    editor: &Entity<fe_gpui::EditorView>,
    ranges: &[Range<usize>],
    cx: &mut VisualTestContext,
) {
    editor.update(cx, |editor, cx| {
        let selections = ranges.iter().map(|range| ResolvedSelection {
            range: range.clone(),
            reversed: false,
        });
        editor.model_mut().set_selections(selections);
        cx.notify();
    });
    draw(cx);
}

#[gpui::test]
fn matches_outside_the_scope_are_ignored(cx: &mut TestAppContext) {
    // foo at 0, 8, 16, 24 and 32.
    let original = "foo one\nfoo two\nfoo 333\nfoo 444\nfoo 555\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    // Rows 1 and 2 selected, to row 2's `3`, then the bar opened and the
    // toggle turned on before there's a query: the first match found is
    // the scope's.
    select(&editor, 8..21, cx);
    run("find.show", cx);
    assert!(!on(&pane, cx));
    run("find.toggle_in_selection", cx);
    assert!(on(&pane, cx));
    assert_eq!(scope(&editor, cx), Some(vec![8..21]));
    assert_eq!(selection(&editor, cx), 8..21);
    type_in(&pane, false, "foo", cx);
    assert_eq!(selection(&editor, cx), 8..11);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));

    // Next and previous step inside it, wrapping.
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 16..19);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("2 of 2"));
    run("find.next", cx);
    assert_eq!(selection(&editor, cx), 8..11);
    run("find.previous", cx);
    assert_eq!(selection(&editor, cx), 16..19);
    run("find.previous", cx);
    assert_eq!(selection(&editor, cx), 8..11);

    // A match running out of the scope isn't in it, nor one outside it.
    type_in(&pane, false, "two", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 1"));
    type_in(&pane, false, "333", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("No results"));
    type_in(&pane, false, "444", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("No results"));

    // Off: the whole file again.
    type_in(&pane, false, "foo", cx);
    run("find.toggle_in_selection", cx);
    assert!(!on(&pane, cx));
    assert_eq!(scope(&editor, cx), None);
    assert!(text_label(&pane, cx).is_some_and(|label| label.ends_with("of 5")));
    run("find.close", cx);

    // Several selections, shown with a remembered query, whose first match
    // takes the selection: it's still those that are captured.
    select_all_of(&editor, &[1..11, 24..35], cx);
    run("find.show", cx);
    assert_eq!(selection(&editor, cx), 24..27);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![1..11, 24..35]));
    assert_eq!(text_label(&pane, cx).as_deref(), Some("2 of 3"));
    for want in [32..35, 8..11, 24..27] {
        run("find.next", cx);
        assert_eq!(selection(&editor, cx), want);
    }
    for want in [8..11, 32..35] {
        run("find.previous", cx);
        assert_eq!(selection(&editor, cx), want);
    }

    // Selecting something else before turning it on takes that instead.
    run("find.toggle_in_selection", cx);
    assert!(!on(&pane, cx));
    select(&editor, 0..11, cx);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![0..11]));
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));
}

#[gpui::test]
fn replace_all_stays_in_scope(cx: &mut TestAppContext) {
    let original = "foo Foo\nfoo Foo\nFOO foo\nfoo Foo\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    // Rows 1 and 2, regex and Preserve Case on: the groups expand, the case
    // follows, and rows 0 and 3 stay as they are.
    select(&editor, 8..23, cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    assert!(on(&pane, cx));
    run("find.toggle_regex", cx);
    run("find.toggle_preserve_case", cx);
    type_in(&pane, false, "(f)(o+)", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 4"));
    type_in(&pane, true, "$2$1", cx);
    run("find.replace_all", cx);
    assert_eq!(text(&editor, cx), "foo Foo\noof OoF\nOOF oof\nfoo Foo\n");
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 4"));

    // One undo takes it all back, the scope with it.
    focus(&editor, cx);
    run("editor.undo", cx);
    assert_eq!(text(&editor, cx), original);
    assert_eq!(scope(&editor, cx), Some(vec![8..23]));

    // Replace, one at a time, stays in it too, wrapping inside it, and a
    // selected match outside it is only stepped past.
    select(&editor, 0..3, cx);
    type_in(&pane, false, "foo", cx);
    run("find.toggle_regex", cx);
    run("find.toggle_preserve_case", cx);
    type_in(&pane, true, "longer", cx);
    select(&editor, 0..3, cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), original);
    assert_eq!(selection(&editor, cx), 8..11);
    for want in [
        "foo Foo\nlonger Foo\nFOO foo\nfoo Foo\n",
        "foo Foo\nlonger longer\nFOO foo\nfoo Foo\n",
        "foo Foo\nlonger longer\nlonger foo\nfoo Foo\n",
        "foo Foo\nlonger longer\nlonger longer\nfoo Foo\n",
    ] {
        run("find.replace_next", cx);
        assert_eq!(text(&editor, cx), want);
    }
    assert_eq!(scope(&editor, cx), Some(vec![8..35]));
}

#[gpui::test]
fn scope_follows_edits_and_clears(cx: &mut TestAppContext) {
    let original = "foo\nfoo\nfoo\nfoo\n";
    let (_dir, workbench, cx) = opened(cx, &[("b.txt", "foo\n"), ("a.txt", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    // No non-empty selection: it stays off, and the caret stays too.
    select(&editor, 4..4, cx);
    run("find.show", cx);
    run("find.toggle_in_selection", cx);
    assert!(!on(&pane, cx));
    assert_eq!(scope(&editor, cx), None);
    assert_eq!(selection(&editor, cx), 4..4);

    select(&editor, 4..11, cx);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![4..11]));
    type_in(&pane, false, "foo", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));

    // Edits before it move it; edits inside and at its ends move its end, and
    // a replacement of the match at its end stays in it.
    let edit = |range: Range<usize>, new: &str, cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| {
            let edits = vec![(range, new.to_owned())];
            editor.model_mut().buffer_mut().edit(edits, Origin::User);
            cx.notify();
        });
        draw(cx);
    };
    edit(0..0, "xy", cx);
    assert_eq!(scope(&editor, cx), Some(vec![6..13]));
    edit(9..9, " foo ", cx);
    assert_eq!(scope(&editor, cx), Some(vec![6..18]));
    assert_eq!(text(&editor, cx), "xyfoo\nfoo foo \nfoo\nfoo\n");
    assert!(text_label(&pane, cx).is_some_and(|label| label.ends_with("of 3")));
    edit(15..18, "barbaz", cx);
    assert_eq!(scope(&editor, cx), Some(vec![6..21]));
    // At its end text joins it; after that it stays.
    edit(21..21, "!", cx);
    assert_eq!(scope(&editor, cx), Some(vec![6..22]));
    edit(23..23, "z", cx);
    assert_eq!(scope(&editor, cx), Some(vec![6..22]));

    // All of it deleted: finds nothing, not everything.
    edit(6..22, "", cx);
    assert_eq!(scope(&editor, cx), Some(vec![]));
    assert_eq!(text_label(&pane, cx).as_deref(), Some("No results"));
    assert!(on(&pane, cx));

    // The toggle's button, focus staying where it was.
    let toggle = cx.debug_bounds("find-in-selection").expect("the toggle");
    cx.simulate_click(toggle.center(), Modifiers::none());
    draw(cx);
    assert!(!on(&pane, cx));
    assert_eq!(scope(&editor, cx), None);

    // Closing the bar drops it, and it's off when it opens again.
    select(&editor, 0..3, cx);
    run("find.toggle_in_selection", cx);
    assert!(on(&pane, cx));
    run("find.close", cx);
    assert!(!on(&pane, cx));
    assert_eq!(scope(&editor, cx), None);
    select(&editor, 0..0, cx);
    run("find.show", cx);
    assert!(!on(&pane, cx));

    // So does following another editor: the scope was in this one's buffer.
    select(&editor, 0..3, cx);
    run("find.toggle_in_selection", cx);
    assert!(on(&pane, cx));
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.cycle(true, window, cx)));
    draw(cx);
    assert!(!on(&pane, cx));
    assert_eq!(scope(&editor, cx), None);

    // The selections the bar was shown with stayed behind too: nothing's
    // selected in this editor, so it stays off.
    let other = editor_of(&pane, cx);
    assert_ne!(other.entity_id(), editor.entity_id());
    cx.update(|window, cx| {
        let input = pane.read(cx).find_bar().unwrap().input().clone();
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);
    assert_eq!(selection(&other, cx), 0..0);
    run("find.toggle_in_selection", cx);
    assert!(!on(&pane, cx));
    assert_eq!(scope(&other, cx), None);
}

#[gpui::test]
fn the_selection_captured_is_the_users_latest(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo\nfoo\nfoo\nfoo\nfoo\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    // Shown with one selection, then another made and the query changed,
    // which moves the selection off it: it's that one that's captured.
    select(&editor, 0..7, cx);
    run("find.show", cx);
    type_in(&pane, false, "fo", cx);
    assert_eq!(selection(&editor, cx), 0..2);
    select(&editor, 8..15, cx);
    type_in(&pane, false, "foo", cx);
    assert_ne!(selection(&editor, cx), 8..15);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![8..15]));
    run("find.toggle_in_selection", cx);

    // Stepping counts as changing the query does.
    select(&editor, 0..7, cx);
    run("find.next", cx);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![0..7]));
    run("find.toggle_in_selection", cx);

    // A change to another selection than the primary one counts too.
    select(&editor, 0..3, cx);
    run("find.next", cx);
    let primary = selection(&editor, cx);
    assert_eq!(primary, 4..7);
    select_all_of(&editor, &[0..3, primary.clone()], cx);
    assert_eq!(selection(&editor, cx), primary);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![0..3, 4..7]));
    run("find.toggle_in_selection", cx);

    // And a caret: nothing the user last made is selected, so it stays off.
    select(&editor, 2..2, cx);
    run("find.toggle_in_selection", cx);
    assert!(!on(&pane, cx));
}

#[gpui::test]
fn rows_outside_the_scope_arent_searched(cx: &mut TestAppContext) {
    let original = "foo foo foo\n".repeat(1_000);
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", &original)]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    select(&editor, 12..23, cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "bar", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 3"));

    // Replace, of a selection outside the scope: only stepped past, reading
    // just the scope's row.
    select(&editor, 0..3, cx);
    let before = regex_calls();
    run("find.replace_next", cx);
    assert!(regex_calls() - before < 20, "{}", regex_calls() - before);
    assert_eq!(selection(&editor, cx), 12..15);

    // Replace All reads just that row too.
    let before = regex_calls();
    run("find.replace_all", cx);
    assert!(regex_calls() - before < 20, "{}", regex_calls() - before);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 3"));
    let rows: Vec<String> = text(&editor, cx)
        .lines()
        .take(3)
        .map(str::to_owned)
        .collect();
    assert_eq!(rows, ["foo foo foo", "bar bar bar", "foo foo foo"]);
}

#[gpui::test]
fn whole_rows_selected_and_what_follows_on_a_row_arent_searched(cx: &mut TestAppContext) {
    let original = format!("foo foo\n{}\n", "a".repeat(5_000));
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", &original)]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    // Row 0 with its line break, the row after it not touched by a match.
    select(&editor, 0..8, cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    run("find.toggle_regex", cx);
    type_in(&pane, false, "foo|a", cx);
    type_in(&pane, true, "x", cx);
    let before = regex_calls();
    run("find.replace_all", cx);
    assert!(regex_calls() - before < 20, "{}", regex_calls() - before);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 2"));
    assert!(text(&editor, cx).starts_with("x x\na"));

    // Only the first on a row of many: the rest of it isn't gone through.
    run("find.toggle_in_selection", cx);
    focus(&editor, cx);
    run("editor.undo", cx);
    assert_eq!(text(&editor, cx), original);
    select(&editor, 8..11, cx);
    cx.update(|window, cx| {
        let input = pane.read(cx).find_bar().unwrap().input().clone();
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![8..11]));
    let before = regex_calls();
    run("find.replace_all", cx);
    assert!(regex_calls() - before < 20, "{}", regex_calls() - before);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 3"));
}

#[gpui::test]
fn replace_all_doesnt_spend_its_time_on_what_follows_the_scope(cx: &mut TestAppContext) {
    let original = format!("foo {}\n", "a".repeat(5_000));
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", &original)]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    select(&editor, 0..3, cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    run("find.toggle_regex", cx);
    run("find.toggle_whole_word", cx);
    type_in(&pane, false, "foo|a", cx);
    type_in(&pane, true, "bar", cx);

    set_call_limit(Some(3));
    run("find.replace_all", cx);
    set_call_limit(None);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 1"));
    assert!(text(&editor, cx).starts_with("bar a"));
}

#[gpui::test]
fn replace_past_its_deadline_finding_the_next_changes_nothing(cx: &mut TestAppContext) {
    let original = "foo foo\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    select(&editor, 0..7, cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "bar", cx);
    select(&editor, 0..3, cx);

    // A call to read the replacement, and one that finds the next and runs
    // the budget out.
    set_call_limit(Some(2));
    run("find.replace_next", cx);
    set_call_limit(None);
    assert_eq!(text(&editor, cx), original);
    assert_eq!(selection(&editor, cx), 0..3);
    assert_eq!(
        find_label(&pane, cx),
        Some(("Search took too long".to_owned(), true))
    );

    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), "bar foo\n");
    assert_eq!(selection(&editor, cx), 4..7);
}

#[gpui::test]
fn following_an_editor_remembers_its_selection(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo\n"), ("b.txt", "foo\nfoo\nfoo\n")]);
    let pane = pane(&workbench, cx);
    let b = editor_of(&pane, cx);

    select(&b, 0..7, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.cycle(true, window, cx)));
    draw(cx);
    run("find.show", cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.cycle(false, window, cx)));
    draw(cx);
    assert_eq!(editor_of(&pane, cx).entity_id(), b.entity_id());

    type_in(&pane, false, "foo", cx);
    assert_eq!(selection(&b, cx), 0..3);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&b, cx), Some(vec![0..7]));
}

#[gpui::test]
fn replace_all_takes_ranges_sharing_a_row_once(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo foo foo foo\nfoo foo\nfoo\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    // The first and third on row 0; the first on row 1; none on row 2.
    select_all_of(&editor, &[0..3, 8..11, 16..19], cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    type_in(&pane, false, "foo", cx);
    type_in(&pane, true, "x", cx);
    run("find.replace_all", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 3"));
    assert_eq!(text(&editor, cx), "x foo x foo\nx foo\nfoo\n");
}

#[gpui::test]
fn a_range_replaced_away_is_gone_when_finding_the_next(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo xfoo\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor_of(&pane, cx);

    select_all_of(&editor, &[0..3, 5..8], cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    assert_eq!(scope(&editor, cx), Some(vec![0..3, 5..8]));
    run("find.toggle_regex", cx);
    type_in(&pane, false, "foo|^", cx);
    select(&editor, 0..3, cx);
    run("find.replace_next", cx);
    assert_eq!(text(&editor, cx), " xfoo\n");
    assert_eq!(selection(&editor, cx), 2..5);
    assert_eq!(scope(&editor, cx), Some(vec![2..5]));
}
