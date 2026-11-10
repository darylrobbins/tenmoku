//! Tests of the line and screen motions: `ge gE _ + - | H M L gj gk` (see
//! `src/line_motions.rs`).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_command::{Effect, Invocation};
use fe_editor::{EditorModel, commands::HeadlessHost};

use super::{Measuring, commit};
use crate::{
    HeadlessVim, Mode, Motion, MoveArgs, RangeClass, Registers, VimState, VimTarget, context,
    line_motions, motion::Cursor, registry,
};

/// The cursor after `keys` from `cursor` in `text`, and whether the last
/// key failed.
fn after(text: &str, cursor: (u32, usize), keys: &str) -> ((u32, usize), bool) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    (vim.cursor(), vim.state().failed())
}

/// The text and cursor after `keys`.
fn edited(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    (vim.text(), vim.cursor())
}

#[test]
fn ge_and_big_ge_go_back_to_word_ends() {
    // Punctuation is a word of its own for `ge`, not for `gE`.
    assert_eq!(after("foo.bar baz", (0, 8), "ge"), ((0, 6), false));
    assert_eq!(after("foo.bar baz", (0, 4), "ge"), ((0, 3), false));
    assert_eq!(after("foo.bar baz", (0, 4), "gE"), ((0, 0), false));
    assert_eq!(after("a.b c.d e", (0, 8), "2gE"), ((0, 2), false));
    assert_eq!(after("one two three four", (0, 14), "2ge"), ((0, 6), false));
    // Across rows; an empty row is a word, a blank one isn't.
    assert_eq!(after("abc\n  def", (1, 2), "ge"), ((0, 2), false));
    assert_eq!(after("abc\n\ndef", (2, 0), "ge"), ((1, 0), false));
    assert_eq!(after("abc\n   \ndef", (2, 0), "ge"), ((0, 2), false));
    // From a blank, the word before it.
    assert_eq!(after("foo   bar", (0, 4), "ge"), ((0, 2), false));
    // A count past the start stops there; at the start it fails.
    assert_eq!(after("ab cd ef", (0, 6), "9ge"), ((0, 0), false));
    assert_eq!(after("abc def", (0, 0), "ge"), ((0, 0), true));
    // Inclusive: `dge` takes the char it ends on and the caret's.
    assert_eq!(
        edited("foo bar baz", (0, 8), "dge"),
        ("foo baaz".into(), (0, 6))
    );
    assert_eq!(Motion::WordEndBackward.class(), RangeClass::Inclusive);
    assert_eq!(Motion::BigWordEndBackward.class(), RangeClass::Inclusive);
}

#[test]
fn underscore_plus_and_minus_go_to_first_non_blanks() {
    let text = "  a\n b\n\tc\nd";
    assert_eq!(after(text, (0, 0), "_"), ((0, 2), false));
    assert_eq!(after(text, (0, 0), "3_"), ((2, 1), false));
    assert_eq!(after(text, (0, 0), "+"), ((1, 1), false));
    assert_eq!(after(text, (0, 0), "2+"), ((2, 1), false));
    assert_eq!(after(text, (2, 1), "-"), ((1, 1), false));
    assert_eq!(after(text, (3, 0), "2-"), ((1, 1), false));
    // Past the ends a count clamps; from the last or first row they fail.
    assert_eq!(after(text, (0, 0), "9+"), ((3, 0), false));
    assert_eq!(after(text, (3, 0), "9-"), ((0, 2), false));
    assert_eq!(after(text, (3, 0), "+"), ((3, 0), true));
    assert_eq!(after(text, (0, 2), "-"), ((0, 2), true));
    assert_eq!(after(text, (3, 0), "2_"), ((3, 0), true));
    // A blank row's first non-blank is its last char.
    assert_eq!(after("a\n   \nb", (0, 0), "+"), ((1, 2), false));
    // Linewise.
    assert_eq!(edited("a\nb\nc\nd", (0, 0), "d+"), ("c\nd".into(), (0, 0)));
    assert_eq!(edited("a\nb\nc\nd", (3, 0), "d2-"), ("a".into(), (0, 0)));
    assert_eq!(edited("a\nb\nc\nd\ne", (0, 0), "d+."), ("e".into(), (0, 0)));
    for motion in [Motion::NextLineFirstNonBlank, Motion::PrevLineFirstNonBlank] {
        assert_eq!(motion.class(), RangeClass::Linewise);
    }
}

