---
task_uid: "01m541cq8tn3ftt4d56t8e1pgv"
task: "T127"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Clicking an empty pane's body or a loading pane's body moves keyboard focus to that pane (F1)"
    required: true
    manual: false
  - id: "C2"
    text: "Closing a pane onto a loading or an error pane focuses that pane's focus target, not its hidden tab (F1)"
    required: true
    manual: false
  - id: "C3"
    text: "Splitting while a read is pending over an open tab, or after a failed read, gives an empty new pane (F2); the regression test tests::workspace_focus_split::splitting_over_a_pending_read_gives_an_empty_pane fails before the fix"
    required: true
    manual: false
  - id: "C4"
    text: "Any kiln-shell hook added is defaulted and additive: kiln-shell's existing tests pass unchanged"
    required: true
    manual: false
---

# Test fastedit's pane focus and visible-tab policy under kiln's Workspace: focus follows a pane the Workspace activates, and a split doesn't copy a tab hidden by a loading or error message (Mac T690 review F1, F2; fixed by T109)

Argon follow-up to T108 (Mac T690) review findings F1 and F2 (both medium, real behaviour regressions, filed under note d). Inserted into the zo chain after T109 (Mac T691) and before T110 (Mac T693), because the chain is serial on workbench.rs. Title the host commit "... (Mac T690 review F1, F2)".

## Problem

T108 embedded kiln's `Workspace<Pane>` in fastedit's `Workbench` and deleted the copied pane glue. Two behaviours of the
old path were lost:

- **F1 (focus)** (`crates/fe-workbench/src/workbench.rs` ~1483). When the Workspace activates a pane itself, the
  workbench follows by updating handles but doesn't apply fastedit's focus policy. Clicking another pane's empty or
  loading body makes that pane active but leaves keyboard focus in the previous editor. Closing a pane onto a loading or
  error pane lets the Workspace focus that pane's hidden underlying tab, bypassing `Pane::focus_target`. The old
  activation path handled both.
- **F2 (split copies a hidden tab)** (workbench.rs ~1646). `Workspace::split` reads `PaneState::active_item`
  directly, where the removed code used `Pane::active_item`, which excludes tabs hidden by a loading or error message.
  With a.txt open, start loading another file and split before it finishes: the new pane copies the hidden a.txt tab
  instead of staying empty. The same happens after a failed read.

## Change

- When the Workspace activates a pane on its own (a click, or closing onto a neighbour), apply fastedit's
  `Pane::focus_target` policy, so focus follows to the pane's focus target, without stealing focus on ordinary
  notifications.
- Make a split honour fastedit's visible-item policy (`Pane::active_item`), keeping the Workspace as the owner of the
  split. If this needs a hook in kiln-shell's Workspace (for example a pane-provided "item to copy on split"), add it
  defaulted and additive, with no change to existing kiln-shell behaviour for other hosts.

## Tests

In a new `crates/fe-workbench/src/tests/workspace_focus_split.rs`, declared with one `mod workspace_focus_split;` line
in `crates/fe-workbench/src/tests/mod.rs` (test code, so rgt's base run compiles it). The regression test is
`tests::workspace_focus_split::splitting_over_a_pending_read_gives_an_empty_pane` (full module path; keep that name):
- clicking an empty pane's body, and a loading pane's body, moves keyboard focus to that pane;
- closing a pane onto a loading pane and onto an error pane focuses that pane's focus target, not its hidden tab;
- splitting while a read is pending over an open tab, and after a failed read, gives an empty new pane.

## Implementation notes

At the base commit both findings were already handled: T109 added `PaneView::shown_item` (fastedit's `Pane` returns `Pane::active_item`, so a split and the workspace's focus skip a tab hidden by a loading or error message), and `Workbench::follow_workspace` applies `Pane::focus_target` when the workspace activates a pane itself. No source change was needed, and no kiln-shell hook. This task adds the missing tests in `tests/workspace_focus_split.rs`; reverting `shown_item` to the raw `PaneState::active_item` makes the two split tests fail.
