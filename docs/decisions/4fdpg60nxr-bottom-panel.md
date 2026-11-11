# T1548: Bottom panel: Problems, Terminal and Git log as bottom tool windows

Spec: `docs/specs/4fdpg60nxr-bottom-panel-problems-terminal-and-git.md`;
design: `docs/design/design-system.md#workbench-anatomy` (Bottom panel:
Problems, Runs, Terminal, Git log; own header, own tabs; 250 px, resizable).

## What changed

- **kiln-shell `DockState::with_tabs`.** The bottom variant of the dock gets
  tabs, as the spec asks, but as an opt-in builder that works at any
  position, so a bottom dock moved to a side (`terminal.move_to_left`) keeps
  its strip. The title row becomes kiln-ui's `TabBar`, the same strip and
  `Tab`s the editor's pane draws, at `TAB_HEIGHT` (34 px). After it come the
  pane controls (the active panel's actions, ⋯ if the dock has a menu, then
  —) on the strip's chrome ground, laid out like `Pane::tab_row`. There is a
  tab for each *open* panel: one shown since, and not closed by its tab's ×
  (`DockState::close_tab`), plus the active one. Tabs are in the order the
  panels were added. Closing the shown tab shows the next one, else the one
  before; closing the last hides the dock. `DockView::tab_clicked` and
  `tab_closed` are hooks with defaults. Kiln's `Dock` overrides them to emit
  `HidFocus` when a hidden panel keeps focus. No `DockEvent` variant was
  added: `Workspace` refocuses on every `DockEvent`.
- **Rail action badges and clicks.** `set_badge` on a `RailAction`'s id now
  draws on that action's button, so Problems keeps its error dot.
  `DockView::action_clicked` lets a host take an action's click; by default
  it runs the command as before. `run_action` is public so a host can still
  run it.
- **fe-workbench.** The bottom dock (still `Workbench::terminal_dock`, so the
  ~20 files using it are unchanged) holds Problems, the terminal and the Git
  log, in the design's order (Runs, when it comes, goes after Problems).
  It is built `.without_rail().with_tabs().size(BOTTOM_PANEL_HEIGHT)`, 250
  px, and stays hidden at launch. The sidebar's rail foot is three actions:
  Problems (`problems.show`), Git log (`git.log`) and Terminal
  (`terminal.toggle`). Each is lit while the bottom dock shows its tool
  window. Clicking a lit one hides the bottom panel, focusing the editor if
  focus was in it, as a lit panel button hides the sidebar. Problems is no
  longer a sidebar panel: `DockPanel::ALL` is four, and
  `DockPanel::Problems` stays as the id its button and badge use.
  `Dock::problems()` and `Dock::git_log()` keep returning the entities, so
  the problems and git code that reaches them through the sidebar is
  untouched.
- **Git log.** It leaves Source Control (now Changes alone, a lone view,
  so it draws no view header) and is its own tool window with the id
  `git_log`, the id it had before it was Graph. Its tab says what it lists
  (`GitLogPanel::title`: "Graph: main", "Graph: All Branches"), as Graph's
  header did. `q` in it hides the bottom panel instead of the sidebar.
- **Saved layouts.** The bottom dock saves its sizes, as the terminal's dock
  did, plus `"shown"` when it shows Problems or the Git log. A restore shows
  that tool window again, unfocused; the terminal still comes back hidden,
  with no session. This keeps `tests/git_log_restore_first_list.rs` passing:
  a session saved with the Git log shown lists it again on restore. A layout
  saved earlier, with Problems or the Git log active in a shown sidebar,
  shows that tool window in the bottom panel the same way. The sidebar
  keeps its own default panel, and a saved `graph` view under Source
  Control is ignored (`old_layouts_load`,
  `an_old_layout_on_git_log_restores_the_log`).

## Files outside the scope list, and why

- `source_control.rs`: the `#[path]` submodule of `dock.rs` that built
  Graph. Dropping the Git log from Source Control can't be done without
  editing it (`GRAPH` and `graph()` would otherwise be dead code).
- `dock_focus.rs`, `dock_leave.rs`: vim's `<C-w>` keys assumed the bottom
  dock always showed the terminal. With Problems shown there, `<C-w>j`
  would have focused the hidden terminal. They now focus the tool window
  the bottom dock shows (`Workbench::focus_bottom_panel`) and count its
  tool windows as the bottom dock's (`bottom_panel_focused`).
