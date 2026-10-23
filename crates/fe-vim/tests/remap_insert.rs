//! Remaps in insert, replace and cmdline mode (`docs/decisions/vim-remaps.md`
//! §3–§5), through `HeadlessVim`: `VimState::remap_insert` asked before each
//! key those modes type. The expected texts are nvim's, for the same
//! `:imap`/`:cmap` lines and keys.

use std::sync::Arc;

use fe_command::ContextFrame;
use fe_vim::{HeadlessVim, Remap, RemapMode, RemapSet, VimKey};

const N: RemapMode = RemapMode::Normal;
const I: RemapMode = RemapMode::Insert;
const C: RemapMode = RemapMode::CmdLine;

fn noremap(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

fn map(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        recursive: true,
        ..noremap(modes, from, to)
    }
}

fn set(remaps: &[Remap]) -> Arc<RemapSet> {
    let (set, issues) = RemapSet::new(remaps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    Arc::new(set)
}

fn vim(text: &str, remaps: &[Remap]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.state_mut().set_remaps(set(remaps));
    vim
}

fn notation(key: VimKey) -> String {
    match key {
        VimKey::Char('<') => "<lt>".into(),
        VimKey::Char(ch) => ch.into(),
        VimKey::Esc => "<Esc>".into(),
        VimKey::Enter => "<CR>".into(),
        other => panic!("no notation for {other:?}"),
    }
}

/// What a host does once `CHORD_TIMEOUT` passes with no key.
fn time_out(vim: &mut HeadlessVim) {
    vim.state_mut().remap_timeout();
    for key in vim.state_mut().take_replay() {
        vim.feed(&notation(key)).unwrap();
    }
}

fn after(text: &str, remaps: &[Remap], keys: &str) -> (String, &'static str) {
    let mut vim = vim(text, remaps);
    vim.feed(keys).unwrap();
    (vim.text(), vim.mode())
}

#[test]
fn inoremap_jk_ends_insert() {
    let jk = [noremap(&[I], "jk", "<Esc>")];
    // nvim: `ijkx` on "abc" leaves "bc" (the `x` deletes in normal mode).
    assert_eq!(after("abc", &jk, "ijkx"), ("bc".into(), "normal"));
    // A broken prefix types the `j` and goes on.
    assert_eq!(after("", &jk, "ijxk"), ("jxk".into(), "insert"));
}

#[test]
fn expansion_types_its_text() {
    let m = [noremap(&[I], "teh", "the")];
    assert_eq!(after("", &m, "iteh <Esc>"), ("the ".into(), "normal"));
}

#[test]
fn imap_chains_and_inoremap_does_not() {
    let chain = [map(&[I], "a", "b"), map(&[I], "b", "c")];
    assert_eq!(after("", &chain, "iaX").0, "cX");
    let flat = [noremap(&[I], "a", "b"), noremap(&[I], "b", "c")];
    assert_eq!(after("", &flat, "iaX").0, "bX");
}

#[test]
fn held_prefix_then_a_breaking_key() {
    let m = [noremap(&[I], "ab", "X"), noremap(&[I], "abc", "Y")];
    let mut vim = vim("", &m);
    vim.feed("ia").unwrap();
    assert!(vim.state().remap_waiting());
    let frame = vim.state().stamp(ContextFrame::new("editor"));
    assert!(frame.has("vim.pending"));
    assert_eq!(frame.get("vim.pending_kind"), Some("remap"));
    assert_eq!(vim.text(), "");
    vim.feed("b").unwrap();
    assert!(vim.state().remap_waiting());
    vim.feed("q").unwrap();
    assert!(!vim.state().remap_waiting());
    // nvim: `iabq` is "Xq".
    assert_eq!(vim.text(), "Xq");
    // No map at all: the first key is typed and the rest match again.
    let m = [noremap(&[I], "bc", "Z"), noremap(&[I], "bcd", "W")];
    assert_eq!(after("", &m, "iabx").0, "abx");
    assert_eq!(after("", &m, "iabcx").0, "aZx");
}

#[test]
fn timeout_resolves_a_held_prefix() {
    let m = [noremap(&[I], "ab", "X"), noremap(&[I], "abc", "Y")];
    let mut vim = vim("", &m);
    vim.feed("iab").unwrap();
    time_out(&mut vim);
    assert_eq!(vim.text(), "X");
    // With no shorter map, the typed keys.
    let m = [noremap(&[I], "abc", "Y")];
    let mut vim = self::vim("", &m);
    vim.feed("iab").unwrap();
    assert!(vim.state().remap_waiting());
    time_out(&mut vim);
    assert_eq!(vim.text(), "ab");
    assert!(!vim.state().remap_waiting());
}

#[test]
fn replace_mode_takes_insert_maps() {
    let jk = [noremap(&[I], "jk", "<Esc>")];
    // nvim: `Rjkx` on "xyz" leaves "yz".
    assert_eq!(after("xyz", &jk, "Rjkx"), ("yz".into(), "normal"));
    let m = [noremap(&[I], "q", "QQ")];
    assert_eq!(after("abcd", &m, "Rq<Esc>").0, "QQcd");
}

#[test]
fn cnoremap_on_the_command_line() {
    let m = [noremap(&[C], "jk", "<Esc>"), noremap(&[C], "zz", "s/a/b/")];
    let mut vim = vim("aaa", &m);
    vim.feed(":zz<CR>").unwrap();
    assert_eq!(vim.text(), "baa");
    // `<Esc>` cancels the line.
    vim.feed(":jk").unwrap();
    assert_eq!(vim.mode(), "normal");
    // An insert map doesn't apply there, nor a cmdline one in insert.
    assert_eq!(after("", &[noremap(&[C], "x", "y")], "ix").0, "x");
}

#[test]
fn set_remaps_while_a_prefix_is_held() {
    let m = [noremap(&[I], "ab", "X")];
    let mut vim = vim("", &m);
    vim.feed("ia").unwrap();
    assert!(vim.state().remap_waiting());
    vim.state_mut().set_remaps(set(&[]));
    assert!(!vim.state().remap_waiting());
    for key in vim.state_mut().take_replay() {
        vim.feed(&notation(key)).unwrap();
    }
    vim.feed("b").unwrap();
    assert_eq!(vim.text(), "ab");
}

#[test]
fn cycle_is_e223() {
    let cycle = [map(&[I], "a", "b"), map(&[I], "b", "a")];
    let mut vim = vim("", &cycle);
    vim.feed("ia").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.state().message(), Some("E223: recursive mapping"));
    assert_eq!(vim.text(), "");
    assert!(vim.state_mut().take_replay().is_empty());
    vim.feed("c").unwrap();
    assert!(!vim.state().failed());
    assert_eq!(vim.text(), "c");
}

