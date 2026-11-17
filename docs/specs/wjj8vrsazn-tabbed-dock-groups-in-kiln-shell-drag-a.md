---
task_uid: "01m7nk9gz5508bhrwjj8vrsazn"
task: "T416"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "dock_groups::a_tab_dropped_on_a_groups_right_quarter_splits_it_into_a_group_beside passes: a bottom tabbed dock with three open panels; dragging one tab onto the right quarter of the content draws the split indicator over the right half while hovering, and the drop leaves two groups side by side, each with its own tab strip, both groups' active panels drawn at once (shown_panels has two), the dropped panel active and its group focused"
    required: true
    manual: false
  - id: "C2"
    text: "dock_groups::a_tab_dropped_on_another_groups_strip_joins_it_at_the_pointer passes: with two groups, a tab dropped between two tabs of the other group's strip lands at that index there and becomes that group's active panel; a drop on its own group's content centre changes nothing"
    required: true
    manual: false
  - id: "C3"
    text: "dock_groups::a_tab_dragged_along_its_own_strip_reorders_it passes: dragging a tab between two other tabs of its own strip draws the insertion marker between them while hovering (none where the order would not change) and the drop moves it to that index; the new order is drawn, kept by save and restore, and kept by join_groups"
    required: true
    manual: false
  - id: "C4"
    text: "dock_groups::dragging_a_groups_last_tab_out_merges_the_group_away passes: moving the only placed panel of a group into another group (drag, and move_panel) leaves one group, drawn as a dock that was never split (the same tab ids and strip bounds); join_groups puts every panel back in one group, left to right in tab order"
    required: true
    manual: false
  - id: "C5"
    text: "dock_groups::a_split_is_refused_where_a_half_would_be_too_narrow passes: dropping a group's only open tab on its own edge does nothing, and no indicator is drawn or split made when either half would be narrower than MIN_DOCK_WIDTH"
    required: true
    manual: false
  - id: "C6"
    text: "dock_groups::the_divider_moves_width_between_neighbours_and_a_double_click_evens_them passes: dragging the divider between two groups changes their widths by the drag, each kept at least the minimum, its cursor the resize cursor; a double-click makes the groups equal"
    required: true
    manual: false
  - id: "C7"
    text: "dock_groups::hiding_a_shown_panel_collapses_only_its_group passes: with two drawn groups, hide_panel on the right group's active panel stops drawing that group (is_collapsed), the left group takes the whole width, the dock stays visible and focus group moves left; show on that panel draws the group back with the same tabs, active panel and weight; hide_panel when only one group is drawn hides the dock as set_visible(false); the dock's hide control still hides the whole dock"
    required: true
    manual: false
  - id: "C8"
    text: "dock_groups::closing_a_groups_last_open_tab_hides_the_group_but_keeps_its_place passes: closing the only open tab of the right group draws one group and hidden_panel_focused counts it hidden; showing that panel again draws it back in the right group; closing every open tab hides the dock as today; a panel shown in a non-focused drawn group is not hidden_panel_focused"
    required: true
    manual: false
  - id: "C9"
    text: "dock_groups::groups_are_saved_and_restored passes: save of a split, reordered or collapsed dock has groups with panels in tab order, active, weight and collapsed, and restore into a fresh dock gives the same, dropping unknown ids and empty groups; a default dock's save has no groups key and equals what it saved before this card; a state saved without groups restores as one group"
    required: true
    manual: false
  - id: "C10"
    text: "dock_groups::a_side_dock_stacks_its_groups_top_to_bottom passes: the same dock moved to the left edge draws its groups one above the other, its split zones are the top and bottom quarters, and its divider drags vertically"
    required: true
    manual: false
  - id: "C11"
    text: "dock_groups::groups_and_the_divider_are_named_for_assistive_tech passes: each drawn group is a Role::Group node labelled with its active panel's title, each strip a TabList, and the divider a Role::Splitter named from the catalogue"
    required: true
    manual: false
  - id: "C12"
    text: "The existing kiln-shell dock tests (modules dock, dock_position, dock_resize, dock_no_rail, dock_rail_actions, dock_full_height, dock_bottom_min_centre, persist, layout) and fe-workbench's tests::bottom_panel_b3 pass unmodified (cargo nextest run -p kiln-shell -p fe-workbench)"
    required: true
    manual: false
  - id: "C13"
    text: "kiln-shell::it.l10n::ui_text_is_in_the_catalogue and kiln-shell::it.l10n::keys_used_here_exist pass with the new divider key"
    required: true
    manual: false
---

# Tabbed dock groups in kiln-shell: drag a tool tab to a group's edge to split the dock into side-by-side groups (own tab strip and active panel each), reorder tabs within a strip, a draggable divider, hide one panel's group alone, merged away when emptied, saved and restored (Mac T1932)

# Tabbed dock groups in kiln-shell: drag a tool tab beside another to split the dock

