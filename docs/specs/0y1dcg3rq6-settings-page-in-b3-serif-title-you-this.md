---
task_uid: "01m73wnepcmc5nrf0y1dcg3rq6"
task: "T569"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::settings_b3 (cargo nextest run -p fe-workbench) pass: the title is Instrument Serif 30 px; the scope switch reads You / This project; searching editor.font finds that row; a section with a changed setting has its index mark; the first section shows Yohen, Hakuji and Match system cards, then rail position, editor font and size, Vim mode"
    required: true
    manual: false
  - id: "C2"
    text: "Every row shows its label, its settings.toml key, one help line from the catalogue and one 26 px control; a changed row has the accent dot and Reset, which restores the default"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Settings page in B3: serif title, You / This project, key search, changed-section marks, theme cards first, label + key + help + control rows (Mac T1555)

# Settings page in B3

Spec: `docs/design/design-system.md#settings`. Audit findings SE1–SE4 (the
held cards P11's settings part and P17).

The page exists (`settings_editor*.rs`): search, a User/Workspace switch,
category navigation with counts, per-row Reset, a 2 px modified bar.

## Change (`crates/fe-workbench/src/settings_editor.rs`, `_layout.rs`, `_controls.rs`, `_choices.rs`)

- A serif page title "Settings" (Page title, 30/36); section headings 15/20
  600; the scope switch as a segmented control reading You / This project.
- One search that also takes keys: `editor.font` finds the row by its key.
- The section index on the left marks sections with changed settings (an
  accent dot).
- Common first: theme cards drawn in each theme's real colours (Yohen, Hakuji,
  Match system; reuse `setup_wizard_preview.rs`'s swatches without changing
  them), then rail position, editor font and size, Vim mode. Rows for settings
  that don't exist yet (colour-blind mode, agent command permissions,
  notification level) are not added.
- Every row: label (UI body), its key in settings.toml (code small, faint),
  one line of help (UI small, muted, from the catalogue by key, card C1), one
  control; an accent dot and Reset when changed from default. Controls are the
  controls card's, 26 px (SE3).
- Choices apply live (exists): changing rail position moves the rail at once.
- An audit scene `audit/settings.rs`, both themes.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `settings_editor.rs`, `settings_editor_layout.rs`, `settings_editor_controls.rs` and `settings_editor_choices.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/settings.ftl` (keys `workbench-settings-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Implementation notes

- **Common section.** With nothing searched for, `rows()` puts a Common
  heading first (its theme cards drawn under it) and moves `workbench.rail`,
  `editor.font_family`, `editor.font_size` and `vim.enabled` there from their
  own groups, so no setting shows twice (two rows would share one input
  entity). While searching there is no Common section and the settings show in
  their groups. The cards set `theme.mode`: Hakuji `light`, Match system
  `system`; Yohen, the default `dark`, resets the key. They use
  `setup_wizard_preview`'s `Swatches` and `TileArt` unchanged.
- **Theme cards stand for Tenmoku's own themes (review F1).** A card is the
  one shown only when `theme.mode` is its mode and, for each appearance it
  covers, nothing shows another theme: `theme.dark` / `theme.light` set to
  another theme, or, unless the slot names Yohen / Hakuji, a `theme.name` of
  that appearance. Picking a card removes those overrides as well as setting
  the mode, so what's chosen is what it draws; with an override in place no
  card is outlined. A slot is compared as the app reads it (fastedit's
  `theme::choice`): only the exact installed IDs `builtin/yohen` and
  `builtin/hakuji` keep Tenmoku's own; any other value, a name such as
  `yohen` included, is an override (review round 2, F1).
- **Wrapped cards (review round 2, F5).** Each card records where it was
  drawn; entering the cards row and every card key scroll the list as little
  as it takes for the ringed card and its ring to show, or, if the cards
  weren't drawn, to the list's top and then to the card on the next frame.
- **Keyboard (review F3).** With nothing searched for, the Common heading's
  row (the cards) is the first row Up and Down select. While it's selected in
  the focused list, the card shown (else Yohen) has kiln-ui's focus ring, and
  Left / Right / Home / End pick the card before / after / first / last, as a
  radio group's keys do; Enter and Space pick the shown one. Off under This
  project, where the theme can't be set.
- **Labels (review F2).** Every row's label is a catalogue message,
  `workbench-settings-label-<key, dashes for dots>` (111 of them, a dynamic
  prefix in `resources!`), its English the name the key makes as before but
  the Common four, which the design names (Rail position, Editor font, Font
  size, Vim mode); a setting newer than the catalogue falls back to that name.
  A test checks every setting has its message.
- **Reveal (review F4).** Going up to a group's first setting brings the
  heading with it only if both fit at the heights drawn; an unmeasured Common
  heading counts as its tallest, the cards one to a line.
- **Section index.** `Section` gained `entries`; a section is marked with an
  accent dot (`settings-nav-mark-<title>`) when the selected scope's file sets
  one of them, `theme.mode` counting for Common. After Common the sections run
  Editor, Theme, Workbench, Vim, Terminal, LSP, Tree, Status Bar, Chat,
  Extensions, Update, then the rest (audit SE1); index labels are ink, counts
  muted (SE2); rows are 28 px.
- **Rows.** Label (UI body), key (code small, faint, always shown), one help
  line (UI small, muted, cut short), and the control slot at 26 px. The help is
  the setting's catalogue description (fe-config's `settings-<key>`). Two
  settings have no doc comment and so no message, `lsp.timeouts.hover` and
  `lsp.timeouts.completion`; they show their table's (`settings-lsp-timeouts`)
  rather than nothing, as fe-config is outside this card. The 2 px modified bar
  became a 6 px accent dot before the label, and Reset is a ghost button
  after the key. Row padding is 12 (SE3).
- **26 px controls.** kiln-ui's `TextInput` was fixed at 30 px; it gained an
  opt-in `height()` builder (default unchanged), and the rows' inputs use
  `height::CONTROL`. Dropdown buttons, Add Item and Edit in settings.toml (now
  a `Button`) are 26 px; a switch sits centred in the 26 px slot.
- **Scope switch.** kiln-ui's `SegmentedControl` reading You / This project
  (This project only with a folder open). The Open button reads "Open
  settings.toml"; its tooltip names the file, so `open_file_target` returns the
  path only.
- **i18n.** All four files are in `l10n-allow.toml`'s migrated list, their text
  in `l10n/en-US/settings.ftl` (`workbench-settings-…`). Element ids and debug
  selectors go through a `selector(name, key)` helper; three Button ids are
  allow-listed. Kept text is byte-equal (Default (built-in), not installed,
  Other…, Add Item, Reset to Default as Reset's accessible name, …). The
  restricted note follows the new scope name.
- **Audit scene.** `audit/settings.rs` (surface `settings`, both appearances)
  replaces `audit/settings_editor.rs`, whose probes pointed at the old layout.
  It sets `workbench.rail = "left"`, the default, so the first Common row shows
  its dot and Reset without moving the rail. Its probe points were measured
  from the test platform's layout at the scene size (1000×600, title bar on);
  the scenes render only on macOS, so they are unverified on Linux.
- **Tests changed.** The existing settings tests that asserted the old page
  were updated in place: the scope tabs' selectors (now `segment:You` /
  `segment:This project`), the Open button's label, three-line descriptions
  (now one), the 18 px control height, rows and section counts with Common
  first, and the controls' English texts as local consts.
