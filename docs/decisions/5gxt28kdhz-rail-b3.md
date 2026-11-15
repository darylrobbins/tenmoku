# The rail in B3 (T1540)

The calls T1540 made in giving the activity rail the design system's
anatomy (`docs/design/design-system.md#workbench-anatomy`), and the files it
touched beyond its scope list, with why.

## Design calls

- **Measures.** kiln-ui's `RAIL_WIDTH` is 42 px; a `RailButton` is 42 by
  `RailButton::HEIGHT` (32 px), with a 16 px icon. The rail has 4 px of
  padding above its first button and below its last. A bottom dock's row of
  rail buttons is a button's height plus its border.
- **Surface.** The rail draws on `surface.chrome`, as the design's colour
  table puts it ("Title bar, rail, tab strip, status bar"), and `Badge`'s
  default ring already assumed.
- **Selection.** A highlighted button has a `state.selected` tab from 4 px
  inside the window's edge to the rail's panel-facing edge (its outer
  corners rounded 4 px), and the 2 px `accent.base` bar on that edge. The
  1 px line between rail and sidebar moved to the same edge: before, it was
  always on the rail's left, which faced the window's edge on a left rail.
  `ActivityRail::on_left` sets the side; kiln-shell's dock passes its
  position.
- **Badges.** The host sets them (`DockState::set_badge` with a
  `kiln_shell::dock::RailBadge`), not each `DockItem`, because Changes is a
  `ViewContainer`, kiln-shell's own `DockItem`, which fastedit can't
  extend. **T1829 revises the placement:** the design system says a badge sits at the icon's bottom-right corner with a 2 px cut-out ring and "never on top of the glyph", which this reads as never over the glyph's middle; the ring exists to cut the corner. The ring's bottom now sits on the button's bottom edge and its right edge 3 px inside the button's, so a badge stays wholly inside its own button, never reaches the next one (the Changes count had read as the Chat button's), and stacked badges never touch. It is an ordinary child of the button, no longer deferred (T1540 F2: deferred painting put it above the setup wizard's scrim). Changes uses `Icon::FileDiff`. `badges_stay_clear_of_the_glyphs` and `rail_badge_placement` pin this. Its ring is the chrome, blended
  with the selection's tint under a highlighted button. Each badge is a named status node
  ("3 changed files", "2 errors").
- **Accessibility.** Rail buttons are now `Role::Button` with the panel's
  or action's title as their name, and `aria_toggled` for the selection.
- **The foot.** `DockItem::on_rail_foot` puts a panel's button in the
  bottom group, before the `RailAction`s. Problems is there and is now
  always on the rail (it joined once shown before). The Git log is a rail
  action running `git.log`, which shows Source Control with Graph expanded:
  it isn't a panel of its own. Its button isn't highlighted; Changes' is
  while Source Control shows.
- **`workbench.rail` and the saved layout.** The setting decides the
  sidebar's side. A window is built on the app's setting; its own settings
  move it if they differ; a reload that changes the setting moves it
  (`Workbench::move_sidebar`, which `dock.move_sidebar_left`/`_right` run,
  so the terminal's dock gives way as it does for them). A reload for
  another key leaves a sidebar the commands moved. Restoring a saved
  layout keeps the sidebar where the setting put it and puts the
  terminal's dock back at its saved edge unless that's the sidebar's
  (`dock_layout::displaced`): every layout saved before this change says
  `right`, the old default, and would otherwise override the new default
  and any explicit setting at each launch.
- **Text.** `dock.rs`'s text (the Folders and Outline views' titles, the
  rail actions' titles, the badges' names) is in `dock.ftl`; `dock_layout.rs`
  has none (its position names are a saved layout's keys). The panels'
  names stay in their modules' `SIDEBAR` consts, migrated by those panels'
  cards. The dock follows a change of language (`ActiveLocale`): the
  actions, views and badges take their text anew.

## Files beyond the scope list

- `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n-allow.toml`:
  the spec's i18n section asks for `dock.ftl` in `resources!` and the files
  in the `migrated` list.
- `crates/fe-workbench/src/workbench_chrome.rs`: `LayoutKeys::restore`, for
  the saved-layout rule above. The two restore paths (the workspace's
  `on_restore` and `restore_layout` without panes) both go through it.
- `crates/fe-workbench/src/sidebar_panels_tests.rs`,
  `explorer_views_tests.rs`, `git_panel_tests.rs`, `chat_rail_tests.rs`:
  they pin the old names ("Explorer", "Source Control"), icons
  (`MessageSquare`, `X`) and rail (Problems joining once shown, Terminal
  alone at the foot). `sidebar_panels_tests`' `rail` helper zipped the
  registration order with the drawn icons, which no longer match once the
  foot draws after the top group; it reads the drawn ids
  (`DrawnDock::rail_ids`, new) instead.
- Tests that assumed the sidebar on the right, the old default:
  `dock_focus_tests.rs` and `dock_leave_tests.rs` (vim's `<C-w>` walks,
  written around a right sidebar) move it right first, through
  `Workbench::move_sidebar`; `dock_position_tests.rs` starts from the left
  and pins the saved-layout rule above; `dock_resize_tests.rs` drags the
  left dock's inner edge rightwards; `zen_mode_tests.rs` measures the pane
  from the sidebar's edge; `ai_changes_panel_tests.rs` expects the left.
  `chat_panel_tests.rs`, `source_control_views_tests.rs`,
  `git_history_tests.rs` and `tests/references_panel.rs` pin the old
  names, icons or rail.
- `crates/fe-workbench/examples/screenshots/audit/*.rs`: the UI audit's
  probes (not gated; `screenshots -- audit`) sat in a right sidebar and an
  editor at the window's left. They moved with the layout: panel probes by
  −676 px, editor probes by +282 px, the rail's to its new place on
  `surface.chrome`; the prompt's scrim probe and the Git log card's (now
  over the sidebar) were re-picked from the captures, as was the
  shortcuts editor's first (its search field is the panel's colour). The
  git panel's Commit-button probe was already stale (since T1518 a
  disabled button is drawn at 50% of its enabled look, not
  `state.disabled`); it moved with the panel and still fails, as before.
- `crates/fe-config/src/tests/fixtures/qps-ploc/settings-template.ftl`:
  regenerated with the other `UPDATE_GOLDEN=1` catalogue files for the new
  `workbench.rail` template line.
- `crates/fe-workbench/src/tests/mod.rs`: declares `rail_b3`.
- `kiln/crates/kiln-ui/l10n-allow.toml`: allows the rail icon's debug
  selector, `rail-icon:{icon_id}`, which the badge-clearance tests read.
