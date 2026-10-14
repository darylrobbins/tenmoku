//! `gu gU g~ g?`, and visual `u U ~ g?`, set `'[` and `']` to what they
//! cover, as nvim does: the first and last character of a charwise span,
//! or the first row's column 0 and the last row's last byte over rows,
//! whether or not they change anything. One that changes nothing leaves `'.`
//! where it was. Each expectation was probed with `nvim --clean --headless`
//! (0.12.6).

use fe_vim::HeadlessVim;

const TEXT: &str = "abc DEF 12\n  x Y }\n}\n\nend.";

/// Feeds `keys` to a vim on [`TEXT`] at `cursor` and checks `'[` and `']`.
fn check(cursor: (u32, usize), keys: &str, start: (u32, usize), end: (u32, usize)) {
    let mut vim = HeadlessVim::new(TEXT, cursor);
    vim.feed(keys).unwrap();
    assert_eq!(
        (vim.mark('['), vim.mark(']')),
        (Some(start), Some(end)),
        "{keys:?} from {cursor:?}"
    );
}

#[test]
fn case_operators_set_change_marks_over_what_they_cover() {
    // Over rows: the first row's column 0 to the last row's last byte.
    check((0, 0), "gUU", (0, 0), (0, 9));
    check((0, 4), "guu", (0, 0), (0, 9));
    check((0, 0), "g~~", (0, 0), (0, 9));
    check((0, 0), "g??", (0, 0), (0, 9));
    check((0, 0), "gUj", (0, 0), (1, 6));
    check((1, 2), "gUj", (1, 0), (2, 0));
    check((1, 2), "gUk", (0, 0), (1, 6));
    check((0, 0), "gUip", (0, 0), (2, 0));
    check((0, 0), "gUap", (0, 0), (3, 0));
    check((1, 2), "3gUU", (1, 0), (3, 0));
    // Charwise: the first and the last character covered.
    check((0, 0), "gUw", (0, 0), (0, 3));
    check((0, 0), "gUaw", (0, 0), (0, 3));
    check((0, 4), "gU$", (0, 4), (0, 9));
    check((0, 4), "gU0", (0, 0), (0, 3));
    check((1, 2), "gUb", (0, 8), (1, 1));
    check((1, 2), "gUe", (1, 2), (1, 4));
    // Changing nothing ("DEF" is already upper case, "abc" lower).
    check((0, 4), "gUiw", (0, 4), (0, 6));
    check((0, 4), "gUw", (0, 4), (0, 7));
    check((0, 4), "gUiW", (0, 4), (0, 6));
    check((0, 0), "guiw", (0, 0), (0, 2));
    check((1, 2), "guiw", (1, 2), (1, 2));
    check((0, 4), "gUiwj.", (1, 4), (1, 4));
    // Visual: the selection's ends, or its rows.
    check((0, 0), "vjgU", (0, 0), (1, 0));
    check((0, 0), "vjU", (0, 0), (1, 0));
    check((0, 4), "vjU", (0, 4), (1, 4));
    check((1, 2), "vjg?", (1, 2), (2, 0));
    check((0, 0), "veu", (0, 0), (0, 2));
    check((0, 0), "VjgU", (0, 0), (1, 6));
    check((0, 4), "Vj~", (0, 0), (1, 6));
    check((1, 2), "Vj~", (1, 0), (2, 0));
}

#[test]
fn a_case_operator_that_changes_nothing_keeps_the_dot_mark() {
    // `x` on the second row sets `'.` there; the case operator after it
    // changes nothing, so `'.` stays.
    for keys in ["jxk0guiw", "jxk0wgUiw", "jxk0wgUw", "jxk$gUb", "jxk0vegu"] {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.mark('.'), Some((1, 0)), "{keys:?}");
    }
}

#[test]
fn a_visual_selection_starting_on_a_line_break_marks_the_row_end() {
    // `v$o` starts the selection on the first row's line break; `'[` clamps
    // to the row's last byte, with the text changed or not.
    for text in ["abc\nDEF", "ABC\nDEF"] {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.feed("v$ojU").unwrap();
        assert_eq!(vim.mark('['), Some((0, 2)), "{text:?}");
    }
}