#[test]
fn bar_goes_to_a_screen_column() {
    assert_eq!(after("abcdef", (0, 0), "4|"), ((0, 3), false));
    assert_eq!(after("abcdef", (0, 3), "|"), ((0, 0), false));
    // Past the row's end, its last char; the goal stays the column asked.
    assert_eq!(after("abc", (0, 0), "99|"), ((0, 2), false));
    assert_eq!(after("abc\nabcdefghij", (0, 0), "8|j"), ((1, 7), false));
    // A tab (4 columns) and a wide char cover their columns.
    assert_eq!(after("\tabc", (0, 3), "3|"), ((0, 0), false));
    assert_eq!(after("\tabc", (0, 0), "6|"), ((0, 2), false));
    assert_eq!(after("a中b", (0, 0), "3|"), ((0, 1), false));
    assert_eq!(after("a中b", (0, 0), "4|"), ((0, 4), false));
    // Exclusive.
    assert_eq!(edited("abcdef", (0, 5), "d2|"), ("af".into(), (0, 1)));
    // In visual mode, past the row's end is the row's end.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed("v99|").unwrap();
    assert_eq!(vim.cursor(), (0, 3));
    assert_eq!(Motion::Column.class(), RangeClass::Exclusive);
}

/// 30 rows, `row N` each, indented by 0 to 4 spaces.
fn rows30() -> String {
    (0..30)
        .map(|row| format!("{}row {row}", "  ".repeat(row % 3)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A vim on [`rows30`] at `cursor` in a window 10 rows high.
fn windowed(cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(&rows30(), cursor);
    vim.set_window_rows(10);
    vim
}

#[test]
fn h_m_l_go_to_the_viewport_rows() {
    let row_after = |cursor, keys| {
        let mut vim = windowed(cursor);
        vim.feed(keys).unwrap();
        vim.cursor()
    };
    // The column is kept, as `G` keeps it.
    assert_eq!(row_after((5, 3), "H"), (0, 3));
    assert_eq!(row_after((5, 3), "M"), (4, 3));
    assert_eq!(row_after((5, 3), "L"), (9, 3));
    assert_eq!(row_after((5, 0), "3H"), (2, 0));
    assert_eq!(row_after((5, 0), "3L"), (7, 0));
    assert_eq!(row_after((5, 0), "20H"), (9, 0));
    assert_eq!(row_after((5, 0), "20L"), (0, 0));
    assert_eq!(row_after((3, 0), "$L"), (9, 4));
    // From the view's top row, once scrolled.
    assert_eq!(row_after((5, 0), "<C-e><C-e><C-e>H"), (3, 0));
    assert_eq!(row_after((5, 0), "<C-e><C-e><C-e>M"), (7, 0));
    assert_eq!(row_after((5, 0), "<C-e><C-e><C-e>L"), (12, 0));
    // Only the rows the text has count.
    let mut vim = HeadlessVim::new("a\nb\nc\nd\ne", (0, 0));
    vim.set_window_rows(10);
    vim.feed("M").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
    vim.feed("L").unwrap();
    assert_eq!(vim.cursor(), (4, 0));
    // A jump, linewise.
    let mut vim = windowed((7, 2));
    vim.feed("H").unwrap();
    assert_eq!(vim.mark('\''), Some((7, 2)));
    let mut vim = windowed((2, 4));
    vim.feed("dL").unwrap();
    assert_eq!(vim.text().lines().count(), 22);
    assert_eq!(vim.text().lines().next(), Some("row 0"));
    let mut vim = windowed((7, 2));
    vim.feed("yH").unwrap();
    let yanked = vim.registers().get('"').unwrap();
    assert!(yanked.linewise);
    assert_eq!(yanked.text.lines().count(), 8);
    for motion in [
        Motion::WindowTop,
        Motion::WindowMiddle,
        Motion::WindowBottom,
    ] {
        assert!(motion.is_jump());
        assert_eq!(motion.class(), RangeClass::Linewise);
    }
}

/// Runs `keys` against a model whose host has no viewport and `page_rows`
/// 8, so the view is the 10 rows from row 0.
#[test]
fn no_viewport_is_page_rows_plus_two_from_row_0() {
    let registry = registry();
    let mut model = EditorModel::new(Buffer::new(&rows30()), None);
    let mut host = HeadlessHost::new();
    host.page_rows = 8;
    let mut state = VimState::new();
    let mut registers = Registers::new();
    for (keys, row) in [("L", 9), ("M", 4), ("3L", 7), ("H", 0)] {
        let invocation = commit(keys).context(context(&state));
        let mut target = VimTarget {
            model: &mut model,
            host: &mut host,
            state: &mut state,
            registers: &mut registers,
        };
        registry.dispatch(&mut target, &invocation).unwrap();
        let head = model.primary_selection().head();
        assert_eq!(model.text().offset_to_point(head).row, row, "{keys}");
    }
}

/// `motion` once from `cursor` in `text`, outside any command.
fn moved(text: &str, cursor: usize, motion: Motion, count: Option<u32>) -> (usize, bool) {
    let text = fe_text::Text::new(text);
    let mut caret = Cursor::at(&text, cursor);
    let args = MoveArgs {
        motion,
        extend: false,
        count,
    };
    let result = line_motions::motion(&mut caret, args, None, 4);
    (caret.offset(), result.is_err())
}

/// Outside a command the view is a headless host's with no viewport:
/// `page_rows() + 2` (22) rows from row 0.
#[test]
fn outside_a_command_the_view_is_headless_and_gj_is_j() {
    let text = (0..40).map(|row| format!("r{row}")).collect::<Vec<_>>();
    let text = text.join("\n");
    let row_start = |row: usize| text.split('\n').take(row).map(|r| r.len() + 1).sum();
    assert_eq!(
        moved(&text, 0, Motion::WindowBottom, None),
        (row_start(21), false)
    );
    assert_eq!(
        moved(&text, 0, Motion::WindowMiddle, None),
        (row_start(10), false)
    );
    // `gj` and `gk` move between buffer rows, keeping the column.
    assert_eq!(
        moved("abc\nabcdef", 2, Motion::DisplayDown, None),
        (6, false)
    );
    assert_eq!(moved("abcdef\nab", 9, Motion::DisplayUp, None), (1, false));
    assert_eq!(moved("ab\ncd", 3, Motion::DisplayDown, None), (3, true));
    // Short of the count, they move and fail.
    assert_eq!(moved("a\nb\nc", 0, Motion::DisplayDown, Some(5)), (4, true));
}

#[test]
fn with_view_restores_the_view_before_it() {
    let text = (0..40).map(|row| format!("r{row}")).collect::<Vec<_>>();
    let text = text.join("\n");
    let mut model = EditorModel::new(Buffer::new(&text), None);
    let mut outer = HeadlessHost::new();
    outer.page_rows = 8;
    let mut inner = HeadlessHost::new();
    inner.page_rows = 18;
    let mut state = VimState::new();
    let mut registers = Registers::new();
    let bottom_row = || {
        let text = fe_text::Text::new(&text);
        let mut caret = Cursor::at(&text, 0);
        let args = MoveArgs {
            motion: Motion::WindowBottom,
            extend: false,
            count: None,
        };
        line_motions::motion(&mut caret, args, None, 4).unwrap();
        caret.row
    };
    let mut target = VimTarget {
        model: &mut model,
        host: &mut outer,
        state: &mut state,
        registers: &mut registers,
    };
    line_motions::with_view(&mut target, None, |target| {
        assert_eq!(bottom_row(), 9);
        let mut nested = VimTarget {
            model: &mut *target.model,
            host: &mut inner,
            state: &mut *target.state,
            registers: &mut *target.registers,
        };
        line_motions::with_view(&mut nested, None, |_| assert_eq!(bottom_row(), 19));
        assert_eq!(bottom_row(), 9, "the outer view is back");
        // A panic in a nested one still puts the outer view back.
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            line_motions::with_view(&mut nested, None, |_| panic!("in the closure"))
        }));
        assert!(panicked.is_err());
        assert_eq!(bottom_row(), 9, "the outer view is back after a panic");
    });
    assert_eq!(bottom_row(), 21, "no view once it returns");
}

