---
task_uid: "01m5tyhpexk1gj1x591zhbwrp2"
task: "T353"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "A document link open started in the same update as a pane or tab activation completes (resolution and file check) instead of being discarded, while superseded opens are still dropped. Tests an_open_right_after_pane_activation_succeeds, an_open_right_after_tab_activation_succeeds"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Capture the link-open activation epoch synchronously so an open right after a pane or tab activation is not discarded (T339 F1)

## Problem
T339 (T330 F9/F10) review round 3 left F1 (medium, `crates/fe-workbench/src/lsp_links.rs:431`), carried as a documented limit of T339: `Workbench::activate` increments the link-open activation counter synchronously through `follow_active_pane`, but `Workspace::follow_active` also queues `ActiveChanged` whose subscriber increments the same counter after the current update. A link open started right after a pane or tab activation in the same update captures a counter that then changes with no further navigation, so both completion guards silently discard the valid open.

## Change
Capture an activation epoch updated synchronously at the originating transition, not a counter also advanced by deferred event delivery.

## Tests
In `crates/fe-workbench/src/lsp_links_landing_tests.rs`: successful opens where a pane or tab is activated and an open starts in the same update, covering resolution and file checks, while retaining the superseded-open tests.
