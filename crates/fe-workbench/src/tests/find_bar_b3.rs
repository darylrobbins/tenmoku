//! The find and replace row in B3 (`docs/design/design-system.md`, Find and
//! replace): the find field a bordered input with the accent focus ring,
//! the option toggles inside it, the count a plural message, Find in
//! Selection a toggle, and the equivalent Vim command at the row's end.

// A scope of one range is what these mean, not a mistyped `Vec` of offsets.
#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;

use fe_buffer::ResolvedSelection;
use fe_gpui::{EditorView, set_vim_enabled, set_vim_search_case};
use fe_ui::{FOCUS_RING, Icon};
use fe_vim::{
    HeadlessVim,
    search::{regex_calls, set_call_limit},
};
use gpui::{Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext};

use super::find_bar::{
    draw, editor, focus, input_focused, label, opened, pane, run, select, selection, text_label,
};
use crate::{Pane, find_bar::FindOp};

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"))
}

/// Clicks the middle of what `selector` names, and draws.
fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let at = bounds(selector, cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
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

fn focus_query(pane: &Entity<Pane>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let input = pane.read(cx).find_bar().unwrap().input().clone();
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);
}

fn vim_command(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Option<String> {
    pane.read_with(cx, |pane, cx| pane.find_bar().unwrap().vim_command(cx))
}

/// The ring a focused field draws: 2 px of the accent, 1 px out from its
/// border ([`fe_ui::focus_ring`]).
fn assert_rings(field: &'static str, ring: &'static str, cx: &mut VisualTestContext) {
    let (field, drawn) = (bounds(field, cx), bounds(ring, cx));
    let out = FOCUS_RING.width + FOCUS_RING.offset;
    assert_eq!(drawn.origin.x, field.origin.x - out, "{ring}: left");
    assert_eq!(drawn.origin.y, field.origin.y - out, "{ring}: top");
    assert_eq!(drawn.size.width, field.size.width + out * 2., "{ring}");
    assert_eq!(drawn.size.height, field.size.height + out * 2., "{ring}");
}

#[gpui::test]
fn the_find_field_is_a_bordered_input_ringed_while_focused(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo one\nfoo two\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    select(&editor, 0..3, cx);
    run("find.show", cx);
    assert!(input_focused(&pane, cx));

    // The input is inside the field, which the ring goes round.
    let field = bounds("find-field", cx);
    let input = pane.read_with(cx, |pane, _| pane.find_bar().unwrap().input().clone());
    let input_bounds = bounds("text-input-text", cx);
    assert!(inside(input_bounds, field), "{input_bounds:?} in {field:?}");
    assert_rings("find-field", "find-field-focus-ring", cx);
    assert!(cx.debug_bounds("find-replace-field").is_none());
    assert_eq!(FOCUS_RING.width, gpui::px(2.));

    // With focus in the editor there's no ring; back in the input, there is.
    focus(&editor, cx);
    assert!(cx.debug_bounds("find-field-focus-ring").is_none());
    cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    draw(cx);
    assert_rings("find-field", "find-field-focus-ring", cx);

    // The replace field is drawn the same, and rings while it has focus.
    run("find.show_replace", cx);
    assert!(cx.debug_bounds("find-field-focus-ring").is_none());
    assert_rings("find-replace-field", "find-replace-field-focus-ring", cx);
    let (find, replace) = (bounds("find-field", cx), bounds("find-replace-field", cx));
    assert_eq!(
        (find.left(), find.size.width),
        (replace.left(), replace.size.width)
    );
    // Preserve Case is inside it; Replace and Replace All follow it.
    assert!(inside(bounds("find-preserve-case", cx), replace));
    for button in ["find-replace", "find-replace-all"] {
        assert!(bounds(button, cx).left() >= replace.right(), "{button}");
    }
}

#[gpui::test]
fn case_whole_word_and_regex_are_named_toggles_inside_the_field(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo one\nfoo two\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    select(&editor, 0..3, cx);
    run("find.show", cx);

    let toggles = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| pane.find_bar().unwrap().field_toggles())
    };
    let named: Vec<_> = (toggles(cx).into_iter())
        .map(|(op, icon, name, on)| (op, icon, name.to_string(), on))
        .collect();
    assert_eq!(
        named,
        [
            (
                FindOp::ToggleCase,
                Icon::CaseSensitive,
                "Match Case".to_owned(),
                false
            ),
            (
                FindOp::ToggleWholeWord,
                Icon::WholeWord,
                "Match Whole Word".to_owned(),
                false
            ),
            (
                FindOp::ToggleRegex,
                Icon::Regex,
                "Use Regular Expression".to_owned(),
                false
            ),
        ]
    );

    // Each is inside the field, after the query, in order.
    let field = bounds("find-field", cx);
    let query = bounds("text-input-text", cx);
    let mut left = query.right();
    for selector in ["find-toggle:0", "find-toggle:1", "find-toggle:2"] {
        let toggle = bounds(selector, cx);
        assert!(inside(toggle, field), "{selector}: {toggle:?} in {field:?}");
        assert!(toggle.left() >= left, "{selector}");
        left = toggle.right();
    }

    // A click turns one on, its tint the accent's (kiln-ui's ToggleButton).
    for (ix, selector) in ["find-toggle:0", "find-toggle:1", "find-toggle:2"]
        .into_iter()
        .enumerate()
    {
        let at = bounds(selector, cx).center();
        cx.simulate_click(at, Modifiers::none());
        draw(cx);
        assert!(toggles(cx)[ix].3, "{selector} is on");
        assert!(input_focused(&pane, cx), "{selector} kept focus");
    }
    let query = pane.read_with(cx, |pane, cx| pane.find_bar().unwrap().query(cx));
    assert!(query.match_case && query.whole_word && query.regex);
}

