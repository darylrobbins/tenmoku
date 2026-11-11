# The sidebar in B3 (T1541)

The calls T1541 made in giving the sidebar the design system's anatomy
(`docs/design/design-system.md#workbench-anatomy`; audit findings CH1, CH2,
PT2 and PT3 in `docs/decisions/25w38k498b-ui-polish-audit.md`), and the
files it touched beyond its scope list, with why.

## Design calls

- **One heading per tool window.** kiln-shell's dock title row is the
  panel's only heading: its title in UI heading (13/18, 600, `text.primary`,
  where it was `text.muted`, CH1), then the pane controls, right-aligned:
  the panel's own actions (`DockItem::actions`, `kiln_shell::PaneAction`),
  ⋯ (`DockState::set_menu`, named "More actions") and — ("Hide"). Both
  side and bottom docks draw them. — runs the host's
  `DockState::hide_command` once the click's update is over (fastedit's is
  `dock.toggle`, which moves focus out of what it hides), or else hides the
  dock (`DockView::hide_clicked`; kiln's `Dock` then sends `HidFocus` as a
  rail click does). ⋯ opens a menu under its button's right end, shifted
  to stay in the window (gpui `anchored`, 4 px margin), as a 160 px sidebar
  on the left would otherwise push its 220 px out (review F5). Opening it
  focuses it, its first item highlighted: Up, Down, Home and End move the
  highlight, Enter or Space runs the item, Escape closes it, and closing it
  gives focus back to what had it, so a command runs where focus was
  (`DockState::open_menu`, `close_menu`; review F4). A press outside closes
  it; a press on ⋯ toggles it. fastedit's offers only moving the sidebar to
  the other side, titled by its command (`dock_menu`) and rebuilt when the
  sidebar moves: Grow Dock and Shrink Dock resize whichever dock has focus,
  the terminal's maybe, so they aren't the sidebar's to offer (review F1).
- **A lone view loses its header.** A `ViewStack`'s only view, and the view
  built as its lead (`StackedView::lead`, `ViewContainer::lead`), draw no
  header and never collapse (`ViewStack::is_headless`; `set_collapsed` and
  `toggle` refuse, a saved `collapsed: true` is read as expanded, and
  `save` writes what's drawn). The Explorer's lead is the project tree, so
  "Project" over the root row is the one heading (CH2: 76 px before `src`
  is now 58). Its actions are the dock header's
  (`ViewContainer::set_actions`): Reveal Active File in Project (`target`)
  and Collapse All Folders (`fold-vertical`), named by their commands.
  Collapse All runs as its key does (`ProjectPanel::run_op`), dropping a
  reveal still waiting for a listing (review F2).
- **Structure.** The Outline view is Structure (`workbench-dock-structure`),
  collapsed at the Explorer's foot as before. Collapsed, its header shows
  the innermost symbol holding the active editor's caret
  (`outline_panel::CaretSymbol`, through `ViewContainer::set_detail` and
  `StackedView::detail`): its kind's icon in `text.faint`, its name in the
  editor's colour for the kind's highlight (`symbol_highlight`: a function
  in `function`, a type in `type`, a module in `module`…), nothing outside
  any symbol, and nothing while the outline shown is of another version of
  the text than the editor's: the outline kept on screen until the next
  lands has the old ranges (review F6). The outline already followed every
  editor notification, so the detail follows the caret. The `outline.show` command keeps its id and
  title ("Show Outline"): renaming a command title is the commands
  catalogue's change, not this card's.
- **Project tree.** The root row is the folder's name as spelled (medium
  weight, 24 px, inset to the rows' chevron column), the first row under
  the heading; rows are UI body (13/18, from 12 px). Git status uses the
  `vcs.*` tokens (`project_panel::vcs_color`), never the accent: Modified
  was `ui.accent`, Added and Untracked a hue shift of it. A file's letter
  and a folder's dot share kiln-ui's `BADGE_COLUMN`, at least 14 px, the
  letter centred in it and the dot a 6 px circle (`TreeRow::dot`,
  `DOT_SIZE`) centred in it (PT2). A longer badge, the call hierarchy's
  "recursive · 3", is as wide as its text (review F3). The focused selection was already the controls
  card's (`state.selected`, Yohen's `#3e2717`, and the 2 px accent edge).
