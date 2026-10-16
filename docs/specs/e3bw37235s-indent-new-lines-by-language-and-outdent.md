---
task_uid: "01m4xew660gjqa5pe3bw37235s"
task: "T119"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "auto_indent_tests::newline: with a host suggesting +1 level, editor.newline after { indents the new row one unit (tabs or the detected space width), at every caret, as one undo step; with no suggestion it copies the indentation as today"
    required: true
    manual: false
  - id: "C2"
    text: "auto_indent_tests::between_pair: Enter between { and } puts } on its own row at the opener's indentation and the caret on an indented row between"
    required: true
    manual: false
  - id: "C3"
    text: "auto_indent_tests::closer: typing } as the first non-blank on an over-indented row re-indents the row to closer_outdents' row in the same edit, so one undo removes both"
    required: true
    manual: false
  - id: "C4"
    text: "auto_indent_host_tests: in a real Rust file Enter after fn main() { indents, and in Python Enter after def f(): indents one level"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-editor -p fe-gpui and cargo clippy -p fe-editor -p fe-gpui --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Indent new lines by language and outdent typed closers in the editor, through an EditorHost indent hook answered from the view's syntax (Mac T520)

## Problem

`EditorModel::newline` (crates/fe-editor/src/lib.rs:992) copies the row's indentation, whatever the language.
The previous card adds `fe_syntax::suggest_indent` and `closer_outdents`; the editor doesn't ask for them.

## Change

- `EditorHost` (crates/fe-editor/src/commands.rs:27) gets `fn suggest_indent(&self, text: &Text, row: u32) ->
  Option<IndentSuggestion>` and `fn closer_outdents(&self, text: &Text, row: u32) -> Option<u32>`, both
  defaulting to `None` (a fe-editor copy of the suggestion type, so fe-editor still doesn't depend on fe-syntax).
- New `crates/fe-editor/src/auto_indent.rs`: `EditorModel::newline_in(&mut self, host: &dyn EditorHost)`, which
  `editor.newline` uses: each caret's new row gets the suggestion's indentation, in the buffer's indent unit (tabs
  or the space width, from T458's detection, else the tab size); with `None` it copies as `newline` does today.
  Between a pair (`{|}`), the closer goes to its own row at the opener's indentation and the caret sits on an
  indented row between. `EditorModel::newline` stays for hosts without syntax.
- Typing a char that leaves it the row's first non-blank and `closer_outdents` answers: the row is re-indented to
  that row's indentation in the same edit, so one undo removes both.
- `ViewHost` (`crates/fe-gpui/src/commands.rs`) answers both from the view's `Syntax` through fe-syntax (none while
  it loads).

Not in this card: vim's `o`, `O`, `cc`, `S` and insert-mode Enter (the next card).

## Tests

New file `crates/fe-editor/src/auto_indent_tests.rs`, declared from auto_indent.rs, with a test host answering
fixed suggestions, plus one fe-gpui test in a new `crates/fe-gpui/src/auto_indent_host_tests.rs` on real Rust and
Python syntax.

## Implementation notes

- The hooks take the `&BufferSnapshot` the edit left rather than its `&Text`: the host has to catch its
  tree up with that text before fe-syntax can answer, and only a snapshot of the same buffer lets
  `Syntax::finish` edit a tree forward and reparse incrementally.
- `ViewHost` answers from the view's loaded syntax even when its tree lags the text (before a frame
  catches it up), carried in the `language` field, now a `LoadedLanguage` (name, syntax, and a
  per-dispatch cache and deadline), so vim.rs, which only passes `loaded_language` through, is
  unchanged. `reparsed` builds a throwaway `Syntax` for the snapshot: a tree parsed at its own version
  seeds it and is edited forward and reparsed incrementally; a tree waiting on a reparse isn't reparsed,
  and the text is parsed afresh. Seeding is cheap for well-formed code (about 2 ms for 584 KB of Rust)
  but costs about a full parse for malformed code (230 to 420 ms measured), where tree-sitter reuses
  little, so every parse, the seed included, runs on the dispatch's thread and stops at the deadline:
  nothing parses on past it. That needed `ParseJob::parse` (one attempt that stops when told, with no cap
  or retry from scratch) public in fe-syntax, so the scope was widened to `crates/fe-syntax/src/syntax.rs`
  (respec; a worker thread waited on until the deadline was rejected, as it parsed on unheeded). All of
  one dispatch's parsing shares one 20 ms deadline, and the result is cached per version, so many carets
  parse once. Out of time, there's no suggestion and the row's indentation is copied. The view's own
  tree catches up on a frame as before.
- Each edit lands in two batches of one transaction: `newline_in` inserts the line breaks with the
  copied indentation, asks the host about each new row, and re-indents in `Buffer::edit_in` on the
  first batch's transaction (nothing more if nothing changes and no pair split). A suggestion's basis
  row, if it's another new row, counts as re-indented, and a split pair's closer row goes to its opener row's
  indentation as re-indented, in a batch before the caret rows', and later rows count from the closer row
  as re-indented. Every caret ends an edit of the last batch, so
  redo puts it back: a re-indented row's edit replaces its indentation, any other's puts back the text
  from the line break to its caret. `apply_selecting` gained a sibling, `apply_through`, that returns
  the transaction, and a command's transaction list no longer repeats an id it already ends with.
- A typed closer's re-indent goes into the transaction the char's insert went into (captured as
  `run_as` captures a command's), with `edit_in`, so one undo takes out both however long the host
  takes; the run of typing ends there. Its last batch has an edit ending at every caret after the
  typed char (the char put back, where the row isn't re-indented), so redo puts the carets back.
- A caret is between a pair when the chars around it are an opener and closer from the host's
  `auto_pairs` (not quotes): a host without a language, like `HeadlessHost`, splits nothing, and
  `editor.newline` there behaves as before. Without a suggestion the row between is a level in.
- `ViewHost::closer_outdents` answers nothing with vim on, where typing inserts exactly what was
  typed, as `auto_pairs` already does; vim's own line breaks are the next card's.
- Carried finding T69 F1 (vim insert mode pairing) was fixed by T82 (08db3a6): `ViewHost::auto_pairs`
  is empty with vim on, pinned by `vim::insert_tests::vim_insert_types_pairs_as_typed` on a loaded
  Rust view. `editor.insert` keeps going through `host.auto_pairs()` (now in `insert_typed_in`), so
  it still holds.
