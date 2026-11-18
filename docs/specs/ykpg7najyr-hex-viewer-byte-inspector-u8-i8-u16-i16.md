---
task_uid: "01m7j1ptwees5pabykpg7najyr"
task: "T1896"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "inspect(bytes, Endian) in hex_viewer/inspector.rs gives u8, i8, u16, i16, u32, i32, u64, i64, f32 and f64 from the first bytes in either order, None where too few bytes remain, floats as Rust {} prints them with NaN/inf/-inf/-0: hex_viewer::inspector_tests::reads_each_type_little_and_big_endian, ::a_short_tail_leaves_wide_types_empty, ::floats_show_nan_infinity_and_negative_zero (cargo nextest run -p fe-gpui)"
    required: true
    manual: false
  - id: "C2"
    text: "A strip at the bottom of the hex view shows those values for the selection start (or caret), \"—\" for a missing value or an empty file, with an LE/BE toggle button; it updates when the selection moves and reads the bytes once per selection change, not per frame; it wraps to a second row when narrow: hex_viewer::inspector_tests::strip_follows_the_selection_start, ::endian_toggle_flips_the_values, ::the_strip_reads_once_per_selection_change. Known limit (user-approved findings_override of review F1, 2026-11-17): a long value's tooltip shows the whole value wrapped at 40 characters, but is not yet constrained to the window, so a narrow or short window can clip it; viewport-constrained tooltips and rendered hover tests are follow-up T1936, not this task"
    required: true
    manual: false
  - id: "C3"
    text: "hex.toggle_inspector and hex.toggle_endian in context HexViewer, titled in hex.ftl and listed in the palette; the strip is shown by default and hiding it gives its height to byte rows; shown and byte order survive serialize/restore, and a state saved without them restores as shown, little-endian: hex_viewer::inspector_tests::hiding_the_strip_gives_rows_back, ::inspector_state_survives_a_saved_layout, ::an_older_saved_state_restores_the_defaults"
    required: true
    manual: false
  - id: "C4"
    text: "The status bar items, the minimap-off rule and every hex viewer test from the hex card still pass (cargo nextest run -p fe-gpui hex_viewer; cargo nextest run -p fe-workbench tests::hex_viewer); debug_selector ids use verbatim!, the l10n lint and clippy pass"
    required: true
    manual: false
---

# Hex viewer byte inspector: u8/i8/u16/i16/u32/i32/u64/i64/f32/f64 at the selection, little or big endian, in a strip under the rows

## Ask

The user, verbatim: "Should we offer a hex editor view for binary files?" The lead proposed a read-only viewer first, with a byte inspector for the selection; the user: "Let's card it. Let's disable the minimap for this mode."

This card is the byte inspector, on top of the read-only `HexViewer` from the card it depends on (`crates/fe-gpui/src/hex_viewer.rs`).

## What it is

A small strip at the bottom of the hex view, inside the view and not in the workbench status bar. It goes in the view because 10 types times two byte orders don't fit in the status bar; the status bar keeps the hex card's Read-only and offset items unchanged.

- **What the strip shows.** The values of the bytes starting at the selection's start, or at the caret when the selection is empty:
  - `u8 i8 u16 i16 u32 i32 u64 i64 f32 f64`
  - in one byte order at a time
  - with a `LE` / `BE` toggle button at the strip's end
- **Font and colours.** The strip uses the theme's mono font. Use no new theme tokens: a new token group breaks `design_role_tokens`.
- **Width.** Below about 600 px it wraps onto a second row.
- **Short tail.** A type that needs more bytes than are left before end of file shows `—`. An empty file shows `—` for every type.
- **Floats.** f32 and f64 print as Rust's `{}` does (shortest round-trip), with `NaN`, `inf`, `-inf` and `-0` as such.
- **Integers.** Integers are decimal.

Put the value logic in a headless `crates/fe-gpui/src/hex_viewer/inspector.rs`: `inspect(bytes: &[u8], endian: Endian) -> Inspection`, one `Option` per type. The strip only formats it. Read the 8 bytes through the same `FileView::bytes` the rows use. Never do this read on every frame: read once per selection change.

Commands go in the hex view's `Registry` with context `HexViewer`, and their titles in `crates/fe-gpui/l10n/en-US/hex.ftl`:

- `hex.toggle_inspector`: shows or hides the strip. It is shown by default, and hiding it gives its height back to byte rows.
- `hex.toggle_endian`: does what the button does.

The strip's shown state and byte order are saved in the hex tab's layout state (`serialize` / `restore` in `hex_viewer/pane_item.rs`). A saved state from before this card has neither, and restores as shown, little-endian.

## Out of scope

- Editing values.
- Copying a value.
- Signed/unsigned display options.
- Other types (LEB128, dates, GUIDs).

## Linux-ok

This card needs no golden and no audit change. The hex card's `hex_viewer` audit surface keeps its probes out of the strip's area, so this card doesn't touch the audit; a later Mac audit run shows the strip. Everything here is headless: the gpui test platform plus pure functions.

## Notes for the worker

- Write debug_selector ids with `verbatim!`, or the R3 l10n lint flags them. The l10n lint must pass.
- The hex card's tests must still pass: `cargo nextest run -p fe-gpui hex_viewer`.
