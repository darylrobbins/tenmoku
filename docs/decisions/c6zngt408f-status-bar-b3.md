# Status bar in B3 (T1543)

Spec: `docs/specs/c6zngt408f-status-bar-in-b3-24-px-the-vim-mode-pill.md`;
design: `docs/design/design-system.md#workbench-anatomy` (Status bar row,
Measure 24 px) and `#signals`; audit CH9.

## What changed

- **The bar** (kiln-ui `status_bar.rs` `StatusBarFrame`): `height::STATUS_BAR`
  (24 px) on `surface.chrome`, its text the `UI_LABEL` role (12/16, 500).
- **Entries** (`StatusEntry`): one 8 px padding either side, filled or not
  (the audit's CH9: it was 8 px plain, 12 px filled). A filled entry draws its
  icon and label in a pill inside that padding: 16 px tall (a 4 px inset in the
  24 px bar), 6 px inside either end, `radius::CONTROL` corners, semibold.
  Test selector `status-pill:<id>`. A fill still keeps its colour on hover.
- **The vim mode** is that pill in the editor theme's `vim_mode_*` colour, as
  before (`pane_item::status::vim_mode_fill`). Under Yohen, NORMAL is hare's
  fur (`accent.base`), INSERT celadon, VISUAL jun, and the theme already said so.
  `recording @a` follows at priority 5.
- **Vim's command line** (`vim_line.rs`) takes the bar's place with the same
  metrics. Its `COMMAND` segment is a pill with the same measures (16 px,
  6 px inside, 4 px corners, semibold), 8 px from the left edge and 8 px
  before the prefix. The `vim-line-mode` selector covers the lead-in and the
  pill, so `mode_segment_tests` (flush left, 8 px gap) pass unchanged.
- **Error and warning counts** already had their glyphs (`Icon::CircleX` ✕,
  `Icon::TriangleAlert` ▲, from `problems_panel.rs`) at priorities 60/61, after
  the mode and recording. Nothing there changed. The B3 test pins the glyphs.
- **The right side** was already in the design's order: the language server
  (priority 0), then position, indent, encoding, line ending and language
  (100–500). The test pins it. The bell waits for the inbox.
- **No branch item.** `git_branch.rs` no longer sets a status item. The title
  bar's branch widget shows the branch.

## Decisions

- **The change marks move to the title bar.** The status item showed VS Code's
  `*` (working-tree or untracked changes), `+` (staged) and `!` (merge or
  rebase in progress) after the branch. Dropping them would have lost a
  tested feature. It would also have left `git::Marks` unread: dead code
  outside this card's scope (`git.rs`). So `set_branch_item` now puts the
  marked label (`main*+`) in `Workbench::title_branch`, which the title bar's
  widget draws. It notifies only when the label changes. The marks tests
  (`git_branch_marks_tests.rs`, `git_branch_merge_marks_tests.rs`) are
  unchanged: their helper `branch_item` (in `git_branch_tests.rs`) now reads
  `title_bar_state().branch`. The old click test now clicks `title-branch`.
- **Text moved to the catalogue** (`l10n/en-US/status.ftl`,
  `workbench-status-…`): `recording @{ $register }`, the encoding item's
  tooltip `Select Encoding` and the line's `COMMAND`, all byte-equal. The
  line's cancel command registers with its `command-vim_line-cancel` title.
  `status_bar.rs` and `vim_line.rs` are on `l10n-allow.toml`'s migrated list.

## Out of the spec's file list

- `crates/fe-workbench/src/tests/mod.rs`: the `mod status_bar_b3;` line
  (append-only shared file).
- `crates/fe-workbench/src/l10n.rs` and `l10n-allow.toml`, and kiln-ui's
  `l10n-allow.toml` (the `status-pill:{id}` selector, like its siblings):
  append-only lines.
- `kiln/crates/kiln-shell/tests/status_style.rs`: `icon_and_tooltip_draw`
  pinned a filled entry's icon 12 px in (the old segment padding). It is now
  14 px: the entry's 8 px padding plus the pill's 6 px.
- `kiln/crates/kiln/examples/gallery-screenshots/golden/*.png` (six): the
  gallery draws kiln-ui's `StatusBarFrame`, so its bar's text moved to the UI
  label size and these goldens moved with it. They were regenerated with
  `UPDATE_SCREENSHOTS=1` and checked: only the bottom bar's text differs.
  Nothing else in them moved.
- `crates/fe-workbench/examples/screenshots/audit/chrome.rs`: the status bar
  probe now expects `surface.chrome`. Other probes were already stale before
  this card and are left as they were: the tab strip's (600, 45), stale
  since T1542, and settings_editor, shortcuts_editor and git_panel.
- Doc comments only: `kiln/crates/kiln-shell/src/status.rs` (in scope) and
  `crates/fe-gpui/src/pane_item.rs` (in scope).
