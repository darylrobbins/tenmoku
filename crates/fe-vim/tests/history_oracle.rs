//! fe-vim's command-line histories: `<Up>` and `<Down>` on `/`, `?` and `:`
//! lines, against recorded Neovim (`tests/corpus/history.toml` and its
//! expectations, which the ignored recorder in `tests/oracle.rs` writes),
//! and [`Registers`]' histories themselves.

#[allow(dead_code)]
mod support;

use fe_vim::{CommandLineKind, HISTORY_MAX, HeadlessVim, HistoryRecall, Registers};
use support::{check, load};

use CommandLineKind::{Ex, SearchBackward, SearchForward};

#[test]
fn history_matches_neovim() {
    let recorded = load("history").unwrap();
    assert!(recorded.len() >= 28, "history: {} cases", recorded.len());
    // What the card asks the corpus to cover, by case name.
    for name in [
        "up",
        "up_up",
        "prefix",
        "prefix_down_back_to_typed",
        "down_back_to_nothing_typed",
        "question_shares_slash",
        "repeated_search_moves_to_newest",
        "ex_up",
        "ex_prefix",
        "offset_recalled",
        "question_recalls_an_offset",
        "esc_cancelled_search_recalled",
        "esc_cancelled_ex_recalled",
        "ctrl_c_cancelled_search_recalled",
        "star_pattern_recalled",
        "hash_pattern_recalled",
        "substitute_pattern_recalled",
        "global_pattern_recalled",
        "address_pattern_recalled",
    ] {
        assert!(
            recorded.iter().any(|case| case.name == name),
            "history: no case {name}"
        );
    }
    let failures: Vec<String> = recorded
        .iter()
        .filter_map(|case| check(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "history: {} of {} cases differ from nvim:\n{}",
        failures.len(),
        recorded.len(),
        failures.join("\n")
    );
}

fn history(registers: &Registers, kind: CommandLineKind) -> Vec<&str> {
    registers.history(kind).collect()
}

/// Each recall of `typed` from the line as typed, older and older.
fn recalls(registers: &Registers, kind: CommandLineKind, typed: &str) -> Vec<String> {
    let mut from = usize::MAX;
    let mut found = Vec::new();
    while let Some((at, entry)) = registers.recall(kind, typed, from, true) {
        found.push(entry.into_owned());
        from = at;
    }
    found
}

#[test]
fn registers_keep_a_search_and_an_ex_history() {
    let mut registers = Registers::new();
    registers.add_history(SearchForward, "foo");
    registers.add_history(SearchBackward, "bar");
    registers.add_history(Ex, "s/x/y/");
    registers.add_history(SearchForward, "far");
    // `/` and `?` share one, newest last; `:` has its own.
    assert_eq!(history(&registers, SearchForward), ["foo", "bar", "far"]);
    assert_eq!(history(&registers, SearchBackward), ["foo", "bar", "far"]);
    assert_eq!(history(&registers, Ex), ["s/x/y/"]);

    // A repeated line moves to the end, once.
    registers.add_history(SearchForward, "foo");
    assert_eq!(history(&registers, SearchForward), ["bar", "far", "foo"]);
    // An empty line adds nothing.
    registers.add_history(SearchForward, "");
    registers.add_history(Ex, "");
    assert_eq!(history(&registers, SearchForward), ["bar", "far", "foo"]);
    assert_eq!(history(&registers, Ex), ["s/x/y/"]);
    // The same text typed after the other delimiter is another entry, as
    // in nvim.
    registers.add_history(SearchBackward, "foo");
    assert_eq!(
        history(&registers, SearchForward),
        ["bar", "far", "foo", "foo"]
    );
}

#[test]
fn a_history_keeps_its_newest_entries() {
    let mut registers = Registers::new();
    for n in 0..=HISTORY_MAX {
        registers.add_history(Ex, &n.to_string());
    }
    let ex = history(&registers, Ex);
    assert_eq!(ex.len(), HISTORY_MAX);
    assert_eq!(ex.first(), Some(&"1"));
    assert_eq!(ex.last(), Some(&HISTORY_MAX.to_string().as_str()));
    assert_eq!(history(&registers, SearchForward), Vec::<&str>::new());
}

#[test]
fn recall_steps_by_prefix() {
    let mut registers = Registers::new();
    for line in ["foo", "bar", "far", "fab"] {
        registers.add_history(SearchForward, line);
    }
    assert_eq!(
        recalls(&registers, SearchForward, ""),
        ["fab", "far", "bar", "foo"]
    );
    assert_eq!(
        recalls(&registers, SearchForward, "f"),
        ["fab", "far", "foo"]
    );
    assert_eq!(recalls(&registers, SearchForward, "fa"), ["fab", "far"]);
    assert_eq!(
        recalls(&registers, SearchForward, "q"),
        Vec::<String>::new()
    );
    assert_eq!(recalls(&registers, Ex, ""), Vec::<String>::new());
    // Newer, from an entry; none past the newest.
    let newer = |typed, from| {
        registers
            .recall(SearchForward, typed, from, false)
            .map(|(at, entry)| (at, entry.into_owned()))
    };
    assert_eq!(newer("f", 0), Some((2, "far".to_string())));
    assert_eq!(newer("fo", 0), None);
    assert_eq!(newer("", 3), None);
    assert_eq!(newer("", usize::MAX), None);
}

#[test]
fn recall_swaps_a_search_entrys_delimiters() {
    let mut registers = Registers::new();
    registers.add_history(SearchForward, r"a?b/e");
    registers.add_history(SearchForward, r"a\/b[/]");
    registers.add_history(SearchBackward, r"c/d?s-1");
    let backward = recalls(&registers, SearchBackward, "");
    assert_eq!(backward, [r"c/d?s-1", r"a\/b[?]", r"a\?b?e"]);
    let forward = recalls(&registers, SearchForward, "");
    assert_eq!(forward, [r"c\/d/s-1", r"a\/b[/]", r"a?b/e"]);
    // The prefix is matched against the entry as it was typed.
    assert_eq!(recalls(&registers, SearchBackward, "a?"), [r"a\?b?e"]);
}

#[test]
fn history_recall_goes_back_to_what_was_typed() {
    let mut registers = Registers::new();
    for line in ["foo", "bar", "far"] {
        registers.add_history(SearchForward, line);
    }
    let mut recall = HistoryRecall::default();
    let step = |recall: &mut HistoryRecall, line: &str, older| {
        recall.step(&registers, SearchForward, line, older)
    };
    assert_eq!(step(&mut recall, "f", true).as_deref(), Some("far"));
    assert_eq!(step(&mut recall, "far", true).as_deref(), Some("foo"));
    assert_eq!(step(&mut recall, "foo", true), None);
    assert_eq!(step(&mut recall, "foo", false).as_deref(), Some("far"));
    assert_eq!(step(&mut recall, "far", false).as_deref(), Some("f"));
    assert_eq!(step(&mut recall, "f", false).as_deref(), Some("f"));
    // An edit makes the line the prefix, from the entry shown.
    assert_eq!(step(&mut recall, "f", true).as_deref(), Some("far"));
    recall.edited();
    assert_eq!(step(&mut recall, "fo", true).as_deref(), Some("foo"));
    assert_eq!(step(&mut recall, "foo", false).as_deref(), Some("fo"));
}

#[test]
fn submitted_and_cancelled_lines_go_into_the_history() {
    let mut vim = HeadlessVim::new("x foo bar\ny", (0, 0));
    // A failing line is kept, as in nvim.
    vim.feed("/foo<CR>/zzz<CR>?bar<CR>:s/q/r/<CR>").unwrap();
    // A line cancelled with <Esc> is kept, an empty one isn't; the pattern of
    // `:s` is another entry.
    vim.feed("/cancelled<Esc>:cancelled<Esc>/<CR>:<CR>")
        .unwrap();
    assert_eq!(
        history(vim.registers(), SearchForward),
        ["foo", "zzz", "bar", "q", "cancelled"]
    );
    assert_eq!(history(vim.registers(), Ex), ["s/q/r/", "cancelled"]);
    // Each line starts at the end of its history; a recalled line cancelled
    // moves to the end.
    vim.feed("/<Up><Up><Esc>/<Up><CR>").unwrap();
    assert_eq!(vim.registers().last_search(), Some(("q", true)));
}

#[test]
fn star_global_and_address_patterns_go_into_the_search_history() {
    let mut vim = HeadlessVim::new("x foo\nbar foo\nfoo bar", (0, 0));
    vim.feed("w*").unwrap();
    vim.feed(":g/bar/s/o/0/<CR>:/zzz/d<CR>").unwrap();
    // The last pattern a line set, once, whatever it ran.
    assert_eq!(
        history(vim.registers(), SearchForward),
        ["\\<foo\\>", "o", "zzz"]
    );
    assert_eq!(history(vim.registers(), Ex), ["g/bar/s/o/0/", "/zzz/d"]);
}

#[test]
fn searches_submitted_inside_an_ex_line_wait_for_it_to_end() {
    for ex in [":g/foo/normal @a<CR>", ":/foo/normal @a<CR>"] {
        // Register `a` holds two searches (the second typed on `?`), then an
        // empty one, which adds nothing and doesn't clear the second.
        let mut vim = HeadlessVim::new("zed zip\nfoo", (0, 0));
        vim.feed("qa/zed<CR>?zip<CR>/<CR>q/new<CR>j0").unwrap();
        assert_eq!(
            history(vim.registers(), SearchForward),
            ["zed", "zip", "new"]
        );
        vim.feed(ex).unwrap();
        // Only the second is added again, the newest.
        let searches = history(vim.registers(), SearchForward);
        assert_eq!(searches, ["zed", "new", "zip"], "{ex}");
        assert_eq!(
            recalls(vim.registers(), SearchBackward, ""),
            ["zip", "new", "zed"],
            "{ex}"
        );
    }
}
