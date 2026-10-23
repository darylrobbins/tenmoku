---
task_uid: "01m5jp6fvryhesexjppmyqd91m"
task: "T203"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test theme_tests::open_editors_follow_the_palette passes: with a file open in a pane and in a split, setting fe_gpui::ActivePalette to EditorPalette::fastedit_light() changes every open editor's background and keyword colour from fastedit Dark's, and an editor opened afterwards in either pane starts with the new colours"
    required: true
    manual: false
  - id: "C2"
    text: "rg -n ': UiTheme\\b|UiTheme::default_dark' crates/fe-workbench/src finds no match: no workbench type or constructor keeps or builds a UiTheme copy (find_bar.rs's and the helpers' \u0026UiTheme parameters excepted)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond constructor-call updates the removed ui/theme parameters force; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no change under kiln/ and no Cargo change"
    required: true
    manual: false
---

# Make open editors and the workbench chrome follow kiln's active theme live, with no stored UiTheme copies (Mac T247)

## Problem


After the host card (T246), a theme change (the user's `theme.toml` saved, `theme.mode` changed, or the
system's appearance flipping under `mode = "system"`) changes kiln's `GlobalTheme`, which redraws every window,
and fastedit's `fe_gpui::ActivePalette`. kiln-ui's widgets (tab bar, tree rows, labels, inputs, toggles,
status entries) already read `cx.theme()` as they render and follow. fastedit's workbench doesn't: each part
keeps the `UiTheme` it was built with, and each editor the colours it was opened with.

- `crates/fe-workbench/src/workbench.rs`: `Workbench::new` takes `UiTheme::default_dark()` (line 293) into
  its `ui` field (line 225), passes copies to every panel, picker, the dock, the status bar, the toasts and the
  panes (lines 304–385), and draws the pane group with it (line 1927); re-rooting (`set_root`, line 644, copies
  at lines 664–666) and `new_quick_open` (line 499) hand out the same copy.
- `crates/fe-workbench/src/pane.rs`: `Pane` keeps `ui: UiTheme` (field, line 113) for its placeholder,
  background, toolbar, breadcrumbs, buttons and find bar, and an `EditorTheme` it clones into each new
  editor (`new_editor`, line 575). An open `EditorView` keeps its colours.
- Copies drawn from in `project_panel.rs` (field `theme`), `outline_panel.rs`, `search_panel.rs`,
  `references_panel.rs`, `nav_picker.rs` (both delegates), `toolbar_picker.rs`, `language_picker.rs`,
  `quick_open.rs`, and `status_bar.rs` (drawn in `vim_line.rs`, an `impl StatusBar`). `dock.rs` and `nav.rs`
  pass copies on to kiln-shell's `DockState` and `Picker`.
- Six more files, added since this card was written, keep or make copies too (rg on main 9424ce48; refreshed against 2b787020 below):
  `config_problems.rs` (`ConfigProblemsPicker`'s `ui`, line 192, drawn at line 375), `git_panel.rs`
  (`GitPanel`'s `ui`, line 105, drawn at lines 310 and 443), `lsp_rename.rs` (`Picker::new(…,
  UiTheme::default_dark(), cx)`, line 106), `problems_panel.rs` (`ProblemsPanel`'s `ui`, line 134, drawn at
  lines 329 and 478), `recent_picker.rs` (`RecentPicker`'s `ui`, line 289, drawn at lines 448–456) and
  `toast.rs` (`ToastLayer`'s `ui`, line 369, drawn at line 451). C2's `rg` can't pass while any of them keeps
  one, so they're in this card's scope.

The user asked for "hot reload when the theme file or the setting changes" (plan approved 2026-10-08).

**Decided** (user, 2026-10-11): "Use kiln's theme service". The workbench follows kiln's active theme
(`fe_ui::ui_theme(cx)`, kiln-ui's view of `GlobalTheme`) and the host's `fe_gpui::ActivePalette`. It never
reads the setting or a theme file, and it adds no theme global or observer of its own for the chrome: kiln
redraws every window on a change, so reading the active theme while rendering is enough.

**kiln-shell's side is on main** (ba016a0, 2026-10-11). kiln-shell's `Shell`, `DockState`, `PaneState` and
`ItemPane`, `PaneGroup`'s split borders, `Picker` and `CommandPalette` read `kiln_ui::ui_theme(cx)` as they draw
and follow a theme change (kiln-shell's `tests/theme_follow.rs`). Their constructors keep a `UiTheme`
parameter that they ignore, left for a later kiln API cleanup: `Shell::new`, `DockState::new`,
`PaneState::new`, `Picker::new`, `CommandPalette::new` and `PaneGroup::render`'s `_ui`. kiln-ui's widget
constructors (`TextInput::new`'s `_theme` …) are the same. The same commit changed fe-workbench by one line:
`Pane`'s render calls `self.state.drop_hint(cx)` (`pane.rs`, line 903). This card changes nothing under
`kiln/`.

## Change

- **Editors.** `Pane::new` observes `fe_gpui::ActivePalette`. On a change it calls
  `self.theme.set_colors(&palette)` (so editors opened later in the pane, or in a split made from it, inherit
  it) and `EditorView::set_colors` on every tab's editor, and notifies.
- **Chrome.** No fe-workbench type keeps a `UiTheme`. Each `render` (and each helper it calls) reads
  `fe_ui::ui_theme(cx)` instead:
  - remove the `ui` field of `Workbench`, `Pane`, `OutlinePanel`, `SearchPanel`, `ReferencesPanel`,
    `StatusBar`, `QuickOpen`, `ToolbarPicker`, `LanguagePicker`, `DefinitionsPicker`, `SymbolsPicker`,
    `ConfigProblemsPicker`, `GitPanel`, `ProblemsPanel`, `RecentPicker` and `ToastLayer`, and `ProjectPanel`'s
    `theme`; `lsp_rename.rs` passes `fe_ui::ui_theme(cx)` to `Picker::new`;
  - remove the `ui`/`theme` parameter of their constructors, of `Dock::new`, `Nav::new` and
    `Workbench::new_quick_open`, and update the call sites (in `workbench.rs`, `pane.rs`, `nav.rs`);
  - kiln-shell and kiln-ui constructors that still take a `UiTheme` they ignore (`Shell::new`,
    `DockState::new`, `PaneState::new`, `Picker::new`, `TextInput::new` and the like) are passed
    `fe_ui::ui_theme(cx)` at the call. fe-workbench's `pane_group.rs` (`render(&self, ui: &UiTheme, …)`, which
    hands it to kiln-shell's `PaneGroup::render`) keeps its signature and doesn't change: `workbench.rs`
    passes `&fe_ui::ui_theme(cx)`;
  - helpers that take `&UiTheme` (`find_bar.rs`'s render, `search_panel.rs`'s `line_label`,
    `references_panel.rs`'s `label`, `nav_picker.rs`'s `row`) keep their signature and are passed the theme
    read at render. `find_bar.rs` doesn't change.

`Workbench::new`'s signature doesn't change: the `EditorTheme` it takes stays the starting editor colours,
which the host builds from the active palette. `Workbench::reload_settings` (line 1744) keeps refreshing
`set_restore_style` from the active pane's `EditorTheme`. Because panes now follow the palette, that theme is
the current one.

## Tests

`crates/fe-workbench/src/theme_tests.rs` is a new file, with one `#[cfg(test)] mod theme_tests;` line in
`crates/fe-workbench/src/lib.rs`. Don't use `crates/fe-workbench/src/tests/mod.rs`: most queued workbench cards
edit it. Use `EditorPalette::fastedit_light()` as the changed palette: its background and keyword colour
differ from fastedit Dark's.

## Decided by the lead overnight (2026-10-12, scope widened, not respecced)

- C2 couldn't pass inside the old scope: the six files above hold copies. They join the scope; line numbers are
  refreshed against main 9424ce48 (the worker re-checks them at its base).
- It waits for T586 (argon's terminal cards T578, T579 and T586 arrive with stack 6 and change `workbench.rs`
  and `terminal_panel.rs` construction); any terminal panel or terminal find row built with a `UiTheme` copy by
  then is converted the same way if it's in this scope, else noted for a follow-up.

## Refresh against main 2b787020 (neon, zz76)

Line numbers above are from older mains: re-run `rg -n 'UiTheme' crates/fe-workbench/src` at your base and work from that. Files holding `UiTheme` today (non-test): config_problems, find_bar (unchanged), git_history, git_log, git_panel, language_picker, lsp_code_actions, lsp_rename, markdown_preview, nav_picker, nav, outline_panel, pane, problems_panel, project_panel, quick_open, recent_picker, references_panel, search_panel, status_bar, terminal_find, theme_picker, toast, toolbar_picker, vim_pick, workbench_palettes, workbench. Files added since the card was written (git_history, git_log, lsp_code_actions, markdown_preview, terminal_find, theme_picker, vim_pick, workbench_palettes) are in scope and converted the same way; find_bar.rs keeps its signature. The terminal-card wait (T586) is satisfied: terminal_find.rs is converted here.

## Implementation notes

- **Editors.** `Pane` keeps a `_palette` subscription to `fe_gpui::ActivePalette` (`Pane::follow_palette`): it
  recolours the pane's `EditorTheme` and every tab's editor from `fe_gpui::editor_palette(cx)`. A split's pane is
  made by `new_peer` from that theme, so it starts in the current colours too.
- **Chrome.** Every type listed keeps no `UiTheme`; renders and the helpers they call read `fe_ui::ui_theme(cx)`.
  Picker delegates read it in `render_match`, whose `cx` was unused before. kiln constructors that ignore their
  theme (`Picker::new`, `TextInput::new`, `IconButton::new`, `Tooltip::new`, …) are passed the theme read at the
  call. `Pane::button` takes the `&UiTheme` its caller read; its tooltip reads the theme as it shows.
- **No-`cx` helpers.** `QuickOpen::label` and `SymbolsPicker::label` (called by tests without a context) build
  `HighlightedLabel` by its public fields, which is all `HighlightedLabel::new` does with its ignored theme.
  The test-only `shown_rows` of the Search and References panels use the highlight ranges split out of
  `line_label`/`label` (`line_highlights`, `highlights`); those helpers keep their `&UiTheme` signature.
  `git_log.rs`'s `ref_label` and `file_cell` took a `UiTheme` by value and now take `&UiTheme`. The test-only
  `GitLogPanel::muted_text` returns the muted colour the last rows were drawn with, which `RowsSeen` records.
- **`StatusBar::new()`** takes no argument now, so `StatusBar` derives `Default` (clippy's `new_without_default`).
- **Files outside the listed scope**, changed only because a constructor or field they used went away:
  `chat_context.rs`, `git_diff.rs` (`QuickOpen::new`), `workbench_root.rs` (re-rooting's panels),
  `vim_line.rs` (the `impl StatusBar` the spec names), `project_panel_ops.rs` (`render_field` read
  `ProjectPanel`'s `theme`). Two existing tests read a removed field and now read `fe_ui::ui_theme(cx)` in the
  same closure: `project_panel_git_status_tests.rs` (`panel.theme`) and `tab_git_status_tests.rs`
  (`workbench.ui`). `tests/outline_detail.rs` built `UiTheme::default_dark()`, which C2's `rg` finds, and reads
  `fe_ui::ui_theme(cx)` instead.
- **Not following live yet** (they're stored by design, outside this card's change): a tab title's git tint
  (`Workbench::tint_tabs` computes it from the accent when the Changes list changes) and the Problems panel's
  severity colours (taken from the `EditorTheme` at construction). The tint updates on the next Changes refresh.
- **Carried findings not fixed here.** T84 F1 (the rename placeholder's selection) needs a selection API on
  kiln-ui's `TextInput`, and C3 rules out changes under `kiln/`. T179 F1, F3 and F5 are Problems-panel behaviour
  (cache invalidation on file-system changes, resync on path resolution, the Save As continuation): each reaches
  into the LSP resolution, the watcher or `Pane::open_once_saved`, with its own tests. This card's edits to
  `lsp_rename.rs` and `problems_panel.rs` are only the theme plumbing, so those stay for their follow-up cards.