/// `gj` and `gk` over a soft-wrapped row: a row of 25 chars wraps at 10.
#[test]
fn display_rows() {
    let text = "abcdefghijklmnopqrstuvwxy\nshort";
    let wrapped = |cursor, keys: &str| {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.model_mut().set_soft_wrap(true);
        vim.model_mut().set_wrap_width(10.0);
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor(), vim.state().failed())
    };
    let cursor = |cursor, keys| wrapped(cursor, keys).1;
    // Down the row's display rows at the same display column in each.
    assert_eq!(cursor((0, 3), "gj"), (0, 13));
    assert_eq!(cursor((0, 3), "gjgj"), (0, 23));
    assert_eq!(cursor((0, 3), "gjgjgj"), (1, 3));
    assert_eq!(cursor((0, 3), "3gj"), (1, 3));
    assert_eq!(cursor((1, 3), "gk"), (0, 23));
    assert_eq!(cursor((1, 3), "3gk"), (0, 3));
    assert_eq!(cursor((0, 23), "gkgk"), (0, 3));
    // A part too short ends at its last char, the goal kept for the next.
    assert_eq!(cursor((0, 8), "gjgj"), (0, 24));
    assert_eq!(cursor((0, 8), "gjgjgk"), (0, 18));
    // `$` goes to each part's end.
    assert_eq!(cursor((0, 0), "$gk"), (0, 19));
    // Past the last display row it moves, then fails.
    assert_eq!(wrapped((0, 3), "9gj").1, (1, 3));
    assert!(wrapped((0, 3), "9gj").2);
    assert!(wrapped((1, 3), "gj").2);
    // Charwise exclusive under an operator.
    assert_eq!(
        wrapped((0, 3), "dgj"),
        ("abcnopqrstuvwxy\nshort".into(), (0, 3), false)
    );
    assert_eq!(
        wrapped((0, 13), "dgk"),
        ("abcnopqrstuvwxy\nshort".into(), (0, 3), false)
    );
    // In visual mode, from the last display row of the row.
    let mut vim = HeadlessVim::new(text, (0, 3));
    vim.model_mut().set_soft_wrap(true);
    vim.model_mut().set_wrap_width(10.0);
    vim.feed("vgj").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 13), Some((0, 3))));

    // With wrap off they move between buffer rows.
    let plain = |cursor, keys: &str| {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.model_mut().set_wrap_width(10.0);
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    assert_eq!(plain((0, 3), "gj").1, (1, 3));
    assert_eq!(plain((0, 23), "gj").1, (1, 4));
    assert_eq!(plain((1, 4), "gk").1, (0, 4));
    assert_eq!(plain((0, 3), "dgj"), ("abcrt".into(), (0, 3)));
    for motion in [Motion::DisplayDown, Motion::DisplayUp] {
        assert_eq!(motion.class(), RangeClass::Exclusive);
        assert!(!motion.is_jump());
    }
}

