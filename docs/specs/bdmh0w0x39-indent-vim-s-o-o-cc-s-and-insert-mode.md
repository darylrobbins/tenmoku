---
task_uid: "01m4xewd82x6b36wbdmh0w0x39"
task: "T120"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "auto_indent: with a host suggesting +1 level after a { row, o on that row and insert-mode Enter at its end open an indented row; O on the row after opens one at the suggested level; 3o indents each copy"
    required: true
    manual: false
  - id: "C2"
    text: "auto_indent: cc and S on an over-indented row inside a block re-indent the emptied row to the suggestion; without a suggestion they keep the row's indentation as today"
    required: true
    manual: false
  - id: "C3"
    text: "auto_indent: an opened row left empty at Esc loses its indentation, as nvim's autoindent does, and . repeats o with the suggested indentation"
    required: true
    manual: false
  - id: "C4"
    text: "every existing fe-vim oracle test (no language, so no suggestion) still passes unchanged against nvim"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim and cargo clippy -p fe-vim --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Indent vim's o, O, cc, S and insert-mode Enter by language, through the editor's indent hook, unchanged where there is no language (Mac T521)

## Problem

Vim's `o`, `O`, `cc`, `S` and insert-mode Enter indent by copying the row before (`Rows` in crates/fe-vim/src/
insert.rs:497; `VimState::line_break`, crates/fe-vim/src/state.rs:219, calls `EditorModel::newline`). The previous
card makes the editor's newline language-aware through `EditorHost::suggest_indent`; vim should indent the same
way, as nvim does with a filetype's indentexpr.

## Change

- `crates/fe-vim/src/state.rs`: insert-mode Enter types its line break through `EditorModel::newline_in(host)`.
- `crates/fe-vim/src/insert.rs`: `o` and `O` open their rows at `host.suggest_indent`'s indentation for the new
  row, falling back to today's copy; a count's copies each get it. Indentation left unused when the session ends
  is removed as today (nvim's autoindent rule).
- `crates/fe-vim/src/operator.rs`: linewise `cc` and `S` keep the row's indentation as today, or with a
  suggestion re-indent the emptied row to it.
- Typing a closer in insert mode outdents as in the plain editor (it goes through `EditorModel::insert`).
- Vim has no `=` operator in fe-vim yet; this card doesn't add one (noted as a gap in insert.rs's module docs).
- With no language (every oracle corpus case), behaviour is unchanged and still matches nvim.

## Tests

New file `crates/fe-vim/tests/auto_indent.rs`, with a host answering fixed suggestions.

## Implementation notes

- Insert-mode `Enter` by language is `VimState::line_break_in(model, host)`, which types through
  `EditorModel::newline_in`; `VimState::line_break(model)` still copies, for hosts without syntax. The app's
  `Enter` and multi-key text input (fe-gpui `vim::line_break`, `vim::type_as_keys`) and `:normal`'s `<CR>`
  (`ex/normal.rs`) pass their host to `line_break_in`. The scope was widened for this after review round 1
  (fe-gpui `vim.rs`, `commands.rs`, `vim/auto_indent_tests.rs`, fe-vim `ex/normal.rs`, `repeat.rs`).
- fe-gpui's `ViewHost::closer_outdents` no longer opts out under vim, so a `}` typed in insert mode lines up
  with its opener as nvim's 'indentkeys' do (vim still doesn't pair). The editor's outdent rewrites the row from
  its start through the closer, so the session keeps the new blanks as its indentation (`Indents::follow`) and
  typing begins past them (`Indents::past`): `.` records `}`, not the blanks. `.` replays typed text through
  `insert::type_at_carets`, which now lines up a row it starts with a closer, as the host says.
- `newline_in` may land in two batches (the break, then the re-indent), so the session notes a line break typed
  through `line_break_with` as every batch from the version before it to the version after it
  (`insert::Break::Many`); replace mode's single-batch note is unchanged (`Break::One`). A line break batch's rows
  of blanks are all indentation, so the caret row between a split pair (`{|}`) still loses its indentation at `Esc`.
- `o`, `O` and linewise `c` (`cc`, `S`, `cj`) open or empty their rows as before, then ask the host for each
  such row in the text the opening left, and re-indent it in a batch of its own inside the session's
  transaction, before the session starts following the buffer (`insert::indent_opened`). The suggestion becomes
  text as the editor's newline makes it: basis row's indentation plus indent units, or fewer columns going out.
  The marks take the re-indent as `o`'s `Put` note, or for `c` as setting no mark.
- A count's copies of `o`/`O` go in as before, then the rows they open are re-indented from the host at `Esc`,
  top to bottom, each from the rows above as re-indented (`insert::indent_copies`), so a copy ending with `{`
  nests the next. Only a row's opened indentation is replaced: blanks typed after it stay. A row the host has no
  suggestion for keeps the width of the row before as re-indented. What re-indenting adds counts towards
  `MAX_PUT` as the copies do (per caret); the copies it has no room for are taken out again, as `reopened`
  leaves out those that don't fit.
- With no suggestion (every oracle case) no extra batch is made, so behaviour is unchanged.
- Review round 2: measuring text as the model catches up may edit the buffer through another model, so every
  follow-on re-indent is planned from the text as it goes in. `o`/`O`/linewise `c` re-indent in the same buffer
  borrow as their opening (`insert::open_indented`). A count's copies and `.`'s replayed closers are planned
  inside `operator::apply_planned`. The copies are re-indented only if no edit since their batch wrote inside
  them or at their edges, where blanks would run on into a blank last copy's (`insert::untouched`, from `BufferSnapshot::edits_since`), their range moved through the edits around
  them; `.`'s typed text is anchored. In-crate regression tests (`insert/indent_tests.rs`, scope widened for it) put a row above while
  measuring.
- The closer outdent is recognised by its shape (a typed edit rewriting a row from its start into blanks and one
  non-blank char: `insert::outdent`), not by indentation the session already tracked there, and whatever
  indentation the row had, none included. `.`'s replay outdents with the same shape (indentation and closer in one
  edit), so the `}` it types is still all `.` records, and repeating it onto a statement types `}` alone.
