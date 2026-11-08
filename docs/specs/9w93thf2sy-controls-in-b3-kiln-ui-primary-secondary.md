---
task_uid: "01m6vvs4sbkhkdhy9w93thf2sy"
task: "T1518"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::b3_controls (cargo nextest run -p kiln-ui) pass: button heights 26 and 22, radius 4; primary fill accent.base; a disabled control at 50% opacity; a focused input draws a 2 px accent ring offset 1 px; a toggle's on-state fill is the accent at alpha 0x26; a badge sits in the bottom-right with a 2 px ring in the surface colour"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::b3_controls::icon_only_buttons_need_a_name passes (an unnamed icon-only button trips the debug assertion; every kiln-ui caller names theirs)"
    required: true
    manual: false
  - id: "C3"
    text: "Focused vs unfocused selection: tests::b3_controls::selected_row_with_focus_has_the_accent_edge passes"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p kiln --example gallery-screenshots and cargo run -p fe-workbench --example screenshots match their goldens (gallery_*.png and setup_wizard_*.png regenerated in this change)"
    required: true
    manual: false
---

# Controls in B3 (kiln-ui): primary, secondary and ghost buttons, inputs with the 2 px focus ring, toggles, switch, checkbox, segmented control, badge; accessible names required

# Controls in B3 (kiln-ui), with the 2 px focus ring and accessible names

Spec: `docs/design/design-system.md#surfaces` (control border, hover, selected,
focused selection), `#type-spacing-and-sizes` (controls 26 / 22 px, radius 4),
`#accessibility-rules` (real buttons, inputs, tabs, trees and switches with
labels; icon-only buttons have names; the focus ring is 2 px rust, offset 1
px). Audit findings FB1 (borderless fields), GP3 (disabled buttons).

The parts every later panel card uses, so each panel doesn't invent its own.

## Change (kiln-ui)

- `Button`: primary (accent fill, `text.on_accent`), secondary
  (`border.control`), ghost (hover fill only); 26 px, small 22 px; radius 4;
  disabled at 50% of the enabled look. `kiln/crates/kiln-ui/src/controls.rs`.
- Text inputs: `border.control`, and the 2 px accent focus ring offset 1 px
  when focused (`text_field.rs`, `text_input.rs`).
- Toggle button (on = accent at alpha 0x26, not a solid fill), Switch,
  Checkbox, and a new `SegmentedControl` (`segmented.rs`, for scope switches and
  You / This project) and `Badge` (count or dot, colour by meaning, with a
  2 px cut-out ring; `badge.rs`).
- Every control takes an accessible name; an icon-only button without one is a
  debug assertion, and each gets a tooltip naming its key where it has one.
- Row selection: selected without focus = `state.selected_inactive`; with focus
  = `state.selected` tint plus a 2 px `accent.base` edge (`tree_row.rs`,
  `list.rs` paint).
- The kiln gallery shows each control in each state
  (`kiln/crates/kiln/examples/gallery-screenshots/main.rs`); regenerate the
  gallery goldens and the setup wizard goldens (its buttons) on the Mac.

i18n: kiln-ui has one user-facing literal today; new controls take labels from
their caller (no new literal). `Button::new`'s parameter type is the i18n
label-constructor card's change, not this one's.

Platform: **Mac-only** (goldens).
