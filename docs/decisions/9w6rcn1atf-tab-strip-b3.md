# Tab strip, breadcrumbs and the pane menu in B3 (T1542)

Spec: `docs/specs/9w6rcn1atf-tab-strip-breadcrumbs-and-the-pane-menu.md`;
design: `docs/design/design-system.md#workbench-anatomy`; audit CH3, CH4, TH5.

## What changed

- **Tabs** (kiln-ui `tab_bar.rs`): `TAB_HEIGHT` is 34 px, which the dock
  header and the terminal panel's strip already share ("panel tab strips
  match"). The bar is on `surface.chrome`. A tab has a `state.hover` fill
  under its content (an absolute first child, so the overlay never covers
  the label) and, when it's the active tab of the focused pane, a 2 px
  `accent.base` line along its top (`tab-accent:<label>`). The close mark is
  `Icon::X` at 12 px in a 16 px box, hover `state.hover`. A pinned tab is
  its icon alone: the pin glyph in its corner went; its dirty dot stays.
- **Focused pane** means the pane whose focus handle contains the focused
  element (`TabBar::focus_within`, set by kiln-shell's `PaneState::tab_bar`
  from the pane's own handle), not the workspace's active pane: with focus
  in the sidebar or a palette no tab is marked, as the audit's focus rules
  read ("unfocused panes' active tabs have none"). The pane view tracks that
  handle around its items in both kiln-shell's `ItemPane` and fe-workbench's
  `Pane`, so focus in the editor counts.
- **After the tabs** (fe-workbench `pane.rs` `tab_row`): a Markdown tab's
  preview buttons as before, then tab list ▾ (`Icon::ChevronDown`), split
  right, ⋯. Split down left the strip: it's the menu's second row. The tab
  list opens the toolbar picker on the pane's tabs (`ToolbarRequest::Tabs`),
  each with its icon and, for tabs sharing a title, its folder; the active
  one selected; a pick shows the tab, focused.
- **The ⋯ menu** (`toolbar_picker.rs` `PANE_COMMANDS`): the design's order,
  then the rest: Split Right, Split Down, Pin Tab (Unpin Tab while the
  active tab is pinned), Close Other Tabs, Reopen Closed Tab
  (`workbench.reopen_closed_editor`), Close Tab, Close Pane, Move Tab
  Left/Right/Up/Down, Toggle Breadcrumbs. "Move to new window" is left out:
  no command exists.
- **Keys in the menu** come from the active keymap, in the context of the
  element that had focus when ⋯ was clicked (`Pane::menu_context`, read
  from the window's dispatch path after the click focuses the item), so vim's
  normal-mode window keys show when vim is on. Chords show, unlike the
  tooltips' single keys. Of several bindings, the menu shows the one bound
  for the innermost context, then the one with the fewest modifiers held
  (`⌃W V` over `⌃W ⌃V`), then the keymap's own order
  (`pane_toolbar::menu_shortcut`). None is shown when nothing binds it.
- **Breadcrumbs** (kiln-ui `breadcrumbs.rs`): `Crumb::color` sets a label's
  colour; otherwise a directory crumb is `text.muted` and a file or symbol
  crumb `text.primary`. Separators stay Lucide's chevron-right (the audit's
  CH4 wants Lucide marks in the chrome, not text glyphs), now in
  `text.faint`; the crumb hover is `state.hover`. fe-workbench colours each
  symbol crumb as its name is in the code: the kind's highlight
  (`outline_panel::symbol_highlight`, made `pub(crate)` for it) through the
  editor theme, plain text where the theme gives that highlight none.
- **A symbol crumb's click** (rev 3, C6) opens Structure's quick outline,
  Go to Symbol in File (`pane_toolbar::QUICK_OUTLINE`,
  `nav.document_symbols`), dispatched from the mouse after the pane's editor
  is focused, so the outline lists that editor's symbols and focus goes back
  to it on Escape. The toolbar picker's sibling list went with it
  (`ToolbarRequest::Symbols`, its stale-outline refusal, and the workbench's
  outline observer's call into it): the quick outline checks a pick
  against its outline the same way (`symbol_picker.rs`). The tests that
  picked a sibling now pick in the quick outline, which selects the name,
  so the caret lands at its end. `symbol_picker.rs`'s module doc still says
  its picks are checked "as the toolbar picker's are"; that file is outside
  this card's scope, so the stale phrase stays.
- **i18n**: every user-visible string in `pane.rs`, `pane_toolbar.rs` and
  `toolbar_picker.rs` is in `l10n/en-US/pane.ftl` (`workbench-pane-…`), and
  the three files are in `l10n-allow.toml`'s migrated list. Kept text is
  byte-equal ("No open files", "Loading {name}…", "Close Tab", …); the menu
  rows the design names follow its words ("Reopen Closed Tab", "Pin Tab").
  The pane's error message is stored as a `Text` and resolved when drawn.
  The large-file viewer's banners are resolved when set: `FileViewer` is
  fe-gpui's and stores a `SharedString`.

## Edits outside the first scope (accepted at rev 3)

- `crates/fe-workbench/src/outline_panel.rs`: `symbol_highlight` made
  `pub(crate)`, so the crumbs and Structure share one kind → highlight
  table instead of a copy.
- `crates/fe-workbench/src/markdown_preview_buttons_tests.rs`: its
  `STANDARD` buttons are now tab list, split right, ⋯ (split down left the
  strip).
- `crates/fe-workbench/src/pane_pin_tests.rs`: a pinned tab no longer draws
  the pin glyph (`tab-pin:`), so `a_pinned_tab_lays_out_at_icon_width`
  checks it's absent.
- `kiln/crates/kiln-ui/src/tests/coloured_icons.rs`: the close mark is now
  an `Icon::X` svg, which the tests' "the icon is the one svg painted"
  checks would count, so their tab is dirty and shows its dot instead.
- `kiln/crates/kiln-ui/l10n-allow.toml`: the `tab-pin:{label}` selector's
  entry is now `tab-accent:{label}`'s (the lint refuses an entry that
  matches nothing).

## Goldens

The five fe-workbench goldens were regenerated with `UPDATE_SCREENSHOTS=1`
and looked at: the 34 px strip and dock header, the accent line on the
focused pane's active tab, the `Icon::X` close mark, tab list ▾, split right
and ⋯ after the tabs, the symbol crumb in the module colour. fe-gpui's
`tabs.png` didn't move when regenerated: its scene is Go source indented
with tab characters, with no tab strip in it.
