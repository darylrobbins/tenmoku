---
task_uid: "01m4saeqgcr04k3yx6k4gwk4v2"
task: "T97"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test join_raw_matches_neovim: tests/corpus/join_raw.toml has at least 20 cases recorded from nvim 0.12.4 and all agree, covering gJ, 3gJ, a count past the last row, gJ and 3gJ on the last row, indented next rows (indent kept), rows ending or starting with blanks, an empty row, a row starting with ), multibyte and wide text, v, V and ctrl-v gJ over one and three rows, . after gJ and after visual gJ, u after gJ, and dgJ cancelling"
    required: true
    manual: false
  - id: "C2"
    text: "J, visual J and :j! behave as before: every earlier corpus expected file is unchanged and its oracle test passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-vim passes; no file outside crates/fe-vim changes; no Cargo.toml or Cargo.lock change; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: gJ joins rows without adding or removing spaces, counted, in visual mode and with dot-repeat, checked against nvim (Mac T575)

## Problem

M7 vim parity: fe-vim has `J` (`vim.join`, `crates/fe-vim/src/edit.rs` `join`) and `:j!` (`ex/lines.rs`), but not
`gJ` (`:help gJ`: "Join [count] lines, with a minimum of two lines. Don't insert or remove any spaces."). `g` then
`J` cancels today (no feature's `g` hook answers it). `gJ` is the everyday way to join without the space `J`
adds (string literals, URLs split over rows), in normal and visual mode.

## Change

- `crates/fe-vim/src/edit.rs`: `join_raw` beside `join` (same row walk, carets, last-row and count rules), but
  each joined row keeps its indentation and no space goes in or comes out: the line break alone is deleted. The
  caret lands where nvim's does (probe; nvim puts it at the last join point). `join`'s signature stays as it is
  (`ex/lines.rs`, held by T566, calls it).
- `crates/fe-vim/src/repeat.rs`: `Change::Join` carries `JoinArgs { count, raw: bool }` (`raw` serde-default false,
  skipped when false) instead of `CountArgs`, so `.` repeats `gJ` raw and `ex/normal.rs`'s existing
  `Change::Join(_)` arm keeps compiling unchanged; `Change::invocation` gives `vim.join_raw` for a raw join.
- `crates/fe-vim/src/g_operators.rs`: its `g` hook answers `J`: with nothing pending, `vim.join_raw {"count"}`
  (registered by its feature's `register`, `mutates_buffer` true, normal mode); in a visual mode,
  `Pending::operate(Operator::JoinRaw, Target::Selection)`; with an operator pending (`dgJ`) it cancels.
- `crates/fe-vim/src/args.rs`: `Operator::JoinRaw`, handled beside `Operator::Join` in
  `crates/fe-vim/src/visual/operate.rs` (same rows and landing, raw join) and `crates/fe-vim/src/visual/block.rs`
  (as `Join`: no register written). No new arm in `operator.rs`, `insert.rs` or `marks.rs` (held by T420); if the
  compiler demands one there, block for a respec instead.
- Known limit, said in `edit.rs`'s doc: `ex/normal.rs`'s `joins` (held by T566) doesn't know `vim.join_raw` or
  `Operator::JoinRaw`, so `:g` row tracking after a `:normal gJ` follows bytes only. The lead files the one-line
  follow-up once T566 lands.

Clean room as every M7 card: fit behaviour from `nvim --clean` probes and `:help` only.

## Tests

New corpus `crates/fe-vim/tests/corpus/join_raw.toml` with its recorded `join_raw.expected.json` (nvim 0.12.4, the
ignored recorder in `tests/oracle.rs`), checked by new `crates/fe-vim/tests/join_raw_oracle.rs`, shaped like
`tests/macros_oracle.rs`.
