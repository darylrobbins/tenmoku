---
task_uid: "01m4tf6kc6k7dcbmfz6shyw302"
task: "T649"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test a_register_before_v_is_dropped_and_visual_mode_starts (tests/register_before_visual.rs, as given in the spec) passes; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Only crates/fe-vim/src/reducer.rs changes under src: v V \u003cC-v\u003e \u003cC-q\u003e start their mode when nothing but a register is pending, a count before them behaves as today, and the doc comment above step states the rule"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with every tests/corpus/*.expected.json byte-identical"
    required: true
    manual: false
---

# fe-vim: a register named before v, V or <C-v> is dropped and the visual mode starts, as in nvim

## Problem

nvim parity (probed 2026-10-13 with `nvim --clean --headless -l crates/fe-vim/tests/support/oracle.lua`,
nvim 0.12.6): a register named before `v`, `V`, `<C-v>` or `<C-q>` is dropped and the visual mode starts
as usual; the operator that later ends the selection writes the registers it would with no name given
(even `"_` is forgotten), and a register named inside the mode (`"av"bd`) still counts. fe-vim instead
cancels: in `step` (`crates/fe-vim/src/reducer.rs`), `'v' | 'V' if pending.is_empty()` and
`VimKey::Ctrl('v' | 'q') if pending.visual || pending.is_empty()` don't match once a register is
pending, so the key is rejected and the keys after it run in normal mode, sometimes destructively.
On `"one two\nthree four\nfive"` at (0,0):

- `"av`, `"aV`, `"a<C-v>`: nvim is in visual / visual_line / visual_block mode anchored at (0,0); fe-vim
  stays in normal mode.
- `"avd`: nvim deletes `o` (`"`=`-`=`o`); fe-vim leaves `d` pending. `"aVd`, `"avjd`, `"a<C-v>jd`,
  `"_vd` likewise.
- `"aviwy`: nvim yanks `one`; fe-vim types `wy` at (0,0) in insert mode (`i` inserts, `w` is typed).
- `"ayiwwyiwb"avep`: nvim puts the unnamed register (`two`) over `one`; fe-vim puts `two` after the
  caret in normal mode.

Already right (keep): a register before `i o R J ~ % w G gg >> gJ gUU m` `<C-a>` (dropped or used as
in nvim), and `v"a…` inside a visual mode (`visual_key` drops a register before `v`/`V`).

## Change

- `crates/fe-vim/src/reducer.rs`, `step`: start the visual mode for `v`, `V`, `<C-v>` and `<C-q>` when
  nothing but a register is pending (no count, no operator, no prefix), dropping the register, as for
  nothing pending. A count before them is unchanged (that's `1v`'s reselect, a feature of its own).
  A prototype of exactly this (a `Pending::at_most_register` helper used by both guards) passed the
  test below and every other fe-vim test at e6988aba. Update the doc comment above `step` with the rule.
- Add `crates/fe-vim/tests/register_before_visual.rs`, as written below (it fails at the base on its first
  assert):

```rust
//! A register named before `v`, `V` or `<C-v>` is dropped and the visual
//! mode starts, as in nvim: the operator that ends the selection writes
//! the registers it would with no name given. Each expectation was probed
//! with `nvim --clean --headless` (0.12.6).

use fe_vim::HeadlessVim;

const TEXT: &str = "one two\nthree four\nfive";

/// The non-empty registers among `" 0-9 - a b`, as (name, text, block
/// width).
fn registers(vim: &HeadlessVim) -> Vec<(char, String, Option<usize>)> {
    "\"0123456789-ab"
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            (!register.text.is_empty())
                .then(|| (name, register.text.clone(), register.block_width))
        })
        .collect()
}

/// Feeds `keys` to a vim on [`TEXT`] at (0, 0) and checks the text, the
/// mode and the registers.
fn check(keys: &str, text: &str, mode: &str, want: &[(char, &str, Option<usize>)]) {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed(keys).unwrap();
    let want: Vec<(char, String, Option<usize>)> = want
        .iter()
        .map(|(name, text, width)| (*name, text.to_string(), *width))
        .collect();
    assert_eq!(
        (vim.text(), vim.mode(), registers(&vim)),
        (text.to_string(), mode, want),
        "{keys:?}"
    );
}

#[test]
fn a_register_before_v_is_dropped_and_visual_mode_starts() {
    // The mode starts, anchored at the caret.
    for (keys, mode) in [
        ("\"av", "visual"),
        ("\"aV", "visual_line"),
        ("\"a<C-v>", "visual_block"),
        ("\"a<C-q>", "visual_block"),
    ] {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(
            (vim.mode(), vim.visual_anchor(), vim.text()),
            (mode, Some((0, 0)), TEXT.to_string()),
            "{keys:?}"
        );
    }
    // The operator ending the selection writes as if no register was named.
    check("\"avd", "ne two\nthree four\nfive", "normal", &[
        ('"', "o", None),
        ('-', "o", None),
    ]);
    check("\"aVd", "three four\nfive", "normal", &[
        ('"', "one two\n", None),
        ('1', "one two\n", None),
    ]);
    check("\"avjd", "hree four\nfive", "normal", &[
        ('"', "one two\nt", None),
        ('1', "one two\nt", None),
    ]);
    check("\"a<C-v>jd", "ne two\nhree four\nfive", "normal", &[
        ('"', "o\nt", Some(1)),
        ('1', "o\nt", Some(1)),
    ]);
    // Even the black hole: `"_` before `v` is forgotten too.
    check("\"_vd", "ne two\nthree four\nfive", "normal", &[
        ('"', "o", None),
        ('-', "o", None),
    ]);
    check("\"aviwy", TEXT, "normal", &[('"', "one", None), ('0', "one", None)]);
    check("\"av$y", TEXT, "normal", &[
        ('"', "one two\n", None),
        ('0', "one two\n", None),
    ]);
    // A register named inside the mode still counts.
    check("\"av\"bd", "ne two\nthree four\nfive", "normal", &[
        ('"', "o", None),
        ('b', "o", None),
    ]);
    // A visual put takes the unnamed register, not the one before `v`.
    check("\"ayiwwyiwb\"avep", "two two\nthree four\nfive", "normal", &[
        ('"', "one", None),
        ('0', "two", None),
        ('-', "one", None),
        ('a', "one", None),
    ]);
}
```