#[test]
fn cross_mode_self_prefix() {
    // nvim: `nmap ab abx | imap b B`, `ab` on "" gives "Bx".
    let m = [map(&[N], "ab", "abx"), map(&[I], "b", "B")];
    let mut vim = vim("", &m);
    vim.feed("ab").unwrap();
    assert_eq!((vim.text().as_str(), vim.mode()), ("Bx", "insert"));
}

#[test]
fn macro_holds_typed_keys() {
    let m = [noremap(&[I], "xy", "Q")];
    let mut vim = vim("one\ntwo", &m);
    vim.feed("qaixy<Esc>jq").unwrap();
    assert_eq!(vim.text(), "Qone\ntwo");
    assert_eq!(vim.registers().get('a').unwrap().text, "ixy\u{1b}j");
    vim.feed("gg@a").unwrap();
    assert_eq!(vim.text(), "QQone\ntwo");
}

#[test]
fn normal_runs_insert_maps_and_normal_bang_does_not() {
    let m = [noremap(&[I], "xy", "Q")];
    let mut vim = vim("", &m);
    vim.feed(":normal ixy<CR>").unwrap();
    assert_eq!(vim.text(), "Q");
    let mut vim = self::vim("", &m);
    vim.feed(":normal! ixy<CR>").unwrap();
    assert_eq!(vim.text(), "xy");
    // A prefix held when the keys end resolves as a timeout does.
    let m = [noremap(&[I], "xyz", "Q")];
    let mut vim = self::vim("", &m);
    vim.feed(":normal ixy<CR>").unwrap();
    assert_eq!(vim.text(), "xy");
    assert!(!vim.state().remap_waiting());
}