User request (2026-11-17): "Can we add support for dragging the panes to be side
by side so I can have like the Git graph beside a terminal, for example? I think we
need a bit more flexible placement of those panes."

This card is the kiln-shell half: the model, drawing, drag and drop, divider and
saved state of groups in a tabbed `DockState`. The workbench half (commands, foot
buttons, the window layout, focus) is the follow-up card that depends on this one.

## Today

`kiln/crates/kiln-shell/src/dock.rs`: a `DockState` built `with_tabs()` (the
workbench's bottom panel, `crates/fe-workbench/src/workbench.rs`) has one `active`
panel and one tab strip (`tab_row`) of `open_panels()`; only one tool window shows
at a time. Dock tabs can be clicked and closed but not dragged. Editor tabs drag
between panes through `kiln_shell::pane` (`DraggedTab`, `TabGhost`, `DropZone::at`,
`DropZone::overlay`, `drop_hint`), and `kiln_ui::SplitPane` / `pane_group.rs` have
divider drags. Reuse those, and do not copy them.

## Change

A tabbed dock holds one or more **groups** laid side by side along the dock's long
axis: left to right at the bottom, top to bottom in a side dock. Each group has its
own tab strip, the same 34 px `TabBar` row as today, and its own active panel. The
groups are drawn together and split the column by weights.

- **Model** (new `kiln/crates/kiln-shell/src/dock/groups.rs`, declared from
  `dock.rs`). Each registered panel is *placed* in one group, in an order. Panels
  never placed go in the first group, in the order they were added, which is how
  every dock behaves today. A group is drawn while one of its placed panels is open
  and it is not collapsed. A group is removed only when it has no placed panels
  left. Closing a group's last open tab (its ×, or a terminal's shell exiting) hides
  that group but keeps the panel's placement, so showing the panel again brings it
  back in that group. When no group is drawn the dock hides, as today.
- **Hiding one panel** (new `DockState::hide_panel(panel)`, for the host's foot
  buttons and toggles). If `panel` is a drawn group's active panel and another group
  is drawn, its group *collapses*: it is not drawn, but it keeps its placement, its
  open tabs and its active panel. The other groups share the width by their weights,
  and the focused group moves to the nearest drawn neighbour. If it is the only drawn
  group, the whole dock hides (`set_visible(false)`), as today. `show(panel)`
  un-collapses its group, with `panel` active and its old weight back. The dock's —
  control still hides the whole dock. `is_collapsed(group)` is readable, and the
  `hidden_panel_focused` rules count a collapsed group's panels as hidden.
- **Focused group**: the group of the panel last shown, clicked or focused.
  `active()` and `rail_highlighted()` keep their meaning for that group: its active
  panel. Hosts that ask "which one is shown" keep working with one group. New:
  `shown_panels()` (each drawn group's active panel, in order), `is_shown(panel)`,
  `group_count()`, `group_of(panel)`, `open_panels_in(group)`.
  `open_panels()` and `DrawnDock::tabs()` keep returning every open panel or tab,
  group by group. `show(panel)` shows it in its own group and focuses that group.
  `hidden_panel_focused` counts a panel as shown if it is any drawn group's active
  panel.
- **Moves, for drag and for host commands**: `move_panel(panel, to)`, where `to` is
  into group *g* at tab index *i*, or into a new group before or after group *g*.
  Also `join_groups()` (everything back into one group, left to right, keeping each
  group's tab order) and `focus_next_group(by)`. Moving a group's last placed panel out removes
  that group. Joining removes the extra groups. No move crosses docks: a drag that
  comes from another dock is ignored.
- **Drag and drop**: a dock tab drags (`DraggedDockTab { dock, panel }`, with the
  `TabGhost` ghost).
  - Over a group's content, `DropZone::at` along the dock's axis picks where it
    drops. The outer quarter on the leading or trailing side splits the group: a new
    group on that side, the tab active in it. Anywhere else joins that group.
  - Over a group's strip, it drops at the slot under the pointer: into another group
    at that index, or, on its own group's strip, a **reorder** to that slot (the
    insertion marker is drawn between the two tabs it would land between; no marker
    where the drop would leave the order unchanged).
  - While a drag hovers, a drop indicator is drawn: the tinted half that a split
    would fill (`DropZone::overlay`), or the strip's insertion marker, as the editor
    panes draw them. The indicator goes when the drag leaves or ends.
  - Dropping a group's only open tab on its own group does nothing. A split is
    refused, with no indicator, when either half would be narrower than
    `MIN_DOCK_WIDTH` (shorter than `MIN_DOCK_HEIGHT` at a side).
  - Dragging the last tab of a group into another group merges that group away.
- **Divider**: a 1 px border between adjacent drawn groups, with a 6 px grab area
  and the resize cursor (as `SplitPane`). Dragging it moves weight between the two
  neighbours, each kept at least the minimum. A double-click evens all the groups.
- **Controls**: each group's strip ends in its own active panel's `actions()`. The
  dock-wide ⋯ and — controls are drawn once, at the end of the last group's strip.
  With one group, the dock draws exactly as it does today.
- **Saved state**: `DockState::save` writes `"groups"`:
  `[{"panels": [ids in tab order], "active": id|null, "weight": f, "collapsed": bool}]`,
  but only when the layout differs from the default (one group, panels in the order
  added, none collapsed). A default dock saves exactly what it saves today.
  `restore` puts the groups back for registered ids. It drops unknown ids and empty
  groups, and a state saved without `"groups"` restores as one group.
- **Accessibility**: each drawn group is a `Role::Group` node labelled with its
  active panel's title. Each strip stays a `TabList` of `Tab`s. The divider is a
  `Role::Splitter` named from the catalogue (new key in
  `kiln/crates/kiln-shell/l10n/en-US/kiln-shell.ftl`, e.g.
  `kiln-shell-dock-group-divider = Resize panel groups`). The dock column's
  `Region` landmark is unchanged.
- `DrawnDock` gains test reads: `group_bounds(g)`, `divider_bounds(g)`,
  `group_tabs(g)`, and the drop indicator drawn (`drop_hint()`: which group, and
  split before, split after, join or reorder at an index).

## Not in this card

- The workbench's commands, foot-button lighting, window layout and focus: the
  follow-up card.
- Moving tool windows between the bottom dock and the sidebar.
- A tab context menu.
- Vim's window keys between groups: a separate card in fe-workbench.

## Pins

With one group nothing changes. The existing kiln-shell dock tests
(`tests/dock.rs`, `dock_position.rs`, `dock_resize.rs`, `dock_no_rail.rs`,
`dock_rail_actions.rs`, `dock_full_height.rs`, `dock_bottom_min_centre.rs`,
`persist.rs`, `layout.rs`) and fe-workbench's `tests::bottom_panel_b3` must pass
unmodified. No screenshot golden should move: if one does, the single-group drawing
changed, and that is a bug. New tests go in the new
`kiln/crates/kiln-shell/tests/dock_groups.rs`, a module of the one `it` binary
(listed by build.rs, no list to edit).

Depends on T1928: it changes `dock.rs`'s strip controls (PaneActions for the
terminal), and this card builds the per-group strip on top of that.

