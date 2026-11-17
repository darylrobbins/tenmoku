---
task_uid: "01m7kthkv7qnbvzgzve7kqs6cn"
task: "T669"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "The hand-in names the status item and code path that drew 'by language server' in the hero capture (helium's site-shot of 5d6b1843c), with the condition that triggers it"
    required: true
    manual: false
  - id: "C2"
    text: "tests::status_bar::by_language_server::the_status_bar_shows_no_bare_by_language_server_label reproduces that condition and fails at base, passes with the fix; existing status bar tests pass"
    required: true
    manual: false
---

# Status bar shows a stray 'by language server' label (seen in the tenmoku.dev hero capture of a Rust file on Linux, Yohen theme): find which status item renders it and when, and stop it showing outside its intended context (Mac T1915)

**Helium filing note.** The regression test lives in a new submodule of the existing `tests::status_bar` module (`crates/fe-workbench/src/tests/status_bar/by_language_server.rs`, declared with `mod by_language_server;` in `crates/fe-workbench/src/tests/status_bar.rs`), not a new module from `src/tests/mod.rs`: rgt finish's regression check finds 0 tests at base for a module declared from mod.rs. Write it against base APIs only. A likely source is `nav::show_source` (crates/fe-workbench/src/nav.rs, `workbench-navigation-source = by { $provider }` in navigation.ftl), also called from nav_picker.rs; confirm before fixing.

## Implementation notes

The label is the `nav_source` status item (`nav::SOURCE_ITEM`, `workbench-navigation-source = by { $provider }`). `nav::show_source` sets it after a go-to-definition (nav.rs, Workbench definitions path) or a workspace-symbols answer (nav_picker.rs); the fe-lsp provider is named "language server". Nothing ever removed it, so after any LSP-answered nav it stayed on the bar indefinitely, bare of context. Fix: `StatusBar::show_expiring` removes it after 5 s.
