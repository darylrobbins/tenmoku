//! fe-vim's `/`, `?` and `:s` over composing characters: a base and the
//! marks after it are one character, as in nvim. The oracle half runs
//! `tests/corpus/composing.toml` against the expectations the ignored
//! recorder in `tests/oracle.rs` writes.

#[allow(dead_code)]
mod support;

use fe_text::Text;
use fe_vim::HeadlessVim;
use fe_vim::search::{FindQuery, Pattern, matches_in};
use support::{check, load};

const ACUTE: &str = "\u{301}";

fn run(buffer: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(buffer, cursor);
    vim.feed(keys).unwrap();
    vim
}

fn substituted(buffer: &str, command: &str) -> String {
    run(buffer, (0, 0), &format!("{command}<CR>")).text()
}

#[test]
fn composing_substitute_takes_whole_characters() {
    let text = format!("a{ACUTE}b");
    let accented = format!("a{ACUTE}");
    assert_eq!(substituted(&text, ":s/./X/g"), "XX");
    assert_eq!(
        substituted(&text, r":s/\(.\)/[\1]/g"),
        format!("[{accented}][b]")
    );
    assert_eq!(substituted(&text, ":s/./<&>/g"), format!("<{accented}><b>"));
    let vim = run(&text, (0, 0), ":s/a/X/<CR>");
    assert_eq!(vim.text(), text);
    assert_eq!(vim.state().message(), Some("E486: Pattern not found: a"));

    let vim = run(&format!("a{ACUTE}b a"), (0, 0), "/a<CR>");
    assert_eq!(vim.cursor(), (0, 5));
}

#[test]
fn composing_matches_never_split_a_character() {
    let text = format!("e{ACUTE}\u{323}a{ACUTE} {ACUTE}b\nx{ACUTE}");
    let is_mark = |ch: char| ('\u{300}'..='\u{36f}').contains(&ch);
    // A boundary: before a non-mark, or at the text's end.
    let boundary = |at: usize| !text[at..].chars().next().is_some_and(is_mark);
    let patterns = [
        ".",
        r"\a",
        r"\w",
        r"\S",
        "[a-z]",
        "[^ ]",
        "x*",
        ".*",
        r"a\|a.",
        r".\{2}",
        r"\(.\)\(.\)",
    ];
    for pattern in patterns {
        let parsed = Pattern::parse(pattern).unwrap();
        for range in matches_in(&Text::new(&text), 0..2, &parsed, usize::MAX) {
            assert!(
                boundary(range.start) && boundary(range.end),
                "{pattern}: {range:?} splits a character"
            );
        }
        // `:s` wraps every group (the whole match, then each `\(…\)`) in
        // guillemets; none may be followed by a mark.
        let groups = pattern.matches(r"\(").count();
        let replacement = if groups == 0 {
            "«&»".to_string()
        } else {
            (1..=groups).map(|n| format!("«\\{n}»")).collect()
        };
        let vim = run(&text, (0, 0), &format!(":%s/{pattern}/{replacement}/g<CR>"));
        let result = vim.text();
        for (at, ch) in result
            .char_indices()
            .filter(|&(_, ch)| ch == '«' || ch == '»')
        {
            let after = &result[at + ch.len_utf8()..];
            assert!(
                !after.chars().next().is_some_and(is_mark),
                "{pattern}: a group splits a character in {result:?}"
            );
        }
    }

    // A find bar's regex is the `regex` crate's, over scalars.
    let query = FindQuery {
        text: ".".into(),
        regex: true,
        match_case: true,
        whole_word: false,
    };
    let find = Pattern::from_query(&query).unwrap();
    let marks = format!("a{ACUTE}");
    assert_eq!(
        matches_in(&Text::new(&marks), 0..1, &find, usize::MAX),
        [0..1, 1..3]
    );
}

#[test]
fn composing_matches_neovim() {
    let recorded = load("composing").unwrap();
    assert!(recorded.len() >= 16, "composing: {} cases", recorded.len());
    for name in [
        "dot_global",
        "literal_base_not_found",
        "capture_whole_character",
        "whole_match_ampersand",
        "class_alpha",
        "collection_range",
        "collection_of_marks",
        "stacked_marks_dot",
        "literal_with_mark",
        "leading_mark",
        "search_slash_skips_composed",
        "search_question_backward",
        "search_n_whole_characters",
        "dot_star",
        "empty_match_global",
        "ignore_case_composed",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "composing: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "composing: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}
