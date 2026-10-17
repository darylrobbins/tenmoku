---
task_uid: "01m55b10yxrcvjk7af79s8g654"
task: "T811"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test kiln-shell the_active_tab_scrolls_into_view passes: in an overflowing strip, after the last tab is activated and a frame drawn, PaneState::drop_target 1 px inside the strip's right end gives DropTarget::TabBar(items.len())"
    required: true
    manual: false
  - id: "C2"
    text: "Test kiln-shell a_hand_scroll_stays_until_the_active_tab_changes passes: after scroll_tabs(px(0.)) and two more frames the strip's left end still probes TabBar(0); activating the first tab then the last reveals the last again"
    required: true
    manual: false
  - id: "C3"
    text: "Existing kiln-shell tests (pane, pane_group, tab_title_color, workspace_*) and fe-workbench's tab and drag tests pass; only kiln/crates/kiln-shell/src/pane.rs changes outside the new test"
    required: true
    manual: false
---

# Scroll the tab strip to show the active tab whenever it changes, instead of leaving a newly opened tab hidden past the strip's end

## Problem

The user's screenshot (2026-10-17, main 92c57c57): the pane shows the diff of
crates/fe-vim/tests/corpus/ex.toml, opened from the Git Log's selected row. But the tab strip shows only
`regatta.toml`, `ex.rs (…)`, `ex.rs (…)` and a cut-off `ex.expected.json (81309b1^ ↔ 8…`. The active
tab, the fifth, is past the strip's end, and nothing scrolls it into view. VS Code reveals the active tab
whenever it changes.

Cause: kiln's tab strip is an `overflow_x_scroll` div tracked by `PaneState::tab_scroll`
(kiln/crates/kiln-shell/src/pane.rs:216, :542; kiln/crates/kiln-ui/src/tab_bar.rs `TabBar::render`), and
nothing ever calls `tab_scroll.scroll_to_item`. The active index changes in `PaneView::activate_item`
(pane.rs:629), `PaneState::set_active` (:294), `insert` and `take_item`, but the strip's offset only
moves under the wheel (or `scroll_tabs`).

## Change

In kiln/crates/kiln-shell/src/pane.rs only: whenever the active index differs from the one the strip
last revealed, the strip is scrolled so the active tab is fully in view, by
`self.tab_scroll.scroll_to_item(active)` when the tab bar is built (`PaneState::tab_bar`), tracked in a
`Cell<Option<usize>>` of the index last revealed. That catches every way the active tab changes: a click,
the keyboard, an open, a close or an insert. The strip doesn't move while the active tab stays the same,
so a scroll by hand stays until the active tab changes again. Item identity matters, not just the index:
reveal again when an insert or close shifts the active item to another index.

Test in a new integration test kiln/crates/kiln-shell/tests/tab_scroll_active.rs, using only base API.
Build a host as tests/pane.rs does, a narrow window (or enough notes, about 30) so the strip overflows,
and `PaneView::activate_item` on the last tab, then draw.

Probe where the strip shows with `PaneState::drop_target(at)`, which uses the last frame's *scrolled*
tab layout and changes nothing. At a point 1 px inside the strip's right end, on its vertical centre,
it gives `DropTarget::TabBar(n)`.

- `the_active_tab_scrolls_into_view`: after activating the last tab and drawing, that probe gives
  `TabBar(items.len())` (the gap after the last tab). At base the strip hasn't scrolled, so it gives an
  earlier gap.
- `a_hand_scroll_stays_until_the_active_tab_changes`: after `scroll_tabs(px(0.))` and two more draws, a
  probe 1 px inside the strip's left end still gives `TabBar(0)` (the same active tab isn't re-revealed).
  After activating the first tab and then the last, the right-end probe gives `TabBar(items.len())` again.