- Tests that pinned the old placement, each now asserting the new one:
  `sidebar_panels_tests.rs`, `source_control_views_tests.rs`,
  `git_log_tests.rs`, `git_history_tests.rs`, `problems_panel_tests.rs`,
  `quickfix_panel_tests.rs`, `chat_panel_tests.rs`, `chat_rail_tests.rs`,
  `explorer_views_tests.rs`, `ai_changes_panel_tests.rs`, `tests/rail_b3.rs`,
  `layout_compat_tests.rs` and `dock_resize_tests.rs` (the bottom panel's
  250 px). Three need room or width, not a different answer:
  `git_log_open_tests::window`, whose log and diff tests click rows, gives
  the log 500 px in a 1600 px window, the room Graph had in the sidebar
  with the editor's height unchanged; the two narrow-list tests
  (`git_log_file_rows_tests::narrow`, `git_log_tests::rails_clip_alike_when_narrow`)
  move the bottom dock to a side at its least width, as they narrowed the
  sidebar before; and `git_panel_vim_nav_tests::first_last` uses a shorter
  window, since Changes alone fills the sidebar.

## Carried finding T1541 F8 (menu focus lifecycle)

Fixed in `kiln-shell/src/dock.rs`. `open_menu` watches its focus
(`on_focus_out`): when focus moves elsewhere, as `search.show` moves it,
the menu closes and leaves focus there. Hiding the dock (`set_visible(false)`
and `toggle`) closes the menu. fe-workbench's `dock.toggle` closes it
first through `close_menu`, so focus returns to what had it before the menu
opened. `hidden_panel_focused` also counts the menu's focus while the menu
isn't shown, so a host that hides the dock some other way still moves
focus somewhere shown. Tests:
`bottom_panel_b3::the_menu_closes_as_a_key_moves_focus_out_of_it` and
`hiding_the_dock_closes_its_menu`.

## i18n

Nothing new to translate. The new rail action's title reuses
`workbench-problems-title`. The Git log's tab text is `GitLogPanel::title`,
already in `git_log.rs`, which is not migrated, and this card only moves it.
`dock.rs`, `dock_layout.rs`, `workbench_chrome.rs` and `source_control.rs`
were already on `l10n-allow.toml`'s migrated list. `workbench.rs` is left
to the last fe-workbench card, as the spec says.

## Review round 1 (35hvgz)

- **F1, a shell exiting:** the last shell exiting now closes the Terminal
  *tab* (`close_tab`), not the whole bottom panel. If Problems or the Git
  log is open, it shows in the terminal's place. Focus moves to the editor
  only if the close hid what had it (`leave_hidden_panel`). An exit while
  another tool window, or the sidebar, has focus leaves focus there.
  `terminal_panel_tests::exit_hides_the_panel` pinned the old focus steal
  and now asserts the tree keeps focus; covered by
  `bottom_panel_b3::the_last_shell_exiting_closes_the_terminal_tab_alone`.
- **F2, open tabs:** kiln's `DockState` now records the shown panel as open
  however it came to show: `show`, `set_visible(true)`, `restore`, or a dock
  built shown. Switching away no longer drops a restored or initial tab. A
  workbench dock hidden since it was built still has no tabs. Tests:
  `dock_position::a_tabbed_docks_shown_panel_keeps_its_tab_however_it_came_to_show`,
  `bottom_panel_b3::a_shown_tool_window_is_saved_and_restored_with_its_tab`
  and `old_layouts_load`.
- **F3, Zen Mode:** `leave_hidden_panel` now counts the bottom dock's hidden
  tool windows as well as the sidebar's. Zen Mode's own check uses
  `bottom_panel_focused`. Leaving Zen Mode brings Problems or the Git log
  back; before, the panel came back only if the terminal had a session.
  Test: `bottom_panel_b3::zen_mode_takes_focus_out_of_the_bottom_panel`.
- **F4, where up goes back to:** Problems and the Git log keep their
  came-from-the-sidebar origin in `Workbench::bottom_from_sidebar`. It is
  cleared by focus-out listeners on both, and reset when entering from the
  editor (`focus_dock_beyond`). The terminal keeps its own. Test:
  `bottom_panel_b3::up_from_a_tool_window_goes_back_only_to_where_a_key_came_from`.
- **F5, tests that measured nothing:** the sidebar width tests
  (`dock_position_tests::assert_panels_fill`, `panel_width_tests`) assert the
  panel they show is active, and no longer list Problems. Both check
  Problems in the bottom dock moved to a side at the same widths
  (`assert_problems_fills_a_side`). `vim_day_one_tests` shows Problems in
  the bottom panel and checks it stays shown across `:cclose`.