- **Ignored rows (PT3).** `TreeRow` mutes an ignored row's label only: its
  chevron and icon keep their colours, and a coloured icon is no longer
  faded (`DIMMED_ICON_OPACITY` is gone). A cut row, drawn as an ignored one,
  follows.
- **Vim focus.** With the tree unable to collapse, `<C-w>` into the
  Explorer always lands on the tree; Structure is reached with
  `outline.show`, which focuses it.

## Text

Every user-visible string in `project_panel*.rs`, `outline_panel.rs` and
`outline_panel_keys.rs` is in `l10n/en-US/project.ftl` (`workbench-project-…`),
byte-equal in English: the expand-all cap and the outline's cap are plural
messages, the paste-into-itself toast two whole messages (moved, copied)
where it was one with the verb spliced in. The project panel's command
titles are their `command-<id>` messages (`command_title`, as fe-gpui's
registries do). The Explorer's title is `workbench-project-title`; its
`SidebarPanel` const keeps the English name the rail orders it by and tests
pin, allow-listed as not shown. kiln-shell's ⋯ and — names are
`kiln-shell-dock-more` and `kiln-shell-dock-hide`.

Text that stays on screen is stored as a `Text` and resolved as it's drawn
(review F7): a folder that couldn't be opened, Expand All's cap, the name
field's errors (an OS error's text verbatim) and the placeholders of the
name field and the filter's box. kiln-ui's `TextField::placeholder` now
takes a `Text` (strings convert as verbatim, so its callers build), and the
panel redraws itself and its open fields on a locale change.
`tests/project_text_fake_locale.rs` switches to a fake locale and back with
each on screen, through `ProjectPanel::shown_words`, a hidden public
read-out, since that test needs a binary of its own.

## Files beyond the scope list

- `kiln/crates/kiln-shell/src/lib.rs`, `kiln/crates/kiln-ui/src/lib.rs`:
  exports of the new types (added lines).
- `kiln/crates/kiln-shell/l10n/en-US/kiln-shell.ftl`: ⋯'s and —'s names.
- `kiln/crates/kiln-ui/src/text_field.rs`: the placeholder as a `Text`,
  resolved as drawn, and `placeholder_text` (review F7).
- `crates/fe-workbench/tests/project_text_fake_locale.rs`: the fake-locale
  test (review F7), in its own binary as the locale is the process's.
- `crates/fe-workbench/src/project_panel_nav_tests.rs`: Expand All's word
  read as a string.
- `crates/fe-workbench/l10n/en-US/dock.ftl`: Outline's view title became
  `workbench-dock-structure`.
- `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n-allow.toml`,
  `kiln/crates/kiln-ui/l10n-allow.toml`: the catalogue file, the migrated
  files, the panel name's and the dot's selector's allow-list entries.
- `kiln/crates/kiln-ui/src/tests/tree_row.rs`, `tree_row_status.rs`,
  `coloured_icons.rs`: pinned the old ignored-row dimming and a badge as
  wide as its letter.
- `crates/fe-workbench/src/project_panel_git_status_tests.rs`: pinned the
  Changes panel's letter colours and a folder's badge selector.
- `crates/fe-workbench/src/project_panel_filter_tests.rs`,
  `crates/fe-workbench/src/tests/lsp_file_operations.rs`: read the
  no-matches row and the renaming note, now catalogue messages, as strings.
- `crates/fe-workbench/src/dock_focus_tests.rs`: collapsed the tree to
  reach the outline, and checked an Explorer with every view collapsed.
- `crates/fe-workbench/examples/screenshots/audit/project_tree_git.rs`: the
  selected row's probe moved up 26 px with the headers gone. The audit's
  `git_panel` Commit probe was already stale before this change (since
  T1518) and still is.
- The `setup_wizard_*` goldens moved: the wizard's backdrop shows the
  sidebar.
