---
task_uid: "01m46tzwbtvmynd7k47y8rkm3w"
task: "T79"
rev: 6
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::splits::split_right_shares_the_buffer: with a file open, cmd-backslash gives two panes side by side (laid-out bounds left and right, widths within 1 px), the new one active with a tab of the same path whose model shares_buffer_with the first; text typed in one pane appears in the other after a frame and both tabs show dirty"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::splits::split_down_nests: cmd-k cmd-backslash splits the active pane below; splitting right inside the lower pane nests a horizontal axis there; closing one collapses the axis back to a single member"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::splits::focus_follows_clicks_and_keys: a mouse down in the other pane makes it active and a tree click then opens into it; cmd-k cmd-left/right/up/down activate the geometric neighbour and do nothing where there is none; the editor's cmd-k cmd-0 fold-all still works"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::splits::move_tab_between_panes: ctrl-cmd-right moves the active tab into the right pane (creating a split when there is none); a pane emptied by the move closes; moving a path the target already shows activates that tab and closes the moved one"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::splits::closing_panes_and_tabs: cmd-k cmd-w closes the active pane and focuses its neighbour; a pane holding the only view of a dirty buffer stays open; the last pane never closes; cmd-w on the last tab of a non-last pane closes the pane; cmd-w on a dirty tab whose buffer another pane shows closes it"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::splits::the_window_title_follows_the_active_pane: activating the other pane by click changes the window title to its active tab"
    required: true
    manual: false
  - id: "C7"
    text: "pane.split_right, pane.split_down, pane.focus_*, pane.move_tab_* and pane.close are registered through the ACTION_TASK command registry with the bindings in the spec; rg 'KeyBinding::new|actions!' finds no match in the files this card adds; existing tests::tabs tests pass"
    required: true
    manual: false
---

# Split the editor area into panes, each with its own tabs

# Split the editor area into panes, each with its own tabs

Milestone M3 "Browse", third batch. It builds on:

- the shared-buffer card: `EditorModel::split` and `shares_buffer_with`
- T63's `Pane` and `Workbench`
- the action-model task's command registry

The plan's M3 scope includes "splits".

## Problem

T63's workbench has one `Pane`. Two files can't be seen side by side, and one file can't be seen in two
places.

## Change

New file `crates/fe-workbench/src/pane_group.rs`: `PaneGroup`, a tree of panes.

- Its members are `Pane(Entity<Pane>)` or `Axis { axis: Horizontal | Vertical, members }`.
- Members of an axis share its space equally, with a 1 px border between them in `UiTheme`'s border
  colour. There are no drag handles.
- Splitting in a member's own axis direction adds a sibling. Splitting across it nests a new axis.
- Removing a pane collapses an axis that's left with one member.
- One pane is **active**. Opening a file, whether from a tree click, Cmd-P, a search result or the
  outline, goes to the active pane.

`crates/fe-workbench/src/workbench.rs` holds a `PaneGroup` instead of T63's single `Pane`.

- "Active editor" and the window title follow the active pane's active tab.
- The active-editor hook the outline panel uses fires when the active pane changes as well as when the
  active tab does.

### Behaviour

- **Split right or down.** A new pane opens beside or below the active pane and becomes active.
  - If the active pane has a tab, the new pane gets a tab for the same path whose `EditorView` is built
    from `EditorModel::split()`, so both share one `Buffer`.
  - Otherwise the new pane is empty.
- **Shared buffers redraw together.** The workbench observes each `EditorView`. When one notifies,
  every other view whose model `shares_buffer_with` it is notified too, so an edit shows in both panes
  on the next frame. Each view keeps its own selections, scroll and folds.
- **Focus follows clicks.** A mouse down anywhere in a pane (tab bar or editor) makes it the active
  pane.
- **Focus follows keys.** The focus commands move to the neighbouring pane in that direction, picked
  from laid-out pane bounds: the nearest pane whose bounds overlap on the other axis. With no neighbour,
  nothing happens.
- **Move a tab.** The move commands take the active tab to the neighbouring pane in that direction.
  With no neighbour, they split in that direction first. If the target pane already shows the path,
  its tab is activated and the moved tab is closed. A pane emptied by a move closes.
- **Close a pane.** The close-pane command closes the active pane with its tabs. The dirty-tab rule
  below still applies, so a pane holding a dirty tab whose buffer no other tab shows stays open, and
  this is logged. The last pane never closes. Focus moves to the nearest pane.
- **Tabs per pane.** T63's tab commands (`cmd-w`, `cmd-shift-[` and `cmd-shift-]`) act on the active
  pane.
  - Closing a pane's last tab closes the pane, unless it's the only pane, which shows the
    empty-pane placeholder as before.
  - T63's rule that `cmd-w` on a dirty tab leaves it open still holds, **except** when another tab
    still shows the same buffer. The edits aren't lost, so that tab closes.
