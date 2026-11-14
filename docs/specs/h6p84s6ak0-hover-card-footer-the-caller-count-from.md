---
task_uid: "01m7bkyjpj5x791zh6p84s6ak0"
task: "T626"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::caller_count (cargo nextest run -p fe-lsp) pass: counts_incoming_call_items, caps_at_99, a_server_without_call_hierarchy_gives_none"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in tests::hover_callers (cargo nextest run -p fe-workbench) pass: the_footer_fills_late_without_moving_the_card, no_count_without_a_provider, the_request_is_cancelled_when_the_pointer_leaves, one_caller_reads_singular"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Hover card footer: the caller count from the call hierarchy, filled in when it arrives (Mac T1602)

# Hover footer: caller count

Spec: `docs/design/design-system.md#editor` ("Hover card ... a footer:
definition location, caller count, gd definition · K pin"). The hover card
card (T1544) builds the footer with the definition location and defers the
caller count.

## Change

- `crates/fe-lsp/src/hierarchy.rs` already has `incoming_calls`; add a cheap
  `caller_count(item)` helper that prepares the call hierarchy at the hovered
  symbol and counts incoming calls' items (capped at 99, shown "99+").
- `crates/fe-workbench/src/lsp.rs` (the hover provider) asks for it after the
  hover text is shown, off the critical path, and fills the footer when it
  arrives; cancelled when the pointer leaves; a server without call
  hierarchy shows no count. The footer slot comes from T1544's hover footer
  API in `crates/fe-gpui/src/hover.rs`; extend it only as far as a late-filled
  segment needs.
- Text: "N callers" (plural), `1 caller`.

## Not in this card

Click-through to the caller list (the existing call hierarchy panel covers it),
the K pin, counts for references.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys with fe-gpui's prefix in
`crates/fe-gpui/l10n/en-US/hover.ftl` (fe-gpui's catalogue: `crates/fe-gpui/src/l10n.rs`); a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**.
