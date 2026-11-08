---
task_uid: "01m6j057543ze6dr82gdb47yed"
task: "T1356"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_format::plugin_format_tests::a_plugin_formats_a_document_nothing_else_covers: with no external formatter and no server formatter, lsp.format_document applies the plugin edits as one undoable edit"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_format::plugin_format_tests::external_and_server_formatters_come_first: with either configured the plugin is never asked; when both fall through it is"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_format::plugin_format_tests::format_on_save_uses_the_plugin_formatter: with editor.format_on_save on, cmd-s formats through the plugin before writing; a failing or timed-out plugin still saves the unformatted text and toasts nothing"
    required: true
    manual: false
  - id: "C4"
    text: "plugin_format::plugin_format_tests::a_stale_answer_is_dropped: an edit made while the plugin is asked drops its answer"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass, the existing lsp_format, lsp_format_save, lsp_format_overlap and external_format_wiring tests unchanged"
    required: true
    manual: false
---

# Format Document and format on save use a plugin formatter for a language that no external or language server formatter covers

## Problem

A plugin's registered formatter (`fe_plugin::formatting`) isn't used by Format Document or format on
save.

## Change

Builds on T1326, which makes `Workbench::format_range` (`crates/fe-workbench/src/lsp_format.rs`)
try ordered sources (the language's `[formatters.<language>]` external command and the LSP server,
by `prefer`), falling through on no formatter, not found, failure or timeout:

- `crates/fe-workbench/src/plugin_format.rs` and `plugin_format_tests.rs` (`mod` in `lib.rs`): a
  plugin formatter is one more source, **last**: it formats a document only when neither the
  external formatter nor an LSP server covers the language (extensions yield to built-ins and to the
  user's own config, record §4). A `prefer = "plugin"` choice is left for later. Document formatting
  only; `lsp.format_selection` stays LSP-only.
- Its edits apply through the same staleness check and `ProposedChange` path as the other sources:
  the version is captured before, and an edit made meanwhile drops the answer. On save it waits at
  most the plugin's call deadline; a failure or timeout saves the text unformatted and logs at debug.
  An explicit Format Document toasts one line naming the plugin when its formatter fails.
- `docs/plugins/authoring.md`: a Formatting section.

## Notes

Seam choice: depend on T1326 and add a source to its loop, rather than a parallel path that would
fight it in `lsp_format.rs`. T1326 is lent to helium: if its Mac card is abandoned when helium's copy
lands (as T1061 was), this dependency never satisfies; then swap it for the landed commit by hand.
Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- The `formatting` slice wasn't wired into plugin loads before this card, so `plugin_host.rs` now
  makes it on the app's `PluginFormatting` (`plugin_format.rs`) for each load, and unloads it with
  the plugin's paint and diagnostics. `LoadContext` keeps its fields (tests build it by hand):
  `start_load` calls a new `load_with` that takes the formatting; `load` is a test-only wrapper.
- The slice is bound before the plugin exists, through a `LateHandle` that holds the plugin weakly
  and is set once the load returns: a request during `init` fails alone instead of unloading the
  plugin.
- A request blocks on the plugin's thread, so it runs on a thread of its own, and the format waits
  at most the plugin's call deadline for it. The snapshot asked about is held for the request, and
  the plugin's answer is checked against it.
- A plugin reads the buffer as it is now, so typing during a request can turn a good format into a
  rejected answer. Any answer to a buffer that changed meanwhile is dropped quietly, failures too.
- The tests load fe-plugin's editor guest. Its formatter commands are `fmt.<name>`, so it is found
  under the id `fmt` (extension ids need two segments, so the manifest says `acme.fmt`).
