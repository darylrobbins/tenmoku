---
task_uid: "01m6vvvcaz8h9s8chfv9854yzs"
task: "T1550"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in annotations_b3 (cargo nextest run -p fe-gpui --test annotations_b3) pass: an inlay hint is a pill on surface.raised in faint text; a warning lens is warning at 70% alpha, an error lens error at 100%; document highlights use identifier_highlight; the current find match find_match and others find_match_other"
    required: true
    manual: false
  - id: "C2"
    text: "The test lens reads '▷ Run test' in UI small sans (cargo nextest run -p fe-workbench test_lens)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist); cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (fe-gpui goldens that change, inline_diagnostics_overhang.png at least)"
    required: true
    manual: false
---

# Language-server annotations in B3: inlay hint pills, inline lens strength, identifier highlight and find-match colours, code lenses in sans

# Language-server annotations in B3: hint pills, lens strength, highlights, code lenses

Spec: `docs/design/design-system.md#editor` (Language-server formatting; Find
and replace's match colours).

## Change

- Inlay type hints as small pills (UI small on `surface.raised`, faint text,
  radius 4): `crates/fe-gpui/src/inlay_hints.rs`.
- Inline diagnostics lens at the end of the line: kiseto at 70% for warnings,
  shinsha at full for errors (`crates/fe-gpui/src/inline_diagnostics.rs`).
- Identifier highlight (document highlights and occurrences): the
  `identifier_highlight` (kiseto-soft) fill (`crates/fe-gpui/src/element/decorations.rs`
  `tint_color`). Find: the current match `find_match`, others
  `find_match_other` (`element.rs` `search_match_color`).
- Code lenses in UI small sans, faint, `·` separated
  (`crates/fe-gpui/src/lens.rs`); the test lens reads `▷ Run test`
  (`crates/fe-workbench/src/test_lens.rs`); references lens unchanged in
  wording (`lsp_lenses.rs`). "Debug" and "passed in the last 8 runs" wait for
  the debugger and test history.
- Mutable underline and wavy squiggles exist: check they use the token's
  colour, thin and solid for mutable.
- Regenerate `inline_diagnostics_overhang.png` and any other fe-gpui golden
  that changes, on the Mac.

## i18n

fe-gpui's text is already in its catalogue (card G1). Any text this card adds
or rewrites goes to `crates/fe-gpui/l10n/en-US/` as keys; the crate's lint
fails otherwise.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `test_lens.rs` and `lsp_lenses.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/lens.ftl` (keys `workbench-lens-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (goldens).
