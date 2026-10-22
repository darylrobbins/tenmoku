//! fe-vim's `:s` over rows where what a join leaves matters, against
//! recorded Neovim: `tests/corpus/substitute_multiline_joined.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes. One
//! test for each of T163's follow-ups, each over the cases whose names start
//! with its prefix.

#[allow(dead_code)]
mod support;

use support::{Recorded, check, load};

/// Checks the corpus's cases named `prefix…`, at least `min` of them.
fn matches_neovim(prefix: &str, min: usize) {
    let recorded: Vec<Recorded> = load("substitute_multiline_joined")
        .unwrap()
        .into_iter()
        .filter(|case| case.name.starts_with(prefix))
        .collect();
    assert!(recorded.len() >= min, "{prefix}: {} cases", recorded.len());
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "{prefix}: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

/// After a replacement joins rows, the search goes on in the joined line
/// as it now is: `^` and `\<` see where the join put the text, as in nvim
/// (`:%s/\n\|^b/X/g` on "a\nb" makes "aXbX").
#[test]
fn substitute_across_rows_matches_the_joined_line() {
    matches_neovim("joined_", 5);
}

/// With `c`, a row is matched as it was until a join, not with the
/// replacements already made on it (`:%s/a\|\<b\|\n/-/gc` and `a` on
/// "ab\ncd" make "-b-cd-"), answered `y` or `a`.
#[test]
fn substitute_confirm_keeps_its_matching_source() {
    matches_neovim("confirm_source_", 5);
}

/// `'<` and `'>` on rows joined through the last go past the last row of
/// the text the substitute leaves, its replacements' line breaks counted,
/// with `c` or without.
#[test]
fn substitute_visual_marks_past_a_join_through_eof() {
    matches_neovim("visual_", 10);
}

/// Matches that `\zs` starts on later rows join none of the rows between
/// them: their marks stay, as in nvim.
#[test]
fn substitute_zs_keeps_untouched_row_boundaries() {
    matches_neovim("zs_", 5);
}

/// A `:g` runs its `:s` on no row that `:s` joined away, through the last
/// row too, and `:s/\n//` is a join to it.
#[test]
fn global_substitute_skips_the_rows_it_joins() {
    matches_neovim("global_", 4);
}
