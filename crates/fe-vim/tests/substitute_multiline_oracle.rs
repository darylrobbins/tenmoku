//! fe-vim's `:s` with a pattern that matches across rows (`:%s/,\n/, /`,
//! `:%s/\n//`, flags, ranges, marks, undo and the `c` prompt over rows)
//! against recorded Neovim: `tests/corpus/substitute_multiline.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes;
//! and the counts it reports.

#[allow(dead_code)]
mod support;

use fe_vim::HeadlessVim;
use support::{check, load};

#[test]
fn substitute_multiline_matches_neovim() {
    let recorded = load("substitute_multiline").unwrap();
    assert!(
        recorded.len() >= 12,
        "substitute_multiline: {} cases",
        recorded.len()
    );
    // What the card asks the corpus to cover, by case name.
    for name in [
        "join_comma_rows",
        "join_all_rows",
        "newline_to_r_splits",
        "g_flag_across",
        "n_flag_counts",
        "range_end_past",
        "marks_on_joined_rows",
        "undo_restores",
        "single_row_unchanged",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "substitute_multiline: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "substitute_multiline: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// What `:s` reports, as nvim 0.12.4 reports it typed (probed by hand: the
/// recorder doesn't read messages): a match over rows counted once, on the
/// line it starts on, `n` ending a line's count with it; replaced, the
/// rows it joins are one line.
#[test]
fn substitute_multiline_reports_nvims_counts() {
    for (buffer, keys, report) in [
        ("a,\nb,\nc,\nd", ":%s/,\\n/, /n<CR>", "3 matches on 3 lines"),
        ("a\nb\nc\nd", ":%s/\\n//n<CR>", "4 matches on 4 lines"),
        (
            "a,\nb,\nc\nd,\ne",
            ":%s/,\\n/, /<CR>",
            "3 substitutions on 2 lines",
        ),
        (
            "x x,\ny x,\nz x",
            ":%s/x,\\n\\|x/Q/<CR>",
            "3 substitutions on 2 lines",
        ),
        (
            "a,b,\nc,d",
            ":%s/,\\n\\|,/\\r/g<CR>",
            "3 substitutions on 1 line",
        ),
        ("a\nb\nc\nd", ":%s/\\n//<CR>", "3 substitutions on 1 line"),
        (
            "a,\nb,\nc,\nd",
            ":%s/,\\n/;/c<CR>a",
            "3 substitutions on 1 line",
        ),
    ] {
        let mut vim = HeadlessVim::new(buffer, (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.state().message(), Some(report), "{keys} on {buffer:?}");
    }
}