/// Counted `ge gE gj gk` that run out part way: nvim moves the caret and
/// fails, and an operator over one changes nothing but leaves the caret
/// where the motion got to.
#[test]
fn counted_motions_failing_part_way() {
    // A step that can't move fails; one that runs into the start doesn't.
    assert_eq!(after("\nabc def\nxyz", (1, 4), "3ge"), ((0, 0), true));
    assert_eq!(after("a bc", (0, 3), "2ge"), ((0, 0), true));
    assert_eq!(after("a bc", (0, 3), "2gE"), ((0, 0), true));
    assert_eq!(after("abc def\nxyz", (1, 1), "9ge"), ((0, 0), false));
    assert_eq!(after("abc\ndef\nghi", (0, 1), "9gj"), ((2, 1), true));
    // In visual mode the cursor goes there too, and the key fails.
    assert_eq!(after("a bc", (0, 3), "v2ge"), ((0, 0), true));
    assert_eq!(after("abc def", (0, 5), "v9ge"), ((0, 0), false));
    assert_eq!(after("abc\ndef\nghi", (0, 1), "v9gj"), ((2, 1), true));
    assert_eq!(after("abc\ndef\nghi", (2, 1), "v9gk"), ((0, 1), true));
    for operator in ["d", "c", "y"] {
        for (text, cursor, keys, to) in [
            ("\nabc def\nxyz", (1, 4), "3ge", (0, 0)),
            ("\nabc def\nxyz", (1, 4), "3gE", (0, 0)),
            ("a bc", (0, 3), "2ge", (0, 0)),
            ("abc\ndef\nghi", (0, 1), "9gj", (2, 1)),
            ("abc\ndef\nghi", (2, 1), "9gk", (0, 1)),
        ] {
            let keys = format!("{operator}{keys}");
            let mut vim = HeadlessVim::new(text, cursor);
            vim.feed(&keys).unwrap();
            let state = vim.state();
            assert_eq!(
                (vim.text(), vim.cursor(), state.failed(), state.mode()),
                (text.to_string(), to, true, Mode::Normal),
                "{keys}"
            );
        }
    }
    // The caret keeps the goal the failed `gj` had.
    assert_eq!(after("abcdef\nabcdef\nab", (0, 4), "d9gj").0, (2, 1));
    assert_eq!(after("abcdef\nabcdef\nab", (0, 4), "d9gjk").0, (1, 4));
    // `y-` leaves its caret on the first non-blank, the goal there.
    assert_eq!(
        edited("abc\n  def\nghijkl\nmnopqr", (2, 5), "y-"),
        ("abc\n  def\nghijkl\nmnopqr".into(), (1, 2))
    );
    assert_eq!(
        after("abc\n  def\nghijkl\nmnopqr", (2, 5), "y-jj").0,
        (3, 2)
    );
}

/// From a visual cursor past the row's end, the word ending the row is the
/// first `ge` goes back to.
#[test]
fn visual_ge_from_past_the_row_end() {
    assert_eq!(after("abc def", (0, 0), "v$ge"), ((0, 6), false));
    assert_eq!(after("abc d.f", (0, 0), "v$gE"), ((0, 6), false));
    assert_eq!(after("abc def", (0, 0), "v$2ge"), ((0, 2), false));
    assert_eq!(after("abc def  ", (0, 0), "v$ge"), ((0, 6), false));
    assert_eq!(after("abc def\nxy", (1, 0), "v$ge"), ((1, 1), false));
    // Not past the end, the word it's on is skipped.
    assert_eq!(after("abc def", (0, 6), "vge"), ((0, 2), false));
}

/// Soft wrap at `width`.
fn wrap(vim: &mut HeadlessVim, width: f32) {
    vim.model_mut().set_soft_wrap(true);
    vim.model_mut().set_wrap_width(width);
}

