---
task_uid: "01m55ef8a5b07x99atmzq49s9t"
task: "T151"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor integration tests parses_tabstops_placeholders_choices_and_mirrors, nested_placeholders_and_escapes, unknown_variables_become_placeholders_or_their_default and malformed_input_is_literal_text (crates/fe-editor/tests/snippet_parse.rs, top level) pass; they fail at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor integration tests tab_and_shift_tab_walk_the_tabstops, mirrors_are_typed_together, the_snippet_is_indented_as_its_row and leaving_the_stops_ends_the_session (crates/fe-editor/tests/snippet_session.rs, top level) pass through commands::registry() dispatch of editor.tab and editor.outdent; they fail at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor integration test accepting_a_snippet_completion_starts_a_session (crates/fe-editor/tests/snippet_session.rs) passes: a CompletionItem with snippet: true accepted through the completion session inserts the expanded text with stop 1 selected, and one editor.undo restores the text from before the accept; with no session editor.tab and editor.outdent behave as at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-editor passes with no existing test edited; cargo clippy -p fe-editor --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-editor: a snippet engine: parse LSP snippet syntax, insert with placeholders filled and indented to the row, and walk the tabstops with Tab and Shift-Tab, mirrors typed together (Mac T856)

## Problem

fastedit has no snippet engine. VS Code inserts LSP completions, user snippets and emmet through one: a snippet's
text goes in with its placeholders filled, the first tabstop is selected (every occurrence of it at once, so typing
fills its mirrors), Tab and Shift-Tab move between tabstops, and the caret ends at `$0`. fe-lsp declines snippets
(`snippet_support: Some(false)`, `crates/fe-lsp/src/server.rs:2038`), which leaves rust-analyzer's and
typescript-language-server's call completions without their argument placeholders. Gap audit 2026-10-17
(user-approved: LSP completeness first); the LSP plumbing is the card that depends on this one.

## Change

GPUI-free, in fe-editor.

1. New module `crates/fe-editor/src/snippet.rs` (`pub mod snippet;` in `crates/fe-editor/src/lib.rs`):
   `Snippet::parse(source: &str) -> Snippet`, the LSP snippet grammar
   (https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#snippet_syntax):
   - `$1`, `${1}`, `${1:placeholder}` (a placeholder may hold text, tabstops and placeholders, nested),
     `${1|one,two|}` (a choice: its first option is the placeholder text), `$0` / `${0}` the final stop;
     a tabstop number repeated is a mirror of the first with a placeholder;
   - `${1/regex/format/flags}` is kept as a mirror of tabstop 1 with no transform applied;
   - variables `$NAME`, `${NAME}`, `${NAME:default}`: none is resolved here; one with a default inserts the
     default, one without becomes a placeholder holding its name, visited after the numbered tabstops (VS Code's
     rule for an unknown variable);
   - `\$`, `\}` and `\\` escape (and `\,` `\|` in a choice); anything malformed (an unclosed `${`) is literal text.
   `Snippet` exposes its plain text and, per tabstop in visiting order (1, 2, …, then name placeholders, then 0), the
   byte ranges of each occurrence in that text.
2. `EditorModel::insert_snippet(&mut self, range: Range<usize>, snippet: &Snippet)` (in `snippet.rs`, an `impl
   EditorModel` block): replaces `range` with the snippet's text as one `Origin::User` transaction, each line after
   the first prefixed with the indentation of the row `range` starts on and each `\t` written as the editor's indent
   unit (`IndentStyle`), as VS Code adjusts snippet whitespace; then, if the snippet has a tabstop other than `$0`,
   starts a session and selects every occurrence of the first stop (multiple selections); else the caret goes to
   `$0`, or the end.
3. The session (state on `EditorModel`, in `lib.rs`): the stops' occurrence ranges as anchors that grow with typing
   at either end. `editor.tab` (`crates/fe-editor/src/commands.rs`) with a session selects the next stop's
   occurrences instead of inserting a tab; reaching `$0` (or the end) puts the caret there and ends the session.
   `editor.outdent` (`crates/fe-editor/src/rows.rs`) with a session selects the previous stop's occurrences (the
   first stays). The session ends when the selections leave every occurrence of the current stop, an edit lands
   outside them, or `editor.single_selection` (Escape) runs.