## Implementation notes

- The model lives in `kiln/crates/kiln-shell/src/dock/groups.rs` as a second
  `impl DockState`: `DockState::active` is now the focused group's active
  panel, and the dock-wide `active` field is gone. A panel has a tab when it is
  in `opened`, or it is the focused group's active panel while the dock shows,
  which is the rule the dock had before. Before focus moves to another group,
  the old focused panel is put in `opened`, so it keeps its tab.
- `MoveTo::Group { index }` is the panel's index among the group's open tabs
  once it has moved. `DockDrop::Insert { index }` is the slot the marker is
  drawn at, before the tab drawn there now. A drop converts the slot to the
  index.
- Drops that would change nothing are refused, with no indicator: a reorder to
  the same slot, a drop on the tab's own group's centre, and splitting a group's
  only tab off on its own edge or next to the group beside it.
- `pane::DropZone::along(bounds, point, axis)` is new: `DropZone::at` along one
  axis only. The groups use it, so the corners of a group's content still split
  along the dock's axis.
- `DockState::follow_focus` makes the group of a shown panel the focused group
  when keyboard focus moves into that panel. It reacts only when focus moves, so
  a `show` elsewhere is kept. `Dock` calls it as it draws. A press in a group's
  cell also focuses that group.
- With one group drawn, the dock looks the same as before. In a tabbed dock,
  the title row and the panel now sit in one `Role::Group` cell, labelled with
  the panel's title, inside the column's unchanged `Region` or `Complementary`
  landmark. The column holds the group cell or the groups' row, then the edge
  handle, then an empty drop layer. `DrawnDock::title_bounds` is the first
  group's strip row. Dock tabs now carry a drag. The tab strip ids are numbered
  across the groups, so the a11y recorder sees each one.
- Layout divides the drawn groups by each one's share of their total weight,
  so repeated splits, hidden groups and closed groups never leave space
  unused. The stored weights are what `save` writes.
- Drops go through `DockView::tab_dropped`. `Dock` emits `HidFocus` when a
  drop hides the focused panel. When `follow_focus` changes the focused group,
  `Dock` notifies its observers after the frame.
- A `restore` without `"groups"` resets the dock to one group, with the panels
  in the order they were added. Restoring `"groups"` gives each group's saved
  `active` panel its tab, and keeps the saved collapse. Only the saved
  top-level `"active"` picks the focused group, and only when its group is not
  collapsed; otherwise the first group that is not collapsed is focused. A
  group with no tab saves `"active": null`.
- `kiln/crates/kiln-shell/Cargo.toml` gains a dev-dependency on kiln-ui with
  `test-support`, for `kiln_ui::nodes` in C11's test. This is outside the
  card's listed files and does not change `Cargo.lock`.