/// A goal past what a display row holds takes `gj gk` no further than its
/// last column, which becomes the goal; `$`'s stays the row's end.
#[test]
fn display_rows_with_a_goal_past_the_width() {
    let wrapped = |text: &str, width, keys: &str| {
        let mut vim = HeadlessVim::new(text, (0, 0));
        wrap(&mut vim, width);
        vim.feed(keys).unwrap();
        vim.cursor()
    };
    let long = format!("{}\n{}", "a".repeat(25), "b".repeat(100));
    assert_eq!(wrapped(&long, 10.0, "99|"), (0, 24));
    assert_eq!(wrapped(&long, 10.0, "99|gk"), (0, 19));
    assert_eq!(wrapped(&long, 10.0, "99|gkj"), (1, 19));
    assert_eq!(wrapped(&long, 10.0, "99|gkgkj"), (1, 9));
    assert_eq!(wrapped(&long, 10.0, "99|gj"), (1, 9));
    assert_eq!(wrapped(&long, 10.0, "$gk"), (0, 19));
    assert_eq!(wrapped(&long, 10.0, "$gkj"), (1, 99));
    // Measured on the wrapped row, onto a short one.
    let short = format!("{}\nshort", "a".repeat(25));
    assert_eq!(wrapped(&short, 10.0, "99|gj"), (1, 4));
    assert_eq!(wrapped(&short, 10.0, "99|gjk"), (0, 9));
    // A goal within the width that the row's end cuts short is kept.
    let rows = format!("{}\n{}", "c".repeat(62), "d".repeat(80));
    assert_eq!(wrapped(&rows, 20.0, "5|3gj"), (0, 61));
    assert_eq!(wrapped(&rows, 20.0, "5|3gjj"), (1, 64));
    assert_eq!(wrapped(&rows, 20.0, "5|3gjgk"), (0, 44));
    // Wrapped before a word, a display row is narrower than the width: the
    // others show what it holds.
    let words = format!("a {}\nz", "b".repeat(27));
    let from = |cursor, keys: &str| {
        let mut vim = HeadlessVim::new(&words, cursor);
        wrap(&mut vim, 10.0);
        vim.feed(keys).unwrap();
        vim.cursor()
    };
    assert_eq!(from((0, 11), "gj"), (0, 21));
    assert_eq!(from((0, 11), "gjgk"), (0, 11));
    assert_eq!(from((0, 21), "gk"), (0, 11));
    assert_eq!(from((0, 11), "gk"), (0, 1));
    assert_eq!(from((0, 11), "gkgj"), (0, 11));
    assert_eq!(from((0, 11), "gkgjgj"), (0, 21));
    // Several display rows in a row broken before words: a goal past them
    // that a display row may hold is kept, back to a full-width one.
    let shortened = format!("abcdefghij{}{}", "abcdefg ".repeat(8), "z".repeat(20));
    let mut vim = HeadlessVim::new(&shortened, (0, 9));
    wrap(&mut vim, 10.0);
    vim.feed("3gj").unwrap();
    assert_eq!(vim.cursor(), (0, 33));
    vim.feed("gj").unwrap();
    assert_eq!(vim.cursor(), (0, 41));
    vim.feed("6gj").unwrap();
    assert_eq!(vim.cursor(), (0, 93));
    vim.feed("8gk").unwrap();
    assert_eq!(vim.cursor(), (0, 25));
    // Tab stops count from each display row's start, as fe-display lays
    // the rows out: the tab after "bbb" is one column wide there.
    let tabbed = format!("a {}\tccdd\nABCDEFGHI", "b".repeat(12));
    let tabs = |cursor, keys: &str| {
        let mut vim = HeadlessVim::new(&tabbed, cursor);
        wrap(&mut vim, 9.0);
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    assert_eq!(tabs((0, 15), "gj").1, (1, 4));
    assert_eq!(tabs((1, 4), "gk").1, (0, 15));
    assert_eq!(tabs((1, 3), "gk").1, (0, 14));
    assert_eq!(tabs((1, 4), "vgk").1, (0, 15));
    // From the tab, one column wide on its display row.
    assert_eq!(tabs((0, 14), "gj").1, (1, 3));
    assert_eq!(tabs((0, 14), "gk").1, (0, 5));
    assert_eq!(tabs((0, 14), "gjgk").1, (0, 14));
    assert_eq!(tabs((0, 14), "vgj").1, (1, 3));
    assert_eq!(
        tabs((0, 14), "dgj"),
        (format!("a {}DEFGHI", "b".repeat(12)), (0, 14))
    );
    assert_eq!(
        tabs((1, 4), "dgk"),
        (format!("a {}\tEFGHI", "b".repeat(12)), (0, 15))
    );
    // An operator over a count past the last display row changes nothing,
    // the caret going as far as it got.
    let text = "abcdefghijklmnopqrstuvwxy\nshort";
    for (cursor, keys, to) in [((0, 3), "d9gj", (1, 3)), ((1, 3), "y9gk", (0, 3))] {
        let mut vim = HeadlessVim::new(text, cursor);
        wrap(&mut vim, 10.0);
        vim.feed(keys).unwrap();
        assert_eq!(
            (vim.text(), vim.cursor(), vim.state().failed()),
            (text.to_string(), to, true),
            "{keys}"
        );
    }
}

/// `.` with a state cloned while its `cgj` session was open, which another
/// clone then ended: the repeat adopts that session's change before it
/// looks up the display rows, so it moves by display rows again.
#[test]
fn dot_adopts_a_closing_cgj_first() {
    let mut vim = HeadlessVim::new("abcdefghijklmnopqrstuvwxy\nshort", (0, 3));
    wrap(&mut vim, 10.0);
    vim.feed("cgjX").unwrap();
    let clone = Rc::new(RefCell::new(vim.state().clone()));
    let mut split = vim.model().split();
    split.set_cursor(vim.model().primary_selection().head());
    // `Esc` catches the model up twice, the second time once it's ending
    // the session: the clone ends it then.
    let calls = Rc::new(Cell::new(0));
    let measuring = {
        let (calls, clone) = (Rc::clone(&calls), Rc::clone(&clone));
        Measuring::new(move |_| {
            calls.set(calls.get() + 1);
            if calls.get() != 2 {
                return;
            }
            let mut clone = clone.borrow_mut();
            let normal_mode = Invocation::of("vim.normal_mode")
                .unwrap()
                .context(context(&clone));
            let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
            let mut target = VimTarget {
                model: &mut split,
                host: &mut host,
                state: &mut clone,
                registers: &mut registers,
            };
            registry().dispatch(&mut target, &normal_mode).unwrap();
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    let mut clone = Rc::try_unwrap(clone).ok().unwrap().into_inner();
    assert_eq!(vim.text(), "abcXnopqrstuvwxy\nshort");
    assert!(
        clone.last_change().is_none(),
        "the clone's ending is yet to be adopted"
    );
    let model = vim.model_mut();
    model.set_cursor(0);
    let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
    let invocation = commit(".").context(context(&clone));
    let mut target = VimTarget {
        model,
        host: &mut host,
        state: &mut clone,
        registers: &mut registers,
    };
    registry().dispatch(&mut target, &invocation).unwrap();
    assert_eq!(vim.text(), "Xtuvwxy\nshort");
}

/// The carets after `keys` from carets at `from`, as (row, column).
fn carets_after(text: &str, from: &[(u32, usize)], keys: &str) -> (String, Vec<(u32, usize)>) {
    let mut vim = HeadlessVim::new(text, from[0]);
    let offsets: Vec<usize> = from
        .iter()
        .map(|&(row, column)| vim.model().text().row_start(row) + column)
        .collect();
    vim.model_mut()
        .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
    vim.feed(keys).unwrap();
    let model = vim.model();
    let carets = model
        .selections()
        .iter()
        .map(|selection| {
            let point = model.text().offset_to_point(selection.head());
            (point.row, point.column as usize)
        })
        .collect();
    (vim.text(), carets)
}

/// With several carets, one whose motion fails part way goes where it got
/// to while the others' operator acts, through their edits; carets that
/// meet keep the goal a failed `gj` had.
#[test]
fn failing_part_way_among_carets() {
    let text = "abc\ndef\nghi\njkl\nmno";
    assert_eq!(
        carets_after(text, &[(1, 1), (3, 1)], "y2gk"),
        (text.into(), vec![(0, 1), (1, 1)])
    );
    assert_eq!(
        carets_after(text, &[(0, 1), (3, 1)], "d2gj"),
        ("ahi\njkl\nmno".into(), vec![(0, 1), (2, 1)])
    );
    assert_eq!(
        carets_after(text, &[(1, 1), (3, 1)], "d2gk"),
        ("abc\ndkl\nmno".into(), vec![(0, 1), (1, 1)])
    );
    assert_eq!(
        carets_after("\nab cd\nef gh", &[(1, 1), (2, 4)], "d2ge"),
        ("\nab c".into(), vec![(0, 0), (1, 3)])
    );
    assert_eq!(
        carets_after("\nab cd\nef gh", &[(1, 1), (2, 4)], "d2gE"),
        ("\nab c".into(), vec![(0, 0), (1, 3)])
    );
    // A failed `gj` keeps its goal beside an operator that acts.
    let five = "abcdef\nabcdef\nabcdef\nabcdef\nab";
    assert_eq!(
        carets_after(five, &[(0, 4), (3, 4)], "y2gjk").1,
        vec![(0, 4), (3, 4)]
    );
    assert_eq!(
        carets_after(five, &[(0, 4), (3, 4)], "d2gjk"),
        ("abcdef\nabcdef\nab".into(), vec![(0, 4), (1, 4)])
    );
    assert_eq!(
        carets_after(five, &[(0, 4), (3, 4)], ">2gjk").1.last(),
        Some(&(3, 4))
    );
    // Both stop on (2, 1) and merge, keeping the goal for `k`.
    let rows = "abcdef\nabcdef\nab";
    assert_eq!(
        carets_after(rows, &[(0, 4), (1, 4)], "d9gj"),
        (rows.into(), vec![(2, 1)])
    );
    assert_eq!(
        carets_after(rows, &[(0, 4), (1, 4)], "d9gjk"),
        (rows.into(), vec![(1, 4)])
    );
}

/// An operator over a motion that fails part way reports the caret it
/// moved as a change, so the host scrolls to it; so does `.` repeating one.
#[test]
fn failing_part_way_moves_are_changes() {
    let effect = |text: &str, cursor, setup: &str, keys: &str| {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.feed(setup).unwrap();
        let invocation = commit(keys).context(context(vim.state()));
        let dispatched = vim.dispatch(&invocation).unwrap();
        (dispatched.effect, vim.cursor(), vim.state().failed())
    };
    let rows = "abc\ndef\nghi";
    for keys in ["d9gj", "y9gj", "c9gj"] {
        assert_eq!(
            effect(rows, (0, 1), "", keys),
            (Effect::Changed, (2, 1), true),
            "{keys}"
        );
    }
    assert_eq!(
        effect(rows, (2, 1), "", "c9gk"),
        (Effect::Changed, (0, 1), true)
    );
    assert_eq!(
        effect("\nabc def\nxyz", (1, 4), "", "d3ge"),
        (Effect::Changed, (0, 0), true)
    );
    // At the start already, nothing moves.
    assert_eq!(
        effect("\nabc", (0, 0), "", "dge"),
        (Effect::Nothing, (0, 0), true)
    );
    assert_eq!(
        effect("\nabc def", (1, 5), "dge", "3."),
        (Effect::Changed, (0, 0), true)
    );
    assert_eq!(
        effect("abc\ndef\nghi\njkl", (0, 1), "dgj", "9."),
        (Effect::Changed, (2, 1), true)
    );
}

/// A g-operator over a `gj` or `gk` that fails having moved the caret
/// leaves it there keeping the goal, as nvim keeps curswant: alone, beside
/// a caret it acts for, and through `.`.
#[test]
fn g_operators_keep_a_failed_display_motions_goal() {
    let rows = "abcdef\nabcdef\nab";
    for keys in ["gu9gjk", "gq9gjk", "gw9gjk", "g?9gjk", "g~9gjk"] {
        assert_eq!(edited(rows, (0, 4), keys), (rows.into(), (1, 4)), "{keys}");
    }
    let up = "ab\nabcdef\nabcdef";
    assert_eq!(edited(up, (2, 4), "gU9gkj"), (up.into(), (1, 4)));
    let four = "abcdef\nabcdef\nabcdef\nab";
    assert_eq!(edited(four, (0, 4), "gugj9.k"), (four.into(), (2, 4)));
    // Beside a caret whose `gu` acts.
    let five = "abcdef\nabcdef\nabcdef\nabcdef\nab";
    assert_eq!(
        carets_after(five, &[(0, 4), (3, 4)], "gU2gjk"),
        (
            "abcdEF\nABCDEF\nABCDef\nabcdef\nab".into(),
            vec![(0, 4), (3, 4)]
        )
    );
}

/// A failed `gj` keeps its goal where the caret lands even when another
/// origin edits the text while the operator's edit is measured, moving the
/// carets past what it put in.
#[test]
fn a_failed_display_motions_goal_survives_edits_meanwhile() {
    let five = "abcdef\nabcdef\nabcdef\nabcdef\nab";
    let run = |keys: &str| {
        let mut vim = HeadlessVim::new(five, (0, 4));
        let offsets = [4, vim.model().text().row_start(3) + 4];
        vim.model_mut()
            .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
                range: offset..offset,
                reversed: false,
            }));
        let mut split = vim.model().split();
        let mut once = true;
        let measuring = Measuring::new(move |after_edit| {
            if after_edit && std::mem::take(&mut once) {
                let ai = fe_buffer::Origin::Ai {
                    agent: "agent".into(),
                    session: "session".into(),
                };
                split.buffer_mut().edit([(0..0, "X\n")], ai);
            }
        });
        vim.feed(keys).unwrap();
        drop(measuring);
        vim.feed("k").unwrap();
        let model = vim.model();
        let carets: Vec<(u32, usize)> = model
            .selections()
            .iter()
            .map(|selection| {
                let point = model.text().offset_to_point(selection.head());
                (point.row, point.column as usize)
            })
            .collect();
        (vim.text(), carets)
    };
    // The failed caret ends on the last row, one down for the row put in,
    // and `k` takes it up at its goal.
    let (text, carets) = run("d2gj");
    assert_eq!(text, "X\nabcdef\nabcdef\nab");
    assert_eq!(carets.last(), Some(&(2, 4)));
    let (text, carets) = run(">2gj");
    assert!(text.starts_with("X\n"), "{text}");
    assert_eq!(carets.last(), Some(&(4, 4)));
    let (text, carets) = run("gU2gj");
    assert_eq!(text, "X\nabcdEF\nABCDEF\nABCDef\nabcdef\nab");
    assert_eq!(carets.last(), Some(&(4, 4)));
}

/// `gj gk` into, out of and across an empty row, wrapped or not, keeping
/// the goal over it.
#[test]
fn display_rows_over_an_empty_row() {
    let text = "abc\n\nxyz";
    for soft_wrap in [false, true] {
        let run = |cursor, keys: &str| {
            let mut vim = HeadlessVim::new(text, cursor);
            if soft_wrap {
                wrap(&mut vim, 10.0);
            }
            vim.feed(keys).unwrap();
            (vim.text(), vim.cursor(), vim.visual_anchor())
        };
        let at = |cursor, keys: &str| run(cursor, keys).1;
        assert_eq!(at((0, 1), "gj"), (1, 0), "{soft_wrap}");
        assert_eq!(at((0, 1), "gjgj"), (2, 1), "{soft_wrap}");
        assert_eq!(at((2, 2), "gk"), (1, 0), "{soft_wrap}");
        assert_eq!(at((2, 2), "gkgk"), (0, 2), "{soft_wrap}");
        assert_eq!(at((1, 0), "gj"), (2, 0), "{soft_wrap}");
        assert_eq!(at((1, 0), "gk"), (0, 0), "{soft_wrap}");
        assert_eq!(at((0, 2), "2gj"), (2, 2), "{soft_wrap}");
        assert_eq!(run((0, 2), "vgjgj").1, (2, 2), "{soft_wrap}");
        assert_eq!(run((0, 1), "vgj").2, Some((0, 1)), "{soft_wrap}");
        assert_eq!(
            run((0, 1), "dgj"),
            ("a\n\nxyz".into(), (0, 0), None),
            "{soft_wrap}"
        );
        assert_eq!(
            run((1, 0), "dgj"),
            ("abc\nxyz".into(), (1, 0), None),
            "{soft_wrap}"
        );
        assert_eq!(
            run((2, 2), "dgk"),
            ("abc\nz".into(), (1, 0), None),
            "{soft_wrap}"
        );
    }
}

/// A tab wider on its display row than in its buffer row: its last column
/// there is a caret's on it, which goes on from that column and comes back
/// to it.
#[test]
fn display_rows_from_a_wider_tab() {
    let text = format!("a {}\tccdd\nABCDEFGHI", "b".repeat(13));
    let run = |cursor, keys: &str| {
        let mut vim = HeadlessVim::new(&text, cursor);
        wrap(&mut vim, 9.0);
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    assert_eq!(run((0, 15), "2gj").1, (1, 7));
    assert_eq!(run((1, 7), "2gk").1, (0, 15));
    assert_eq!(run((1, 7), "2gk2gj").1, (1, 7));
    assert_eq!(run((0, 15), "v2gj").1, (1, 7));
    assert_eq!(
        run((0, 15), "d2gj"),
        (format!("a {}HI", "b".repeat(13)), (0, 15))
    );
}

/// A counted `g_` counts a closed fold as one row (nvim 0.12.4: `2g_` from
/// the fold at rows 0-2 reaches (3, 8), and `d2g_` takes all four rows).
#[test]
fn counted_g_underscore_counts_a_closed_fold_as_a_row() {
    const FOLDED: &str = "abc\n  defghijkl\n  mnopq\nrstuvwxyz";
    let folded = |keys: &str| {
        let mut vim = HeadlessVim::new(FOLDED, (0, 0));
        vim.feed("zc").unwrap();
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor(), vim.state().failed())
    };
    assert_eq!(folded("2g_").1, (3, 8));
    assert_eq!(folded("v2g_").1, (3, 8));
    assert_eq!(folded("d2g_").0, "");
    // From the last visible row it fails; open, it steps through the rows.
    assert!(folded("G2g_").2);
    assert_eq!(after(FOLDED, (0, 0), "2g_"), ((1, 10), false));
}

/// An oversized count past a terminal closed fold goes to the buffer's end,
/// not the fold's header (nvim 0.12: `4g_` from (0, 0) with rows 1-3
/// folded reaches (3, 8), and `d4g_` takes all the text).
#[test]
fn oversized_g_underscore_past_a_terminal_fold_clamps_to_the_buffer_end() {
    const TEXT: &str = "abc\ndefghijkl\nmnopq\nrstuvwxyz";
    let folded = |keys: &str| {
        let mut vim = HeadlessVim::new(TEXT, (1, 0));
        vim.feed("zfjj").unwrap();
        vim.feed("zc").unwrap();
        vim.feed("gg").unwrap();
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    assert_eq!(folded("4g_").1, (3, 8));
    assert_eq!(folded("v4g_").1, (3, 8));
    assert_eq!(folded("d4g_").0, "");
    // Counts within the visible rows still land on the fold's header.
    assert_eq!(folded("2g_").1, (1, 8));
}
