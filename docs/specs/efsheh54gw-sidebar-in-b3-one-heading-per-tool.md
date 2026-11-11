---
task_uid: "01m6vvtqa1g7s0teefsheh54gw"
task: "T1541"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::sidebar_b3 (cargo nextest run -p fe-workbench) pass: one header above the first tree row (no view header for a lone view), ending in the view's actions, then more (an accessible name), then hide; the root row in the folder's own case; rows 24 px; a modified file's letter in vcs.modified, never the accent; a folder's dot 6 px in the letter column"
    required: true
    manual: false
  - id: "C2"
    text: "tests::sidebar_b3::structure_shows_the_caret_symbol_when_collapsed passes; Outline is named Structure"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (the five fe-workbench goldens)"
    required: true
    manual: false
---

# Sidebar in B3: one heading per tool window, pane controls in the header, 24 px project rows in VCS colours, Structure collapsed at the foot

# Sidebar in B3: one heading per tool window, 24 px tree rows, Structure at the foot

Spec: `docs/design/design-system.md#workbench-anatomy` (Sidebar: project tree
with Structure collapsed at its foot; Structure, collapsed, still shows the
symbol under the caret; pane controls). Audit findings CH1, CH2, PT2, PT3 (the
held card P13).

## Change

- One heading per tool window: the dock's header (`kiln/crates/kiln-shell/src/dock.rs`)
  names the view in UI heading (13/18 600, ink) and ends with the pane controls
  rule: the view's own actions, then ⋯, then — (hide). A lone view loses its
  own header (`view_stack.rs`, `view_container.rs`). Sentence case.
- Project tree (`crates/fe-workbench/src/project_panel.rs` and its
  `_filter`, `_nav`, `_ops`, `_clipboard` helpers): the root row is the
  folder's name in its own case (no capitals); 24 px rows in UI body; the
  focused selection is the controls card's focused-selection row; Git status
  letters and folder dots in the `vcs.*` tokens (folder dot 6 px, centred in
  the letter column, PT2); an ignored row keeps its chevron and icon at full
  strength (PT3). `kiln/crates/kiln-ui/src/tree_row.rs`.
- The Outline view is renamed Structure (`crates/fe-workbench/src/dock.rs`,
  `outline_panel.rs`), sits collapsed at the foot of Project, and its collapsed
  header shows the symbol under the caret (faint, syntax-coloured).
- Search's toolbar into the header is the search card's.
- Regenerate the five fe-workbench goldens on the Mac.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `project_panel*.rs`, `outline_panel.rs` and `outline_panel_keys.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/project.ftl` (keys `workbench-project-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (goldens).

## Implementation notes

The design calls, the text moved into `project.ftl`, and the files touched
beyond the scope list (with why) are in `docs/decisions/efsheh54gw-sidebar-b3.md`.
