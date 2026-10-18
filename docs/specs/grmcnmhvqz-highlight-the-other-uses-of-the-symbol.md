---
task_uid: "01m55ef7gdz7vgeygrmcnmhvqz"
task: "T173"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test lsp::trust_tests::caret_highlights::server_highlights_show_where_the_caret_rests passes: after the caret rests 250 ms the active editor's decorations hold a Muted highlight over the server's read range and an Accent one over its write range; two moves inside 250 ms made one textDocument/documentHighlight request; moving off clears them; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test lsp::trust_tests::caret_highlights::a_late_answer_for_an_old_caret_is_dropped passes: an answer landing after the caret moved to another word shows nothing; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit tests lsp::trust_tests::caret_highlights::words_highlight_when_the_server_has_no_provider and lsp::trust_tests::caret_highlights::only_the_active_editor_highlights pass: without a provider the caret word's whole-word occurrences are Muted highlights and no request is sent; a newly active split clears the other editor's; they fail at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (lsp_trust_tests.rs only gains the #[path] mod line); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Highlight the other uses of the symbol at the caret from the language server's document highlights, else the word's occurrences, as VS Code's occurrencesHighlight does (Mac T854)

## Problem

VS Code highlights the other uses of the symbol under the caret (`editor.occurrencesHighlight`, on by default): the
language server's document highlights where one provides them, write uses stronger than read uses, else the
word's textual occurrences. fastedit shows neither:

- fe-editor's word-occurrence provider `OccurrenceHighlights` (`crates/fe-editor/src/occurrences.rs:31`, with
  `EditorModel::occurrence_target` at `:110` and 7 unit tests) is created by nothing outside its tests;
- the server's highlights come from `LspManager::document_highlights` (the card this depends on).

Gap audit 2026-10-17 (user-approved).

## Change

New module `crates/fe-workbench/src/lsp_highlights.rs`, declared from `crates/fe-workbench/src/lsp.rs` as
`#[path = "lsp_highlights.rs"] mod highlights;`, driven from `Workbench::sync_lsp` (`lsp.rs:1114`, the frame hook
after each draw):

- **One provider per editor.** Each open editor gets one `CaretHighlights` `DecorationProvider`
  (`EditorModel::add_decoration_provider`), installed once, as `sync_lsp` installs the hover provider. It shows
  either the server's highlights (anchored ranges with their `HighlightKind`) or an `OccurrenceHighlights`'
  words, never both: `Highlight` decorations, `Tint::Muted` for Text and Read, `Tint::Accent` for Write.
- **When.** Only the active editor shows any; the others' are cleared. For it, the target is the primary
  selection's caret when there is exactly one selection and it is empty or exactly one whole word (the rule
  `occurrence_target` applies); anything else clears. When the (buffer version, target) pair changes, what is
  shown is cleared unless the target is still inside a shown range at the same version, and a request is made
  after the caret has rested 250 ms (VS Code's `editor.occurrencesHighlightDelay` default), on GPUI's executor
  timer; moving again before then restarts the wait, so a run of moves makes one request.
- **Answer.** `Ok(Some(highlights))` is shown if the editor is still active with the same version and target, and
  dropped otherwise (dropping the future of a superseded request cancels it). `Ok(None)` (no server, or the server
  doesn't provide it) shows the target word's occurrences instead (`OccurrenceHighlights::set_target`).
  `Err(_)` shows nothing. Each change that alters what's shown notifies the editor.

New unit-test file `crates/fe-workbench/src/lsp_highlights_tests.rs`, declared by
`#[path = "lsp_highlights_tests.rs"] mod caret_highlights;` at the end of the existing
`crates/fe-workbench/src/lsp_trust_tests.rs`, on `crate::tests::lsp::Lsp` with a trusted root and the manager's
`TestClock`, advancing GPUI's clock past 250 ms (`cx.executor().advance_clock`) and drawing:

- `server_highlights_show_where_the_caret_rests`: the fake provides `documentHighlightProvider` and answers a read
  and a write range; after the wait the active editor's `model().decorations(..)` hold a `Muted` highlight over the
  read range and an `Accent` one over the write range; two caret moves inside 250 ms made one
  `textDocument/documentHighlight` request; moving the caret off the symbol clears them before the next answer.
- `a_late_answer_for_an_old_caret_is_dropped`: the reply delayed; the caret moves to another word before it lands:
  nothing from it is shown.
- `words_highlight_when_the_server_has_no_provider`: a server without `documentHighlightProvider`: the caret's
  word's other whole-word occurrences are highlighted `Muted`, and the fake records no documentHighlight request.
- `only_the_active_editor_highlights`: a split showing another file becomes active: the first editor's
  highlights are cleared.

## Implementation notes

- Any extra task on GPUI's seeded scheduler (here, the 250 ms wait) flipped the existing
  `project_panel_ops_tests::vim` at seed 0; it already failed at seeds 1 and 6 without this change. The cause is a
  lost wake-up in `Pane::open_at`: an open that lands on the tab already active (a Save As of that file) activated
  it without a notify, so the workbench's waiting tree open never put vim in normal mode. `open_at` now notifies
  there (scope widened to `pane.rs` by respec for that one line).
