---
task_uid: "01m6vvtsc5zpqphm9w6rcn1atf"
task: "T1542"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::pane_controls_b3 (cargo nextest run -p fe-workbench) pass: tabs 34 px; only the focused pane's active tab has the 2 px accent top line; a dirty tab shows the dot; a pinned tab is its icon only; after the tabs come tab list, split right, more, in that order"
    required: true
    manual: false
  - id: "C2"
    text: "tests::pane_controls_b3::pane_menu_lists_the_design_actions_with_their_keys passes: split right, split down, pin, close other tabs, reopen closed tab, in that order, each with the key the active keymap binds (none shown when unbound)"
    required: true
    manual: false
  - id: "C3"
    text: "Symbol crumbs take their kind's syntax colour (tests::pane_controls_b3::symbol_crumbs_are_syntax_coloured)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C5"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (the five fe-workbench goldens; fe-gpui tabs.png with cargo run -p fe-gpui --example screenshots)"
    required: true
    manual: false
  - id: "C6"
    text: "tests::pane_controls_b3::clicking_a_symbol_crumb_opens_the_quick_outline passes: a click on a symbol crumb opens Structure's quick outline (nav.document_symbols), not the toolbar picker's sibling list; tests::breadcrumb_symbol_icons, tests::toolbar_history and tests::toolbar_history::goto are updated to that behaviour and pass"
    required: true
    manual: false
---

# Tab strip, breadcrumbs and the pane menu in B3: 34 px tabs with the accent top line in the focused pane, tab list, split, the design's pane actions with keys, symbol crumbs in syntax colours

# Tab strip, breadcrumbs and the pane ⋯ menu in B3

Spec: `docs/design/design-system.md#workbench-anatomy` (Editor tab strip,
Breadcrumbs, Pane controls; Measure: tab strip 34 px). Audit findings CH3, CH4,
TH5 (the held card P12).

## Change

- Tabs (`kiln/crates/kiln-ui/src/tab_bar.rs`, `kiln/crates/kiln-shell/src/pane.rs`):
  34 px on `surface.chrome`; a 2 px accent top line on the active tab of the
  focused pane only; a hover fill; the unsaved dot; a pinned tab as its icon
  only; the close mark `Icon::X` at 12 px in a 16 px box.
- After the tabs (`crates/fe-workbench/src/pane.rs` `tab_row`): tab list ▾
  (opens a list of the pane's tabs in the toolbar picker), split right, ⋯.
  The ⋯ list (`crates/fe-workbench/src/toolbar_picker.rs` `PANE_COMMANDS`)
  in the design's order with each key shown: split right ⌃w v, split down
  ⌃w s, pin tab, close other
  tabs ␣b o, reopen closed tab ␣b u (`workbench.reopen_closed_editor`), then
  today's extra items. Leave out "move to new window" if no command exists
  (it's a separate capability). Maximize (⌃w z) and move to the other split (⌃w x) are left out too: no command exists for either yet; a follow-up card adds pane.toggle_maximize and pane.exchange to this list after Split Down. Keys come from the keymap, not literals.
- Breadcrumbs (`kiln/crates/kiln-ui/src/breadcrumbs.rs`,
  `crates/fe-workbench/src/pane_toolbar.rs`): symbol crumbs in their syntax
  colours (the kind's highlight: function, type, …), path crumbs in muted,
  `›` separators in faint; clicking a symbol crumb opens Structure's quick
  outline (`␣o`, `nav.document_symbols`) instead of the toolbar picker's sibling list; update `workbench.rs` (the outline observer's `toolbar_picker` outline_changed call) and the tests that click symbol crumbs (`tests/breadcrumb_symbol_icons.rs`, `tests/toolbar_history.rs`, `tests/toolbar_history/goto.rs`) to match.
- Regenerate the five fe-workbench goldens and fe-gpui's `tabs.png` on the Mac.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `pane.rs`, `toolbar_picker.rs` and `pane_toolbar.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/pane.ftl` (keys `workbench-pane-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (goldens).

## Respec (lead, 2026-11-11)

Worker block resolved: C2 trimmed to the five actions that exist (maximize and exchange move to a follow-up card); the crumb click goes to the quick outline (option a), with its files added to the scope and a new C6; the small out-of-scope edits already made (outline_panel.rs symbol_highlight pub(crate), markdown_preview_buttons_tests.rs, pane_pin_tests.rs, kiln-ui tests/coloured_icons.rs, kiln-ui l10n-allow.toml) are accepted and in scope.