4. Completion: fe-editor's `CompletionItem` (`crates/fe-editor/src/completion.rs`) gets `pub snippet: bool`. Accepting
   an item with it set parses `edit.1` as a snippet and inserts it through `insert_snippet` in the same single
   transaction the accept makes today (its word-chars-typed-since rule and additional edits unchanged); `false`
   keeps today's plain insert.
5. Tests, all new integration tests with top-level fns, dispatching as `crates/fe-editor/tests/viewer.rs` does:
   - `crates/fe-editor/tests/snippet_parse.rs`: `parses_tabstops_placeholders_choices_and_mirrors`
     (`fn ${1:name}(${2:arg}: ${3|i32,u64|}) {\n\t$0\n}`, plain text and each stop's ranges),
     `nested_placeholders_and_escapes`, `unknown_variables_become_placeholders_or_their_default`,
     `malformed_input_is_literal_text`;
   - `crates/fe-editor/tests/snippet_session.rs`: `tab_and_shift_tab_walk_the_tabstops` (insert, type over stop 1,
     tab to 2, shift-tab back to 1 with the typed text selected, tab twice ends at `$0` and a further tab inserts a
     tab), `mirrors_are_typed_together` (`${1:a} = $1;`: typing replaces both), `the_snippet_is_indented_as_its_row`
     (inserted on an indented row with 4-space indent: the second line gets the row's indent and `\t` four spaces),
     `leaving_the_stops_ends_the_session` (a click elsewhere, then tab inserts a tab), and
     `accepting_a_snippet_completion_starts_a_session` (a `CompletionItem` with `snippet: true` through the existing
     completion session's accept: one undo restores the text from before it).

## Implementation notes

- Outside the listed scope, one mechanical line: `crates/fe-workbench/src/lsp.rs`'s `completion_item` builds
  fe-editor's `CompletionItem` with every field named, so the new `snippet` field needs `snippet: false` there to
  compile (fe-lsp still declines snippets; the plumbing card sets it).
- `editor.single_selection` is registered by `multi_cursor.rs`, outside the scope, so it ends the session in
  `commands::add`, which registers it: the session ends before the command runs, even with one selection.
- The session follows the stops' occurrences by offset through the buffer's edit batches, as `pairs::AutoClosed`
  does, rather than through `Anchor`s: each edit is given to one occurrence of the stop visited, which grows with it,
  as do the placeholders it's nested in as the snippet is written. Edits made through `apply_selecting` and row edits
  are planned before the buffer joins insertions at one offset, so each keeps its owner (wrapping adjacent mirrors in
  a pair); otherwise, mirrors typed together split a batch's edits evenly in order, and at a boundary two share an
  edit goes to the one no edit has gone to yet. Other occurrences don't grow with text typed at their ends (VS Code's
  inactive stickiness); an empty one where text is typed stays before it if it comes first in the snippet. One an
  edit cuts into is gone and skipped. It's caught up wherever `forget_auto_closed` runs.
- Mirrors are kept equal, of the stop visited and of the placeholders enclosing the edit. An edit through
  `apply_selecting` (typing) or a row edit is copied into the mirrors after it in the same batch, so it stays in the
  run of typing and its undo step. Copies into mirrors before it go in a batch of their own in the edit's
  transaction, after it, as `pairs.rs` (out of scope) records auto-closed pairs at offsets in the text its own edits
  make; that batch ends the run of typing (the typing group is `typing.rs`'s, also out of scope), so typing in a
  later mirror after a click into it is an undo step a keystroke. An accepted completion's edit is copied the same
  way, inside its transaction; after an accept inside a stop, redo puts carets at the copies.
- A mirror shows its placeholder's text but not the stops inside it; a placeholder mirroring itself is cut short.
  Hostile input is bounded: placeholders nest, and mirrors show mirrors, at most 64 deep; parsing takes at most 16
  steps a source byte before every further `$` is literal; mirrors add at most 4 MiB, to the text and to the
  placeholder texts kept for them together, after which each mirror is empty.
- `\r\n` and `\r` in a snippet go in as `\n`, as the buffer keeps line breaks, with the row's indentation after.
- A snippet completion is indented as its row is when accepted, not when completions were asked for.
- Shift-tab at the first stop selects its occurrences again, as VS Code restores the current placeholder.
- Inserting a snippet ends a session going on: snippets don't nest.
