---
task_uid: "01m4ta26y8vfvjrgh5hsdvz3p3"
task: "T641"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test deletes_and_changes_on_an_emptied_buffer_write_no_register (tests/emptied_buffer_registers.rs, as given in the spec) passes: on a buffer a linewise delete emptied, dd, ., Vd, \"bdd, cc, S, ciw, Vc and \u003cC-v\u003ec write no register among \" 0-9 - a b, and i\u003cEsc\u003e, \u003e\u003e and cc\u003cEsc\u003e there keep it emptied"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test yanks_and_deletes_once_text_is_back_still_write passes: a yank still writes, a charwise delete to no text or a linewise change doesn't empty the buffer, and typed or undone text ends it"
    required: true
    manual: false
  - id: "C3"
    text: "The args kept for . are unchanged (only the register write is skipped); the undo/redo re-emptying limit is documented on VimState's new field; cargo nextest run -p fe-vim passes with every tests/corpus/*.expected.json byte-identical"
    required: true
    manual: false
---

# fe-vim: a delete or change on a buffer emptied by a linewise delete writes no register, as in nvim

## Problem

nvim parity (probed 2026-10-13 with `nvim --clean --headless -l crates/fe-vim/tests/support/oracle.lua`,
nvim 0.12.6, the oracle's own settings): once a linewise delete removes every row (`dd` on a one-row
buffer, `dG`, `Vjd` over all rows), nvim treats the buffer as having no rows at all, and until text goes
back in, a delete or change there takes nothing and writes **no** register: not `"`, not `1`-`9` (no shift),
not a named one. fe-vim instead takes the empty row it shows and writes `"\n"`:

- `"a"` at (0,0), `dddd`: nvim leaves `"`=`1`=`"a\n"`; fe-vim gives `"`=`1`=`"\n"`, `2`=`"a\n"`. The same for
  `dd.`, `ddVd`, `ddccx<Esc>`, `ddSx<Esc>`, `ddVcx<Esc>`, `dGdd` on `"a\nb"`.
- `dd"bdd`: nvim doesn't write `b`; fe-vim sets `b`=`"\n"`.
- `ddciwx<Esc>` and `dd<C-v>cx<Esc>`: nvim keeps `"`=`"a\n"`; fe-vim's empty inclusive change (the
  `Register::default()` write in `insert::change`, and the block path) clears `"`.
- Edits that change nothing (`i<Esc>`, `>>`, `cc<Esc>` on the emptied buffer) keep it emptied in nvim.

What doesn't empty it (fe-vim already matches nvim): a charwise delete that leaves no text (`xdd` on
`"a"` writes `"\n"`), a linewise change (`cc` keeps its row), and a yank (`ddyy` writes `"`=`0`=`"\n"`).
Text going back in (typed, put, or undone) ends it.

Register writes for `d` and `c` are in `crates/fe-vim/src/operator.rs` (`operate_unless_failing`, the
`registers.write` after `let write = ...`), `crates/fe-vim/src/insert.rs` (`change`, the
`registers.write` for `c`/`cc`/`S`/`ciw`/`Vc`) and `crates/fe-vim/src/visual/block.rs` (block `d`/`c`).

## Change

- `crates/fe-vim/src/state.rs`: `VimState` gains `emptied: Option<Version>`: the buffer's version when a
  linewise delete last left its text empty. The buffer counts as emptied while
  `model.snapshot().version()` still equals it (fe-buffer drops edits that change nothing, so `i<Esc>`,
  `>>` and `cc<Esc>` there keep the version; any edit that puts text in moves it on).
- `crates/fe-vim/src/operator.rs`: in `operate_unless_failing`'s `Operator::Delete` arm, after `apply`,
  when an op is linewise and the text is now empty, set `state.emptied` to the model's current version
  (whether or not the delete made an edit, so `dd` on an already-empty buffer empties it too). Where it
  writes the registers for a delete, write nothing while the buffer is emptied.
