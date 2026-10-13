---
task_uid: "01m4skv64pt62spj7ab465c79s"
task: "T103"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test ex_global_join_raw_matches_neovim: tests/corpus/ex_global_join_raw.toml has at least 12 cases recorded from nvim 0.12.4 and all agree, covering :g/^/normal gJ from an empty row, :g/x/normal gJx, :g/^/normal 2gJ, :v/x/normal gJ, indented next rows (indent kept), gJ on the last row, :g/^/normal VjgJ, vjgJ and ctrl-v jgJ under :g, gJ:g/^/normal . and u after a :g gJ"
    required: true
    manual: false
  - id: "C2"
    text: "every earlier fe-vim corpus expected file is unchanged and its oracle test passes (ex_global_matches_neovim and T575's join_raw_matches_neovim among them)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-vim passes; cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no file outside crates/fe-vim changes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: :g follows its rows through gJ joins as through J: :g/pat/normal gJ, visual gJ and . after gJ, checked against nvim (Mac T612)

## Problem

`:g` follows its marked rows as a command run on each changes the buffer (`crates/fe-vim/src/ex/global.rs`).
When a `:normal` keystroke joins rows, `crates/fe-vim/src/ex/normal.rs` notes it as `global::Change::Joined`
(`run_invocation`, around line 223), so each line break taken out joins the row after it to the one before and the
mark on that row goes, as nvim does. Which invocations count as joins is decided by `joins()` (normal.rs, around
line 250): `vim.join` (`J`), `vim.operate` with `Operator::Join` (visual `J`), and `.` repeating either.

Mac T575 (gJ: join without adding or removing spaces, counted, visual and dot-repeatable; landed on neon as T97,
arrives on the Mac's main with stack 6) adds gJ. Unless gJ goes through exactly those shapes, `:g/pat/normal gJ`,
`:g/pat/normal VjgJ` and `:g/pat/normal .` after a gJ are tracked as plain edits (`edited(…, false)`), which can
leave a mark on a row a join consumed and run the command on it again, unlike nvim.

## Change

- `crates/fe-vim/src/ex/normal.rs`, `joins()`: also true for each way T575 dispatches gJ, whatever shape it took
  (a `vim.join` argument, a new command id, a new `Operator`, or a `g_operators` entry): normal `gJ` and `{count}gJ`,
  visual `gJ` (v, V and ctrl-v), and `.` whose last change is any of those. Update its doc comment to name gJ.
- If T575's gJ edit differs from J's in a way `global.rs`'s `Joined` handling mis-tracks (it keeps leading blanks,
  so its edit spans only the line break), fix it there too; otherwise `global.rs` doesn't change.

## Tests

New corpus `crates/fe-vim/tests/corpus/ex_global_join_raw.toml` with its recorded
`ex_global_join_raw.expected.json` (from nvim 0.12.4 by the ignored recorder in `crates/fe-vim/tests/oracle.rs`,
which records every corpus file), and new file `crates/fe-vim/tests/ex_global_join_raw_oracle.rs`, a copy of
`ex_global_oracle.rs`'s shape for this corpus. Don't add the cases to `ex_global.toml`, which other fe-vim cards
edit.

## Decided by the lead overnight

- **Oracle, not unit tests**: nvim is the reference for `:g`'s row tracking; the cases mirror `ex_global.toml`'s
  J cases (`global_normal_join_from_empty_row`, `global_normal_visual_join`, `global_normal_repeat_join`, the
  `Jx` case) with gJ, plus indented rows, which are where gJ and J differ.
- **Separate corpus file** so this card conflicts with no other fe-vim corpus card.
- Depends on T575.
