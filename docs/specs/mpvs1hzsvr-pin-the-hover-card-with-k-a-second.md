---
task_uid: "01m7c8nf13649t15mpvs1hzsvr"
task: "T381"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-gpui --test hover_pin pass: a_second_show_hover_pins_the_card, a_pinned_card_survives_pointer_leave_and_edits_elsewhere, escape_closes_it, k_again_closes_it, the_wheel_scrolls_and_text_selects_in_a_pinned_card"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Pin the hover card with K: a second editor.show_hover keeps it until Escape (Mac T1603)

# Pin the hover card with K

Spec: `docs/design/design-system.md#editor` (footer "gd definition · K pin").
K is `editor.show_hover` in the vim context
(`crates/fe-keymap/keymaps/fastedit.toml`); `hover.rs` hides a shown popover
when the pointer leaves, the buffer changes, or Esc is pressed.

## Change

- Pressing `editor.show_hover` while the popover it opened is shown **pins** it:
  it stays until Esc, a click elsewhere, or the caret leaves its symbol's
  line; pointer-leave, scrolling and edits elsewhere no longer hide it; its
  content scrolls with the wheel and its text can be selected and copied. A
  pinned card shows a pin glyph in its footer instead of "K pin".
- A second K (or `Esc`) while pinned closes it. Pinning a hover opened by the
  pointer pins it too.
- `crates/fe-gpui/src/hover.rs`, `crates/fe-gpui/src/commands.rs`. No new
  command id, no keymap change (the footer hint already says K).

## Not in this card

Multiple pinned cards, moving a pinned card, the caller count.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys with fe-gpui's prefix in
`crates/fe-gpui/l10n/en-US/hover.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**.

## Implementation notes

- Pin state is `Current::pin` in `hover.rs`; `hide_popover` leaves a pinned card, `close_popover` forces. A pinned card stands while the caret is on its pin line (an anchor, so edits elsewhere move it). A click elsewhere closes it only by moving the caret off the line (view.rs is out of scope).
- Selecting: a press on a card part selects it (highlight, primary selection); the secondary-C copies it.
