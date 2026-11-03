---
task_uid: "01m6ezvcn49yq8b3td0hbk1p36"
task: "T417"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test external_format_wiring_tests::external_formatter_formats_the_document (cargo nextest run -p fe-workbench) passes: with [formatters.rust] set and a fake Runner, lsp.format_document replaces the buffer text with the runner's output as one undoable edit and the caret stays on the same text"
    required: true
    manual: false
  - id: "C2"
    text: "Test external_format_wiring_tests::prefer_external_falls_back_to_lsp (cargo nextest run -p fe-workbench) passes: prefer = external with a runner that reports NotFound or Failed runs the fake LSP formatter instead, and a Failed run toasts one line naming the program and the first stderr line; NotFound toasts nothing when LSP then formats"
    required: true
    manual: false
  - id: "C3"
    text: "Test external_format_wiring_tests::prefer_lsp_uses_external_when_lsp_has_no_formatter (cargo nextest run -p fe-workbench) passes: prefer = lsp with no server formatter runs the external command; with an LSP formatter the runner is never called"
    required: true
    manual: false
  - id: "C4"
    text: "Test external_format_wiring_tests::format_on_save_runs_the_external_formatter (cargo nextest run -p fe-workbench) passes: with editor.format_on_save on, cmd-s formats through the runner before the file is written and a Failed or timed-out run still saves the unformatted text, logging at debug, never toasting"
    required: true
    manual: false
  - id: "C5"
    text: "Test external_format_wiring_tests::save_without_formatting_skips_it (cargo nextest run -p fe-workbench) passes: workbench.save_without_formatting never calls the runner"
    required: true
    manual: false
  - id: "C6"
    text: "Test external_format_wiring_tests::selection_format_ignores_the_external_formatter (cargo nextest run -p fe-workbench) passes: lsp.format_selection uses LSP only and toasts 'No formatter for \u003clanguage\u003e' when it has none; the runner is not called"
    required: true
    manual: false
  - id: "C7"
    text: "Test external_format_wiring_tests::buffer_changed_while_formatting_drops_the_result (cargo nextest run -p fe-workbench) passes: an edit made while the runner is pending makes the late output be dropped (FormatOutcome::Unchanged), as the LSP path does"
    required: true
    manual: false
  - id: "C8"
    text: "Existing lsp_format_tests, lsp_format_save_tests and lsp_format_overlap_tests still pass unchanged"
    required: true
    manual: false
---

# Format Document and format on save use the language's external formatter, preferred or as the fallback to LSP formatting, with toasts for failures (Mac T1326)

## Problem
f7a/f7b give settings and a runner; Format Document and save still call only the LSP server.

## Change
In crates/fe-workbench/src/lsp_format.rs, `Workbench::format_range` (document formatting only; `selection == true` stays LSP-only) first looks up `settings.formatters[language]`. Order by `prefer`; the second source runs when the first returns NoFormatter, `FormatError::NotFound`, or Failed/timeout. Failure of every source toasts only for an explicit `lsp.format_document` (not for save, which keeps logging at debug as `FormatOnSave` does today). External output is applied through the same ProposedChange path and staleness check the LSP reply uses (buffer version captured before, dropped if changed). The Workbench holds an `Rc<dyn Runner>` field set to the real runner in `Workbench::new` and replaceable in tests; `FormatOnSave` needs no change because it calls `format_buffer`. New `FormatOutcome` variants not needed: external success is `Formatted`.
Tests are in the new external_format_wiring_tests.rs (declare in lib.rs under cfg(test)); do not edit existing test files.

## Implementation notes

- Scope grew past the card's three files: `workbench.rs` for the `format_runner` field the spec asks for, `lsp.rs` to share `routed_language` (the key `formatters` is documented to use), and `fe-syntax/src/language.rs` for `Language::first_extension`, which names an unsaved buffer's `untitled.<extension>` placeholder.
- Settings are read from `kiln::platform::config::settings` when formatting starts, so a reload needs no cached copy.
- Trust: `external_format` leaves the folder's trust to its caller. The formatter is the user's choice, but it reads config from the folder, and that config can be code. So just before each run, `ExternalFormatter::run` resolves the folder it runs in and the file's real folder with `lsp::resolve`, off the main thread, and checks both against the live decisions (`kiln_shell::trust::decided`). That covers directory and file symlinks, `..`, a rootless unsaved buffer inheriting the process's folder, and trust revoked while a preferred server is answering. A folder that isn't trusted, or a path that won't resolve, means the language has no external formatter for this format. So a scratch buffer in a rootless window formats only if the app's working folder is trusted.
- The output is re-diffed by lines with `ProposedChange::from_text` over the whole text, not applied as `format_text`'s one edit. One edit would collapse a caret between two changes, and comparing with newlines normalized stops CRLF output from counting as a change.
- Known limit: a save gives its hook `SAVE_HOOK_LIMIT`, 1 s, in fe-gpui. On save a formatter is stopped at that point, whatever its `timeout_ms` (default 3 s), and the text is saved unformatted, with no time left for the LSP fallback. Code actions on save take up to 750 ms of the same second. Pinned by `a_save_stops_a_formatter_past_the_save_hook_limit`; lengthening the limit is a card of its own.
