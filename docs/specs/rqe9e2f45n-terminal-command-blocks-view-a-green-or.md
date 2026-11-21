---
task_uid: "01m6w0ahkhqmfgygrqe9e2f45n"
task: "T1595"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::terminal_blocks (cargo nextest run -p fe-workbench) pass: the_edge_is_celadon_for_exit_0_and_shinsha_otherwise, a_running_or_unknown_block_has_no_edge, the_label_is_dropped_when_the_prompt_row_is_full, terminal.command_blocks_false_draws_neither, the_text_grid_does_not_move"
    required: true
    manual: false
  - id: "C2"
    text: "Test terminal_blocks_defaults_true (cargo nextest run -p fe-config --test terminal_blocks_setting) passes"
    required: true
    manual: false
  - id: "C3"
    text: "UPDATE_GOLDEN=1 regenerates the settings schema and the generated settings.ftl and settings-template.ftl; cargo nextest run -p fe-config passes with them"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C5"
    text: "On the Mac, the audit scene named in the body renders in both themes and its probes pass (recorded in the decision file) (audit/terminal_blocks.rs)"
    required: true
    manual: false
---

# Terminal command blocks, view: a green or red edge, exit code and duration on the prompt line

# Terminal command blocks, view: edge, exit code and duration

Spec: `docs/design/design-system.md#terminal`.

## Change

- `crates/fe-terminal-view/src/{element,layout,view}.rs`: for each visible
  `CommandBlock` (previous card) draw a 2 px left edge along its rows in
  celadon (exit 0) or shinsha (non-zero), none for a running block or an
  unknown code; the edge sits in the existing left padding, so the text grid
  does not move. On the block's prompt row, right-aligned over the
  background, draw the label the host supplies (`exit 1 · 2.3 s`; ✓ for a
  success longer than a second only), so a label never overlaps typed text
  (it is dropped when the row is too full).
- The host (`crates/fe-workbench/src/terminal_panel.rs`) supplies the labels
  through the catalogue (plural-free, with a duration formatter), so
  `fe-terminal-view` holds no UI text. Block colours come from the theme
  tokens added by T1509; an agent's terminal blocks are not coloured here.
- The edge and label can be turned off with `terminal.command_blocks`
  (default true) in `crates/fe-config/src/settings.rs`, schema and template.
- An audit scene `audit/terminal_blocks.rs` (a fake session with a passing
  and a failing block), both themes.

## Not in this card

Send to an agent (a later card), folding blocks, jumping between blocks (the existing
prompt jump stays).

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-terminal-…` in
`crates/fe-workbench/l10n/en-US/terminal.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**; the audit scene is recorded on the Mac.
