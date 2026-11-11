---
task_uid: "01m6vvtveks2c9j8c6zngt408f"
task: "T1543"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::status_bar_b3 (cargo nextest run -p fe-workbench) pass: 24 px; NORMAL is a pill in vim_mode_normal (Yohen: the accent), INSERT in celadon, VISUAL in jun; error and warning counts each carry their glyph; right side in the order language server, position, indent, encoding, line ending, language; no branch item"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C3"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (the five fe-workbench goldens)"
    required: true
    manual: false
---

# Status bar in B3: 24 px, the Vim mode pill in its mode colour, error and warning counts with glyphs, the right-side order, no branch

# Status bar in B3: 24 px, the Vim mode pill, counts with glyphs, no branch

Spec: `docs/design/design-system.md#workbench-anatomy` (Status bar row;
Measure 24 px) and `#signals` (NORMAL accent, INSERT celadon, VISUAL jun).
Audit finding CH9.

## Change

- 24 px on `surface.chrome`, UI label (12/16 500), one 8 px padding per item
  (`kiln/crates/kiln-ui/src/status_bar.rs`, `kiln/crates/kiln-shell/src/status.rs`).
- Left: the Vim mode as a pill in its mode colour (theme `vim_mode_*`; NORMAL
  is the accent) with `recording @a` after it; then error and warning counts,
  each with its glyph (✕ error, ▲ warning) so meaning never rests on hue.
  `crates/fe-workbench/src/status_bar.rs`, `crates/fe-workbench/src/vim_line.rs`,
  and the editor's items in `crates/fe-gpui/src/pane_item.rs` (`vim_mode_fill`).
- Right, in order: language server, position, indent, encoding, line ending,
  language. (The bell waits for the inbox.)
- The branch item goes: it is in the title bar (`crates/fe-workbench/src/git_branch.rs`
  status item; its tests follow).
- Tests that pin today's items follow (`crates/fe-workbench/src/tests/status_bar.rs`,
  `tests/status_bar_icons.rs`, `crates/fe-workbench/src/status_bar_vim_tests.rs`).
- Regenerate the five fe-workbench goldens on the Mac.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `status_bar.rs` and `vim_line.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/status.ftl` (keys `workbench-status-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (goldens).
