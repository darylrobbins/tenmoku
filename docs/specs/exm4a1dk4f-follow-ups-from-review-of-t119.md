---
task_uid: "01m49s87ynddrrc8exm4a1dk4f"
task: "T151"
rev: 2
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F1: a key-driven move of a pane's only tab toward a side with no neighbour follows T79's split-and-move rule again. The sole-tab self-drop restriction applies only to drops, in Pane's drop handling, and PaneGroup::move_tab stays the shared path for drops and keys. A sole-tab keyboard regression test covers it"
    required: true
    manual: false
  - id: "C2"
    text: "F2: a divider press is invalidated when the pane tree's topology changes, or is bound to a stable axis identity and topology generation. Closing A while dragging the root divider of H[A,V[B,C]] leaves V[B,C]'s ratios unaffected by later pointer moves"
    required: true
    manual: false
  - id: "C3"
    text: "F3: dropping a pane's only tab on its own centre, tab bar or edge does nothing, with no reorder or activate. While the pane shows a pending read or a read error, that indicator stays and the pending read completes as before (tested for all three drop places)"
    required: true
    manual: false
  - id: "C4"
    text: "F4: the tab-bar insertion marker stays within the target bar's visible bounds and is painted inside its clip. Tests cover a tab longer than the bar and a horizontally scrolled bar"
    required: true
    manual: false
  - id: "C5"
    text: "New tests live in crates/fe-workbench/src/pane_drag_followup_tests.rs, declared from pane_drag.rs by a #[cfg(test)] #[path] mod line, and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T119

Findings of the review of T119 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/pane_group.rs:553: The sole-tab self-split guard also blocks keyboard moves. When there is no neighbour, pane.move_tab_* supplies Split { source, direction }, so the command now does nothing instead of following T79's split-and-move rule. The existing regression tests exercise this case only with multiple source tabs.
  fix: Apply the sole-tab self-drop restriction in drop handling, while retaining the shared move_tab path and the existing keyboard behaviour; add a sole-tab keyboard regression test.
  instances: crates/fe-workbench/src/pane_group.rs:553

- F2 medium crates/fe-workbench/src/pane_group.rs:495: Divider drag state identifies an axis only by its tree path and member count. Keyboard operations remain available during a drag, and collapsing the tree can replace that path with an unrelated axis of the same size. For example, closing A while dragging the root divider of H[A,V[B,C]] replaces the root with V[B,C]; subsequent movement applies the old horizontal ratios to that vertical divider.
  fix: Invalidate divider presses when tree topology changes, or bind each press to a stable axis identity and topology generation; test collapsing an axis during an active divider drag.
  instances: crates/fe-workbench/src/pane_group.rs:495

- F3 medium crates/fe-workbench/src/pane_group.rs:562: Dropping a pane's sole tab on its own centre or tab bar still calls reorder and activate. activate clears opening and error, so a supposedly unchanged drop replaces a loading indicator or read error with the editor and changes how a pending read completes. Only edge drops receive the required no-op guard.
  fix: Reject all sole-tab drops onto their own pane before reordering or activating, and test centre, bar and edge drops while the pane shows a pending read or read error.
  instances: crates/fe-workbench/src/pane_group.rs:562

- F4 medium crates/fe-workbench/src/pane_drag.rs:128: Insertion markers use unbounded tab edges and are rendered outside the tab bar's scroll clipping. In a 400 px bar with a scrolled tab spanning x=-180..420, hovering at x=350 selects insertion after that tab and paints the marker at x=420, over the neighbouring pane instead of inside the target bar. Long filenames and narrow panes make this reachable.
  fix: Constrain the marker to the visible tab-bar bounds and render it within an appropriate clip; add coverage for long tabs and horizontally scrolled bars.
  instances: crates/fe-workbench/src/pane_drag.rs:128, crates/fe-workbench/src/pane.rs:491

## Lead notes

All findings checked on main 4037dd1. All four still hold: F1 and F3 (move_tab's `only && pane == *source` guard covers only Split, and `target == *source` still calls Pane::reorder, which activates; pane_group.rs:547-563), F2 (drag_divider matches a press by divider path and member count only; pane_group.rs:486-516) and F4 (pane_drag::marker uses unclamped tab edges, pane_drag.rs:127-137, and drop_hint paints it unclipped, pane.rs:479-499).

Scope is narrowed to the three files the fixes touch plus one new test file. Pane::dropped (pane.rs) is the drop handling the review means, so workbench.rs is not needed: reject a sole tab dropped on its own pane there, and keep PaneGroup::move_tab as the shared path for keys. The test file is declared from pane_drag.rs by `#[cfg(test)] #[path = "pane_drag_followup_tests.rs"] mod followup_tests;`. It is not declared from crates/fe-workbench/src/tests/mod.rs, which the active T123 and most ready workbench cards hold, so tests/pane_drag.rs's private helpers aren't reachable: build a small harness in the new file. workbench.rs, lib.rs, tests/mod.rs, fe-ui tab_bar.rs and fe-ui theme.rs were dropped from scope. Clip the marker in pane.rs, for example inside an overflow-hidden wrapper sized to the visible tab bar, not in fe-ui.
