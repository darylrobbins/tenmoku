---
task_uid: "01m4tdntg12p57r9stmw4gyv5x"
task: "T642"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test dot_after_a_numbered_register_change_takes_the_next_register (tests/repeat_numbered_register.rs, as given in the spec) passes: \"1x.., \"1X., \"1dw., \"1Dj., \"1dd., \"1dd2., \"1cw/\"1s/\"1cc with ., vl\"1d., V\"1d. and \"8dd.. leave nvim's registers, and \"9dd. stays on 9"
    required: true
    manual: false
  - id: "C2"
    text: "repeat in crates/fe-vim/src/repeat.rs bumps a Change::Operate register 1-8 as it does a Change::Put one, its doc comment says so, and no other src file changes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with every tests/corpus/*.expected.json byte-identical"
    required: true
    manual: false
---

# fe-vim: . after a delete or change into a numbered register 1-8 takes the next register, as in nvim

## Problem

nvim parity (probed 2026-10-13 with `nvim --clean --headless -l crates/fe-vim/tests/support/oracle.lua`,
nvim 0.12.6, the oracle's own settings): when `.` repeats a change that named a numbered register `1`-`8`,
nvim repeats it with the *next* register (`:help redo-register`; nvim's `start_redo` bumps any redo
register `1`-`8`, `9` stays), and the bumped one is what a later `.` repeats. fe-vim does this only for a
put: `repeat` in `crates/fe-vim/src/repeat.rs` bumps `Change::Put`'s register and leaves
`Change::Operate`'s alone, so a delete or change repeated by `.` rewrites the same register:

- `"abcdef"` at (0,0), `"1x..`: nvim `1`=`a`, `2`=`b`, `3`=`c`; fe-vim `1`=`c` only.
- `"a b c d"`, `"1dw.`: nvim `1`=`"a "`, `2`=`"b "`; fe-vim `1`=`"b "`.
- `"a\nb\nc\nd\ne\nf"`, `"1dd.`: nvim `1`=`b\n`, `2`=`a\n`, `3`=`b\n`; fe-vim `1`=`2`=`b\n`, `3`=`a\n`.
- The same for `"1X.`, `"1Dj.`, `"1dd2.`, `"1cwx<Esc>w.`, `"1sx<Esc>l.`, `"1ccx<Esc>j.`, visual
  `vl"1d.` and `V"1d.`, and `"8dd..` (`8` then `9`, then `9` again). The buffer text already matches;
  only the registers differ.

Already right (keep): `"1p.`/`"1p..` (put), `"9dd.` (stays `9`), and the register-write order for a named
numbered delete (write, then shift), which `"2dd.` shows once the bump is in.

## Change

- `crates/fe-vim/src/repeat.rs`, `repeat`: bump the register of a `Change::Operate` from `1`-`8` to the
  next, exactly as it already does for `Change::Put` (one or-pattern on the same `if let`), and update the
  doc comment ("A put or an operator from register `1`–`8` takes the next one, and `.` remembers that").
  The replayed operator records its own args as the last change (`operator.rs` `operate`,
  `visual/operate.rs`, `insert.rs` on closing a `c` session), so a later `.` takes the one after; no
  other file changes. A prototype of exactly this passed the test below and all of
  `cargo nextest run -p fe-vim` (391 tests) at e6988aba.
- Add `crates/fe-vim/tests/repeat_numbered_register.rs`, as written below (it fails at the base on its
  first check, `"1x..`):

```rust
//! `.` after a change made with a numbered register `1`-`8` named takes the
//! next register, as in nvim (`:help redo-register`): not only for a put but
//! for a delete or change too. Each expectation was probed with
//! `nvim --clean --headless` (0.12.6).

use fe_vim::HeadlessVim;

/// The non-empty registers among `" 0-9 -`, as (name, text).
fn registers(vim: &HeadlessVim) -> Vec<(char, String)> {
    "\"0123456789-"
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            (!register.text.is_empty()).then(|| (name, register.text.clone()))
        })
        .collect()
}

/// Feeds `keys` to a vim on `buffer` at `cursor` and checks the text and the
/// registers.
fn check(buffer: &str, cursor: (u32, usize), keys: &str, text: &str, want: &[(char, &str)]) {
    let mut vim = HeadlessVim::new(buffer, cursor);
    vim.feed(keys).unwrap();
    let want: Vec<(char, String)> = want
        .iter()
        .map(|(name, text)| (*name, text.to_string()))
        .collect();
    assert_eq!(
        (vim.text(), registers(&vim)),
        (text.to_string(), want),
        "{keys:?} on {buffer:?}"
    );
}

const ROWS: &str = "a\nb\nc\nd\ne\nf";

#[test]
fn dot_after_a_numbered_register_change_takes_the_next_register() {
    check(
        "abcdef",
        (0, 0),
        "\"1x..",
        "def",
        &[('"', "c"), ('1', "a"), ('2', "b"), ('3', "c")],
    );
    check(
        "abcdef",
        (0, 5),
        "\"1X.",
        "abcf",
        &[('"', "d"), ('1', "e"), ('2', "d")],
    );
    check(
        "a b c d",
        (0, 0),
        "\"1dw.",
        "c d",
        &[('"', "b "), ('1', "a "), ('2', "b ")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1Dj.",
        "\n\nc\nd\ne\nf",
        &[('"', "b"), ('1', "a"), ('2', "b")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1dd.",
        "c\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n"), ('3', "b\n")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1dd2.",
        "d\ne\nf",
        &[
            ('"', "b\nc\n"),
            ('1', "b\nc\n"),
            ('2', "a\n"),
            ('3', "b\nc\n"),
        ],
    );
    check(
        "ab cd ef",
        (0, 0),
        "\"1cwx<Esc>w.",
        "x x ef",
        &[('"', "cd"), ('1', "ab"), ('2', "cd")],
    );
    check(
        "abcdef",
        (0, 0),
        "\"1sx<Esc>l.",
        "xxcdef",
        &[('"', "b"), ('1', "a"), ('2', "b")],
    );
    check(
        ROWS,
        (0, 0),
        "\"1ccx<Esc>j.",
        "x\nx\nc\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n"), ('3', "b\n")],
    );
    // Over a selection too.
    check(
        "abcdef",
        (0, 0),
        "vl\"1d.",
        "ef",
        &[('"', "cd"), ('1', "ab"), ('2', "cd")],
    );
    check(
        ROWS,
        (0, 0),
        "V\"1d.",
        "c\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n"), ('3', "b\n")],
    );
    // `8` goes to `9`, and `9` stays.
    check(
        ROWS,
        (0, 0),
        "\"8dd..",
        "d\ne\nf",
        &[('"', "c\n"), ('1', "c\n"), ('2', "b\n"), ('3', "a\n")],
    );
    check(
        ROWS,
        (0, 0),
        "\"9dd.",
        "c\nd\ne\nf",
        &[('"', "b\n"), ('1', "b\n"), ('2', "a\n")],
    );
}
```