- `crates/fe-vim/src/insert.rs` (`change`) and `crates/fe-vim/src/visual/block.rs`: likewise, a change
  or block delete/change writes no register while the buffer is emptied. The text edits, the mode, the
  marks and the args kept for `.` don't change: only the register write is skipped.
- Yanks are untouched. Document the rule on the new field.

Out of scope (known limits, noted in that doc comment): nvim also counts the buffer as emptied again
when `u` or `<C-r>` returns it to a state a linewise delete emptied (`ddix<Esc>udd` and `ddu<C-r>dd`
write nothing in nvim); a version check can't see that. `:d` (in `ex.rs`, held by T629) is not
touched.

Tests go in the new `crates/fe-vim/tests/emptied_buffer_registers.rs`, through `fe_vim::HeadlessVim`;
every value below was probed in nvim. Use this file as written (it fails at the base on its first test):

```rust
//! A delete or change on a buffer that a linewise delete emptied writes no
//! register, as in nvim: the buffer has no rows left, so there's nothing to
//! take. A yank still writes, and the buffer stops being emptied once text
//! goes back in. Each expectation was probed with `nvim --clean --headless`.

use fe_vim::HeadlessVim;

/// The non-empty registers among `" 0-9 - a b`, as (name, text).
fn registers(vim: &HeadlessVim) -> Vec<(char, String)> {
    "\"0123456789-ab"
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            (!register.text.is_empty()).then(|| (name, register.text.clone()))
        })
        .collect()
}

/// Feeds `keys` to a vim on `buffer` at (0, 0) and checks the text and the
/// registers.
fn check(buffer: &str, keys: &str, text: &str, want: &[(char, &str)]) {
    let mut vim = HeadlessVim::new(buffer, (0, 0));
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

#[test]
fn deletes_and_changes_on_an_emptied_buffer_write_no_register() {
    let a = [('"', "a\n"), ('1', "a\n")];
    check("a", "dddd", "", &a);
    check("a", "dd.", "", &a);
    check("a", "ddVd", "", &a);
    check("a", "dd\"bdd", "", &a);
    check("a", "ddccx<Esc>", "x", &a);
    check("a", "ddSx<Esc>", "x", &a);
    check("a", "ddciwx<Esc>", "x", &a);
    check("a", "ddVcx<Esc>", "x", &a);
    check("a", "dd<C-v>cx<Esc>", "x", &a);
    // Edits that change nothing leave it emptied.
    check("a", "ddi<Esc>dd", "", &a);
    check("a", "dd>>dd", "", &a);
    check("a", "ddcc<Esc>dd", "", &a);
    // Any linewise delete of every row empties it.
    check("a\nb", "dGdd", "", &[('"', "a\nb\n"), ('1', "a\nb\n")]);
    check("a\nb", "Vjddd", "", &[('"', "a\nb\n"), ('1', "a\nb\n")]);
    check("", "dd\"add", "", &[('"', "\n"), ('1', "\n")]);
}

#[test]
fn yanks_and_deletes_once_text_is_back_still_write() {
    // A yank writes.
    check("a", "ddyy", "", &[('"', "\n"), ('0', "\n"), ('1', "a\n")]);
    // A charwise delete that leaves no text doesn't empty the buffer.
    check("a", "xdd", "", &[('"', "\n"), ('1', "\n"), ('-', "a")]);
    check("a\nb", "vj$ddd", "", &[('"', "\n"), ('1', "\n"), ('2', "a\nb")]);
    // Nor does a linewise change, which keeps its row.
    check("a", "cc<Esc>dd", "", &[('"', "\n"), ('1', "\n"), ('2', "a\n")]);
    // Text put back in ends it, typed or undone.
    check(
        "a",
        "ddix<Esc>dd",
        "",
        &[('"', "x\n"), ('1', "x\n"), ('2', "a\n")],
    );
    check("a", "ddudd", "", &[('"', "a\n"), ('1', "a\n"), ('2', "a\n")]);
}
```