#[gpui::test]
fn n_of_m_is_a_plural_message(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo one\nfoo two\nbar\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    select(&editor, 0..3, cx);
    run("find.show", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));
    // After the field, before Find Previous, Find Next and Find in
    // Selection.
    let count = bounds("find-count", cx);
    assert!(count.left() >= bounds("find-field", cx).right());
    let mut left = count.right();
    for selector in ["find-previous", "find-next", "find-in-selection"] {
        let drawn = bounds(selector, cx);
        assert!(drawn.left() >= left, "{selector}");
        left = drawn.right();
    }
    click("find-next", cx);
    draw(cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("2 of 2"));
    click("find-previous", cx);
    draw(cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));
    type_in(&pane, false, "bar", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 1"));
    type_in(&pane, false, "zzz", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("No results"));

    // Each count is a message selecting on the total, the plural it is.
    let catalogue = include_str!("../../l10n/en-US/find.ftl");
    for key in [
        "workbench-find-count",
        "workbench-find-count-capped",
        "workbench-find-count-unknown",
        "workbench-find-count-unknown-capped",
    ] {
        let message = format!("\n{key} =\n    {{ $total ->\n");
        assert!(catalogue.contains(&message), "{key} selects on $total");
    }
    assert!(catalogue.contains("\nworkbench-find-replaced =\n    { $count ->\n"));
    let count = |key: &str, current: usize, total: usize| {
        kiln_l10n::tr!(key, current = current, total = total).to_string()
    };
    assert_eq!(count("workbench-find-count", 1, 1), "1 of 1");
    assert_eq!(count("workbench-find-count", 2, 3), "2 of 3");
    assert_eq!(
        count("workbench-find-count-capped", 2, 10_000),
        "2 of 10000+"
    );
}

#[gpui::test]
fn in_selection_toggles(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo one\nfoo two\nfoo 333\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    let on = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| pane.find_bar().unwrap().in_selection())
    };
    run("find.show", cx);
    focus(&editor, cx);
    select(&editor, 0..16, cx);
    focus_query(&pane, cx);
    type_in(&pane, false, "foo", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 3"));

    let toggle = bounds("find-in-selection", cx).center();
    cx.simulate_click(toggle, Modifiers::none());
    draw(cx);
    assert!(on(cx));
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 2"));
    assert!(input_focused(&pane, cx));
    cx.simulate_click(toggle, Modifiers::none());
    draw(cx);
    assert!(!on(cx));
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 3"));
}

/// Where vim's `/` and `n` land in `text` from its start, going round once,
/// with 'ignorecase' and 'smartcase' as `case` has them, in order.
fn vim_landings(text: &str, search: &str, case: (bool, bool)) -> Vec<(u32, usize)> {
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.set_search_case(case.0, case.1);
    vim.feed(&format!("{search}<CR>")).unwrap();
    let mut landed = vec![vim.cursor()];
    loop {
        vim.feed("n").unwrap();
        if vim.cursor() == landed[0] {
            break;
        }
        landed.push(vim.cursor());
    }
    landed.sort();
    landed
}

