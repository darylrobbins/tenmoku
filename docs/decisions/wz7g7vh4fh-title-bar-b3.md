# The title bar in B3 (T1538)

The macOS title bar is now the design's 38 px bar, drawn on `surface.chrome`
with no line under it. The traffic lights sit at (14, 12), and
`TRAFFIC_LIGHT_INSET` is 80. Left of the bar are back, forward, the project
widget and the branch widget, side by side with no slash between the two
widgets. Right of the bar are search and a settings gear. The run widget and
the agent chip are not in this card.

## How it's built

- **kiln-shell slots.** `TitleBar::items(TitleBarItems { left, right })`. A
  slot is a closure that builds its element each time the bar draws, so it
  reads the app's state at draw time. The left items take the centred
  title's place, starting after the lights. The right items sit at the far
  end, 8 px from the edge. A bar with only right items keeps its title
  centred. A press on a slot belongs to the item: each slot stops the mouse
  down, so neither a click nor a double click on an item moves or zooms the
  window. `Shell::set_title_bar_items` passes the items through.
  Workbench::set_title_bar sets them from `fe-workbench/src/title_bar.rs`.
- **Widgets.** Back, forward, search and settings are kiln-ui `IconButton`s:
  ghost buttons with a hover fill, keyboard focus, an accessible name and
  `aria-keyshortcuts`. The project and branch widgets are ghost kiln-ui
  `Button`s with the label, then a chevron. Every tooltip comes from kiln-ui
  `Tooltip`: the item's name, then its key as the keymap has it when the
  tooltip shows. A click runs the command from the mouse, as the pane
  toolbar's buttons did:
  - back runs `nav.back` and forward runs `nav.forward`;
  - the project widget runs `workbench.open_recent`;
  - the branch widget runs `git.switch_branch`;
  - search runs `quick_open.toggle`, as its key does;
  - the gear runs `workbench.open_settings_ui`.

  Neither click moves focus.
- **Long names (review awqdtn F1).** The project widget is at most 240 px
  wide and the branch widget at most 200 px. In a narrow bar both shrink and
  their labels end in an ellipsis, so neither pushes the other out of view.
  For that, kiln-ui's `Button` (`controls.rs`, outside the scope list) puts
  its label in a box that truncates and keeps its icon at 14 px. That is
  the same layout as before, unless the caller lets the button shrink:
  every golden that draws `Button`s still matches (fe-workbench,
  fe-chat-view, fe-markdown-view and kiln's gallery).
- **Branch.** The widget shows HEAD's branch, or its short id when detached.
  It is hidden outside a repository. `set_branch_item` updates it with the
  status item, which stays where it is for now; the status bar card removes
  that.
- **Back and forward** were removed from the pane's breadcrumbs row. Their
  enabled state is now the workbench's alone (`Workbench::nav`), and only a
  change notifies. The buttons act on the active pane. Before, a click in an
  inactive pane activated that pane first; the bar belongs to no pane.

## Keys

`git.switch_branch` has no default key. Its tooltip reads "Switch Branch…" and
gains the key once one is bound. A test binds `alt-cmd-b` and reads it there.
Every other item has a default key and shows it.

## Files outside the card's scope list

- `kiln/crates/kiln-shell/src/shell.rs` and `src/lib.rs`: the shell draws the
  bar, so the slots reach it through `Shell::set_title_bar_items`.
- The back and forward arrows were never in `kiln-ui/src/breadcrumbs.rs`. They
  were in fe-workbench's pane toolbar row: `src/pane.rs`, plus the per-pane
  `nav` field in `src/pane_toolbar.rs`. `breadcrumbs.rs` is unchanged. Moving
  the arrows also touched these files:
  - `src/workbench_palettes.rs`: `set_nav_buttons` no longer pushes into
    panes.
  - `src/workbench.rs`: the `title_branch` field.
  - The tests that drove the pane's arrows now drive the bar's:
    `tests/toolbar_history.rs` and `toolbar_history/goto.rs`, `tests/nav.rs`
    and `tests/pane_toolbar.rs`.
- `kiln/crates/kiln-platform/tests/capabilities.rs` pinned the lights at
  (12, 9); it now pins (14, 12).
- `tests/mod.rs` declares `title_bar_b3`. `src/l10n.rs` lists `chrome.ftl`.
  `l10n-allow.toml` adds the three files to `migrated`, plus the two ref-name
  ids (`refs/heads/{name}`, `refs/remotes/{}`) as allowed literals. The spec
  asks for all of these.
- `examples/screenshots/audit/hover.rs`: the card moved down 6 px with
  everything else and put its probe on the signature's text. The probe now
  sits right of the text, still on the card.
- kiln's six gallery goldens
  (`kiln/crates/kiln/examples/gallery-screenshots/golden/*`): the gallery
  draws kiln's `Root`, whose bar is now 38 px. They were regenerated with
  `UPDATE_SCREENSHOTS=1` and checked by eye. A compare run matches all six.

## Known, not this card's

The audit's `git_panel` probe at (893, 160) expects `state.disabled`. Since
the B3 controls card (T1518), a disabled primary button keeps its accent at
half opacity, so that point reads the Commit button's fill, or the panel just
above the button now that the button sits 6 px lower. The controls card landed
after the audit card, so the probe was stale before this card (reasoned from
the history, not run at base). This card leaves it alone.
