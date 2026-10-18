//! Remaps under `.`, macros and `:normal` (`docs/decisions/vim-remaps.md`
//! §5), through `HeadlessVim`: typed keys are recorded, an expansion's
//! change is what `.` repeats, and `:normal!` skips the maps.

use std::sync::Arc;

use fe_editor::commands::HeadlessHost;
use fe_vim::{HeadlessVim, Register, Remap, RemapMode, RemapSet, Write};

const N: RemapMode = RemapMode::Normal;

fn remap(from: &str, to: &str, recursive: bool) -> Remap {
    Remap {
        modes: vec![N],
        from: from.into(),
        to: to.into(),
        recursive,
    }
}

fn vim(text: &str, cursor: (u32, usize), remaps: &[Remap]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    let (set, issues) = RemapSet::new(remaps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(Arc::new(set));
    vim
}

fn register(vim: &HeadlessVim, name: char) -> String {
    vim.registers().get(name).unwrap().text.clone()
}

/// What a host does once the timeout passes with keys held.
fn time_out(vim: &mut HeadlessVim) {
    vim.state_mut().remap_timeout();
    for key in vim.state_mut().take_replay() {
        let text = match key {
            fe_vim::VimKey::Char(ch) => ch.to_string(),
            other => panic!("no notation for {other:?}"),
        };
        vim.feed(&text).unwrap();
    }
}

#[test]
fn remap_repeat_records_typed_keys() {
    let text = "  one two\n  three four";
    let mut vim = vim(text, (1, 6), &[remap("H", "^", false)]);
    vim.feed("qaHq").unwrap();
    assert_eq!(vim.cursor(), (1, 2));
    assert_eq!(register(&vim, 'a'), "H");

    // A held prefix's keys are recorded once, however it resolves.
    let mut vim = self::vim(
        "ab cd ef",
        (0, 0),
        &[remap("gsfb", "x", false), remap("gs", "l", false)],
    );
    vim.feed("qbgsf").unwrap();
    assert!(vim.state().remap_waiting());
    vim.feed("cq").unwrap();
    assert_eq!(register(&vim, 'b'), "gsfc");
    assert_eq!(vim.cursor(), (0, 3));
    vim.feed("qcgsfbq").unwrap();
    assert_eq!(register(&vim, 'c'), "gsfb");
    vim.feed("qdgs").unwrap();
    assert!(vim.state().remap_waiting());
    time_out(&mut vim);
    vim.feed("q").unwrap();
    assert_eq!(register(&vim, 'd'), "gs");

    // A replay plays through the map as it is when the replay runs.
    let mut vim = self::vim(text, (1, 6), &[remap("H", "^", false)]);
    vim.feed("qaHq").unwrap();
    let (set, _) = RemapSet::new(&[remap("H", "$", false)], "\\");
    vim.state_mut().set_remaps(Arc::new(set));
    vim.feed("@a").unwrap();
    assert_eq!(vim.cursor(), (1, 11));
    assert_eq!(register(&vim, 'a'), "H");
}

#[test]
fn remap_repeat_dot() {
    let mut vim = vim(
        "one two three four five",
        (0, 0),
        &[remap("X", "dw", false), remap("Y", "y$", false)],
    );
    vim.feed("X").unwrap();
    assert_eq!(vim.text(), "two three four five");
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "three four five");
    // A yank makes no change: `.` repeats the one before it.
    vim.feed("Y").unwrap();
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "four five");
    vim.feed("2.").unwrap();
    assert_eq!(vim.text(), "");
}

#[test]
fn remap_repeat_in_macros() {
    // The expansion runs, then the macro's own later keys, in order.
    let mut vim = vim("a b c d e f", (0, 0), &[remap("X", "dw", false)]);
    vim.feed("qaXxq").unwrap();
    assert_eq!(vim.text(), " c d e f");
    assert_eq!(register(&vim, 'a'), "Xx");
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), " d e f");

    // An expansion's failing key stops the macro.
    let mut vim = self::vim("abc\ndef", (0, 0), &[remap("Z", "jjx", false)]);
    set_register(&mut vim, 'a', "ZiA\u{1b}");
    vim.feed("@a").unwrap();
    assert_eq!(vim.text(), "abc\ndef");
    assert_eq!(vim.mode(), "normal");
}

/// Puts `text` in register `name`, as a yank would.
fn set_register(vim: &mut HeadlessVim, name: char, text: &str) {
    let register = Register::new(text, false);
    let mut host = HeadlessHost::new();
    vim.registers_mut()
        .write(Some(name), Write::Yank, register, &mut host)
        .unwrap();
}

#[test]
fn remap_repeat_normal_bang() {
    let maps = [remap("H", "x", false)];
    let mut vim = vim("abc\ndef", (0, 1), &maps);
    vim.feed(":normal H<CR>").unwrap();
    assert_eq!(vim.text(), "ac\ndef");
    let mut vim = self::vim("abc\ndef", (0, 1), &maps);
    vim.feed(":normal! H<CR>").unwrap();
    // Built-in `H` moves to the top of the window.
    assert_eq!(vim.text(), "abc\ndef");
    // A prefix held when the keys end resolves, as a timeout would.
    let mut vim = self::vim(
        "abc\ndef",
        (0, 0),
        &[remap("gsfb", "x", false), remap("gs", "l", false)],
    );
    vim.feed(":normal gs<CR>").unwrap();
    assert!(!vim.state().remap_waiting());
    assert_eq!(vim.cursor(), (0, 1));
}

#[test]
fn remap_repeat_delayed_replay_jump_not_recorded() {
    // A replayed jump held for a longer map and released by a timeout
    // isn't typed: the recording keeps `@a` alone.
    let mut vim = vim("x ab", (0, 0), &[remap("gsabc", "x", false)]);
    vim.set_jump_on_s(true);
    set_register(&mut vim, 'a', "gsab");
    vim.feed("qb@a").unwrap();
    assert!(vim.state().remap_waiting());
    time_out(&mut vim);
    vim.feed("q").unwrap();
    assert_eq!(register(&vim, 'b'), "@a");
}

#[test]
fn remap_repeat_normal_from_a_map_keeps_outer_keys() {
    // `:normal`'s own keys have a tag stream: the inner `H` doesn't eat the
    // outer expansion's trailing `x`.
    let maps = [remap("X", ":normal H<CR>x", false), remap("H", "l", false)];
    let mut vim = vim("abcd", (0, 0), &maps);
    vim.feed("qaXq").unwrap();
    assert_eq!(register(&vim, 'a'), "X");
    assert_eq!(vim.text(), "acd");
    assert_eq!(vim.mode(), "normal");
    assert!(!vim.state().remap_waiting());
}

#[test]
fn remap_repeat_normal_escape_skips_maps() {
    let o = |from: &str, to: &str| Remap {
        modes: vec![RemapMode::OperatorPending],
        ..remap(from, to, false)
    };
    let mut vim = vim("one two", (0, 0), &[o("<Esc>", "w")]);
    vim.feed(":normal d<CR>").unwrap();
    assert_eq!(vim.text(), "one two");
    assert_eq!(vim.cursor(), (0, 0));
    let mut vim = self::vim("one two", (0, 0), &[o("<Esc>x", "w")]);
    vim.feed(":normal d<CR>").unwrap();
    assert!(!vim.state().remap_waiting());
    assert_eq!(vim.text(), "one two");
}