#[gpui::test]
fn vim_equivalent_matches_the_query(cx: &mut TestAppContext) {
    let original = "let dirty = Dirty::new();\nif dirty { DIRTY }\nundirtyx\n";
    let (_dir, workbench, cx) = opened(cx, &[("a.rs", original)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    run("find.show", cx);
    type_in(&pane, false, "dirty", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("1 of 5"));

    // With 'ignorecase' and 'smartcase', as LazyVim has them, it's `/dirty`,
    // drawn faintly at the end of the row.
    cx.update(|_, cx| set_vim_search_case(cx, true, true));
    draw(cx);
    assert_eq!(vim_command(&pane, cx).as_deref(), Some("/dirty"));
    let shown = bounds("find-vim-command", cx);
    let bar = bounds("pane-find-bar", cx);
    assert!(shown.left() > bounds("find-in-selection", cx).right());
    assert!(
        bar.right() - shown.right() < gpui::px(16.),
        "{shown:?} at {bar:?}'s end"
    );

    // Vim's `/` and `n` land on the matches the bar steps through.
    let mut found = Vec::new();
    for _ in 0..5 {
        run("find.next", cx);
        let at = selection(&editor, cx).start;
        let point = editor.read_with(cx, |editor, _| editor.model().text().offset_to_point(at));
        found.push((point.row, point.column as usize));
    }
    found.sort();
    assert_eq!(vim_landings(original, "/dirty", (true, true)), found);

    // Vim's own defaults: no 'ignorecase', so `\c`; with Match Case on, the
    // plain pattern, which finds what vim's default search does.
    cx.update(|_, cx| set_vim_search_case(cx, false, false));
    draw(cx);
    assert_eq!(vim_command(&pane, cx).as_deref(), Some("/dirty\\c"));
    run("find.toggle_case", cx);
    assert_eq!(vim_command(&pane, cx).as_deref(), Some("/dirty"));
    assert!(text_label(&pane, cx).unwrap().ends_with(" of 3"));
    assert_eq!(vim_landings(original, "/dirty", (false, false)).len(), 3);

    // Replacing, with Match Case and Regex on: `:%s`, flags from the toggles,
    // which makes the text Replace All does.
    run("find.toggle_regex", cx);
    run("find.show_replace", cx);
    type_in(&pane, true, "needs_paint", cx);
    let command = vim_command(&pane, cx).expect("a :%s");
    assert_eq!(command, ":%s/dirty/needs_paint/gI");
    let mut vim = HeadlessVim::new(original, (0, 0));
    vim.feed(&format!("{command}<CR>")).unwrap();
    run("find.replace_all", cx);
    assert_eq!(text_label(&pane, cx).as_deref(), Some("Replaced 3"));
    assert_eq!(text(&editor, cx), vim.text());
    assert_eq!(
        vim.text(),
        "let needs_paint = Dirty::new();\nif needs_paint { DIRTY }\nunneeds_paintx\n"
    );

    // Find in Selection, which Vim has no command for, shows none.
    focus(&editor, cx);
    select(&editor, 0..10, cx);
    focus_query(&pane, cx);
    run("find.toggle_in_selection", cx);
    assert_eq!(vim_command(&pane, cx), None);
    assert!(cx.debug_bounds("find-vim-command").is_none());
    run("find.toggle_in_selection", cx);
    assert!(vim_command(&pane, cx).is_some());

    // With vim off it isn't shown.
    cx.update(|_, cx| set_vim_enabled(cx, false));
    draw(cx);
    assert!(cx.debug_bounds("find-vim-command").is_none());
}

#[gpui::test]
fn the_vim_command_copies_on_a_click(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "dirty\n")]);
    let pane = pane(&workbench, cx);
    cx.update(|_, cx| {
        set_vim_enabled(cx, true);
        set_vim_search_case(cx, true, true);
    });
    run("find.show", cx);
    type_in(&pane, false, "dirty", cx);
    click("find-vim-command", cx);
    draw(cx);
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("/dirty"));
    // Focus stays where it was.
    assert!(input_focused(&pane, cx));
}

/// Sets the selections to `ranges`, the last the primary one.
fn select_all_of(editor: &Entity<EditorView>, ranges: &[Range<usize>], cx: &mut VisualTestContext) {
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

/// Replace Next, with Find in Selection on, from a selection or caret
/// inside the scope at a row's start that isn't a match: telling that
/// looks no further along the row than the caret, so the excluded `a`s
/// after it (none a whole word) aren't each rejected, and a five-call
/// budget for each search is enough to land on the scoped `foo` (T410 F3).
fn replace_next_skips_the_excluded_suffix(vim: bool, cx: &mut TestAppContext) {
    let original = format!("x {}\nfoo\n", "a".repeat(5_000));
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", &original)]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    let foo = 5_003..5_006;
    select_all_of(&editor, &[0..1, foo.clone()], cx);
    run("find.show_replace", cx);
    run("find.toggle_in_selection", cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.find_scope()),
        Some(vec![0..1, foo.clone()])
    );
    run("find.toggle_regex", cx);
    run("find.toggle_whole_word", cx);
    type_in(&pane, false, "foo|a", cx);
    type_in(&pane, true, "bar", cx);
    if vim {
        cx.update(|_, cx| set_vim_enabled(cx, true));
        draw(cx);
    }
    focus(&editor, cx);
    select(&editor, if vim { 0..0 } else { 0..1 }, cx);
    cx.update(|window, cx| {
        let input = pane.read(cx).find_bar().unwrap().replace_input().clone();
        window.focus(&input.focus_handle(cx), cx);
    });
    draw(cx);

    let before = regex_calls();
    set_call_limit(Some(5));
    run("find.replace_next", cx);
    set_call_limit(None);
    let calls = regex_calls() - before;
    assert_ne!(
        label(&pane, cx).map(|(text, _)| text).as_deref(),
        Some("Search took too long")
    );
    // Thousands, rejecting each `a`, before the bound.
    assert!(calls < 20, "{calls} regex calls");
    assert_eq!(selection(&editor, cx).start, foo.start);
    assert_eq!(text(&editor, cx), original);
}

#[gpui::test]
fn scoped_replace_next_from_a_selection_skips_the_excluded_suffix(cx: &mut TestAppContext) {
    replace_next_skips_the_excluded_suffix(false, cx);
}

#[gpui::test]
fn scoped_replace_next_from_the_vim_caret_skips_the_excluded_suffix(cx: &mut TestAppContext) {
    replace_next_skips_the_excluded_suffix(true, cx);
}