- `crates/fe-workbench/src/pane.rs` changes only as these need it, for example exposing tab removal
  for moves.
- `crates/fe-workbench/src/lib.rs`: a `mod` line, and one line in `init` to register commands.

### Commands

Register through the action-model task's registry. Don't add `KeyBinding::new` or `actions!`. All are
in the `Workbench` context. The scheme follows VS Code:

| ID | Default binding |
|---|---|
| `pane.split_right` | `cmd-\` |
| `pane.split_down` | `cmd-k cmd-\` |
| `pane.focus_left` / `pane.focus_right` / `pane.focus_up` / `pane.focus_down` | `cmd-k cmd-left` / `cmd-k cmd-right` / `cmd-k cmd-up` / `cmd-k cmd-down` |
| `pane.move_tab_left` / `pane.move_tab_right` / `pane.move_tab_up` / `pane.move_tab_down` | `ctrl-cmd-left` / `ctrl-cmd-right` / `ctrl-cmd-up` / `ctrl-cmd-down` |
| `pane.close` | `cmd-k cmd-w` |

The `cmd-k` chords sit beside the editor's existing `cmd-k cmd-0` and `cmd-k cmd-j`. The registry's
context precedence must keep both working. A test covers it.

### Tests

New file `crates/fe-workbench/src/tests/splits.rs`, declared in `tests/mod.rs`. It uses headless GPUI.
Pane geometry comes from laid-out bounds.

The merge cap is 1,000 changed lines, not counting tests. If the card runs over, split the move and
close commands into a follow-up.

## Out of scope

- Drag-resizing panes.
- Dragging tabs between panes.
- Persisting the layout.
- Sharing one `Syntax` between views.
- Zooming a pane.
- Different docks per pane.

## Decided (lead defaults; the user may revisit)

- Bindings follow VS Code, listed above.
- **A split opens the current file in the new pane** (VS Code's "Split Editor"), sharing its buffer.
- An emptied pane closes, unless it's the last one (VS Code's `closeEmptyGroups`).
- Panes are equal-sized, with no resize handles yet.
- A dirty tab may close when another tab still shows its buffer.

## Implementation notes

- **Two files outside the scope**, each with one setter: `search_panel.rs` and `quick_open.rs` get
  `set_pane`. Both hold the `WeakEntity<Pane>` their opens go to, fixed at construction. The
  workbench re-points them whenever the active pane changes, so a search result or a Cmd-P pick
  opens in the active pane, and never in a pane that has closed. The dock's focus fallback reads
  the search panel's pane, so it follows too.
- **Opening a file another pane shows shares its buffer.** `Pane::open` (and a read landing after
  another pane opened the same file) builds the tab from that pane's model with
  `EditorModel::split()`, as a split does. Without this, a tree click in a second pane would give
  the same file two independent buffers, and saving one would overwrite the other's edits.
- **Panes see each other** through `Peers`, the workbench's list of panes, which each pane holds.
  That's how `Pane::close_active` applies the dirty-tab rule for every close path (`cmd-w` and the
  tab's close button alike).
- **Shared-buffer redraws.** The workbench observes every open editor. When one notifies, it
  notifies each other editor whose model `shares_buffer_with` it, and records it in `echoes`, so
  that editor's own notification isn't passed back.
- **Emptied panes.** A pane whose last tab closes (by `cmd-w` or the close button) flags itself;
  the workbench's observer of the pane closes it unless it's the last. A pane emptied by a tab
  move closes at once. A split from an empty pane makes an empty pane, which stays open.
- **Moving a tab onto a file the target shows** closes the moved tab only if that loses no
  edits: if it's dirty and no other tab shows its buffer (two independent reads raced), the move
  is refused and logged.
- **Geometry.** Each pane records its laid-out bounds with a `canvas`. The neighbour in a
  direction is the nearest pane that way whose bounds overlap on the other axis, the longest
  overlap breaking a tie. "The nearest pane" a closed pane hands focus to is the one with the
  nearest centre.
- A pane removed from an axis that leaves one member collapses the axis; if that member is an
  axis running the same way as its parent, its members are spliced into the parent.
- **Reads land where they were asked for.** A pane's read that lands and takes focus (T63's rule)
  makes that pane active too, so the focused editor and the pane commands act on agree. The
  host's launch-file read goes to the pane showing it as loading, not to whichever pane is active.
  A pane whose last tab closed while a read was pending closes once that read fails.
- **Split and move act on the tab that shows.** While a file being read or a read error shows in
  the active tab's place, a split opens an empty pane and a move does nothing.
