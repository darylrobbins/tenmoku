---
task_uid: "01m4rhfw6g85yxn8ft3ftyxvt6"
task: "T518"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "toast_routing_tests::lsp: a fake server's window/showMessage of type Error and Warning each show a toast of that level naming the server; Info and Log show none"
    required: true
    manual: false
  - id: "C2"
    text: "toast_routing_tests::open_failures: opening an unreadable file shows \"Can't open \u003cname\u003e\" with the error as detail; re-rooting at an unreadable folder shows \"Can't open folder \u003croot\u003e\""
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Show language-server errors and warnings, files that can't open and settings failures as toasts instead of only logging them

## Problem

Messages the user should see are only logged (day-one audit 2026-10-12). The previous card adds `toast::show`.

## Change

Route these to toasts, keeping the log lines:

- `crates/fe-workbench/src/lsp.rs`: `window/showMessage` of type Error or Warning (`LspEvent::ShowMessage`, :1341)
  as a toast of that level titled with the server's name; Info and Log stay in the log. "can't restart {name}"
  (:1483) as an error.
- `crates/fe-workbench/src/pane.rs:432`: "Can't open {name}" with the error as detail.
- `crates/fe-workbench/src/workbench.rs`: the settings-file errors (:661, :670) and, on macOS too, the open panel's
  failure (:615), which today shows a prompt off macOS only.
- `crates/fe-workbench/src/project_panel.rs:176`: "Can't open folder {root}".

## Tests

New file `crates/fe-workbench/src/toast_routing_tests.rs`, declared from toast.rs, using the fake language server
of the `tests::lsp*` suites.
