---
task_uid: "01m6vvvxtd2aqqrf17z612tp61"
task: "T1561"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C2"
    text: "en-US byte-equal; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# fe-workbench LSP, tasks, tests, quickfix and reference panel strings (i18n W6, rest)

# fe-workbench LSP, tasks, tests, quickfix and reference panels (W6, rest)

Spec: card **W6** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9, for the fe-workbench files the B3 surface
cards don't restyle (they move their own files' text). Sections 3–8 are the
design. en-US stays byte-equal to today's, so no golden and no behaviour test
changes except tests that construct removed literals.

Files: `lsp.rs` and every other `lsp_*.rs` except `lsp_lenses.rs`, `refactor_preview.rs`, `refactor_preview_view.rs`, `task_defs.rs`, `task_problems.rs`, `task_run.rs`, `test_commands.rs`, `test_runnables.rs`, `quickfix.rs`, `quickfix_commands.rs`, `quickfix_fill.rs`, `quickfix_panel.rs`, `hierarchy_panel.rs`, `references_panel.rs` (and `references_panel/`), `peek.rs`, `peek_references.rs`, `snippet_picker.rs`, `diagnostics_picker.rs`, `ignore_diagnostics.rs`.

Add `crates/fe-workbench/l10n/en-US/lsp.ftl` (keys `workbench-lsp-…`) to
`resources!` in `crates/fe-workbench/src/l10n.rs`, and these files to
`l10n-allow.toml`'s `migrated` list. English plural helpers become plural
messages; fragments become whole messages.

Waits for T1494, T1496 and T1490 (quickfix files). `lsp.rs` and `lsp_format.rs`
are also in T1355/T1356's scopes (blocked today).

Platform: **Linux-OK**.

## Implementation notes

- **Scope added (rev 3, rev 4).** `nav.rs`: its `by()` helper built the
  ` (by <provider>)` fragment the references panel appended, and its doc said
  this card would retire it; the panel's statuses are now whole messages with
  nav's `$source`/`$provider` selector, so `by()` is removed rather than left
  dead. `l10n-allow.toml`: the lint rejects an allow entry that matches
  nothing, so nav.rs's `" (by {provider})"` entry is removed (an edit, not an
  append). `task_commands_tests.rs`, `task_run_tests.rs`,
  `test_commands_tests.rs`: they compared the removed `UNTRUSTED`,
  `NO_TEST_HERE` and `NO_TEST_RUN` consts, now functions.
- **Plural helpers.** `references_panel::count` and the quickfix panel's
  `item{s}` became plural messages (`workbench-lsp-references-found`,
  `-first`, `workbench-lsp-quickfix-items`).
- **Fragments.** nvim's ` error`/` warning` type words in `:clist` and on
  landing are a `$kind` selector in whole messages; each nav-style
  `(by …)` suffix is a `$source` selector, as navigation.ftl has it.
- **Panel names.** `Quickfix List` and `Hierarchy` stay in their
  `SidebarPanel` consts (allow-listed): their `title()`s, which the dock
  shows, resolve from the catalogue. `References` is allow-listed too, but
  dock.rs's `dock_item!` still titles the dock with it, as it does Search and
  Chat: that is dock.rs's to key.
- **Not translated, on purpose.** Debug-log-only reasons (`format on type`,
  `out of time`, `the buffer changed` on save, `link gone`, `an edit`,
  `not a regular file`), parser tokens of compiler output
  (`error: `, `", line `), file names (`Cargo.toml`, `Makefile`), a
  quickfix list's title (the ex command as typed, as nvim titles it) and
  element ids/selectors are `verbatim!` or allow-listed.
- **Known gap, outside scope.** lsp_highlights, lsp_format and
  lsp_format_on_type compare `LspError::Unavailable` reasons against fe-lsp's
  English (`not open on a language server`, `buffer changed`). fe-lsp now
  resolves those reasons with `tr!` and exposes no accessor, so the matches
  hold only in en-US. It needs fe-lsp to expose the reasons (a predicate on
  `LspError` or public keys); a follow-up for fe-lsp's owner.
