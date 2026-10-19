---
task_uid: "01m58v8pn8dkahcgmxd77a312q"
task: "T210"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench unit test workbench::git_blame::git_blame_tests::activation::a_workspace_round_trip_within_one_update_drops_pending_requests passes: Workspace::activate(A) then Workspace::activate back to the original pane inside one update (not Workbench::activate) drops a pending blame or diff open, so no obsolete diff opens and focus is kept; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_blame_tests.rs only gains the #[path] mod line); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Git blame and diff guards: a workspace-driven A to B to A activation within one update drops pending requests (T135 review F4)

## Problem

T135 landed on a user findings_override with this review finding (codex, medium, F4, `crates/fe-workbench/src/workbench.rs:~1583`) open and its test deferred by a relaxed criterion: the activation generation counts workbench synchronization, not every workspace activation. `Workspace::activate(A)` then `Workspace::activate(B)` back to the original pane within one update leaves `follow_workspace` seeing no change, because `ActiveChanged` events are ignored, so a pending blame/diff guard stays valid and can open an obsolete diff. The existing tests call `Workbench::activate`, which increments synchronously, and miss this integration path.

## Change

Count each workspace activation at its source, or consume every relevant `ActiveChanged` event, so a pending guard is dropped by a workspace-driven A to B to A round trip inside one update. Test that workspace-driven round trip (activations through `Workspace`, not `Workbench::activate`).

New tests go in a new file `crates/fe-workbench/src/git_blame_activation_tests.rs`, declared from `git_blame_tests.rs` with one `#[path = "git_blame_activation_tests.rs"] mod activation;` line.

## Implementation notes

`Workspace` emits `ActiveChanged` on each change of its active pane or item, so a round trip within one update emits twice even though `follow_workspace` sees only where it ended. `workspace_event` now counts each one in `Workbench::activations`, which the blame and diff guards already compare.
