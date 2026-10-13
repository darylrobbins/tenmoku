---
task_uid: "01m4sj3rkx2avebpaaed9anexc"
task: "T70"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-terminal-view tests/nerd_font_fit.rs: U+F07B, 1.6 cells wide in the fake text system, followed by a non-blank cell is painted at a font size of at most font_size / 1.6 and a width of at most one cell, centred in its cell within 0.5 px"
    required: true
    manual: false
  - id: "C2"
    text: "nerd_font_fit.rs: the same glyph followed by a blank cell with the default background fits within two cells, centred in them; followed by a blank cell with another background, within one"
    required: true
    manual: false
  - id: "C3"
    text: "nerd_font_fit.rs: Powerline U+E0B0, 1.2 cells wide, fits one cell at its column's origin even before a blank cell; a private-use glyph no wider than a cell, ASCII letters and a wide CJK character paint at the style's font size at their cell's origin, and tests/view.rs passes unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "nerd_font_fit.rs: a focused block cursor on U+F07B draws its glyph fitted by the same rule; PaintedText reports each cluster's font size"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-terminal-view and cargo clippy -p fe-terminal-view --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Fit Nerd Font icons to the terminal grid: a private-use glyph wider than its cell is scaled into one cell, or two when the next is blank, and Powerline symbols into one (Mac T606)

Filed on argon from Mac T606 (card ~/coordination/argon-cards/T606.json rev 1). Mac task uid 01m4sj28seg6e2ydwx2mzx0zen. Its dependencies Mac T585 (argon T64, b048afc) and Mac T578 (argon T62, dbd1bfb) are on argon main, so they aren't recorded here. Title the host commit "... (Mac T606)".

## Problem

The user (2026-10-13): "We should support a custom font for the terminal so I can specify a nerd font."

T578 (argon dbd1bfb, in stack 6) already lets them name one: `terminal.font_family = "JetBrainsMono Nerd Font"`
restyles the running session on a settings reload (`TerminalPanel::set_settings` → `TerminalView::set_style`),
and `fe_terminal_view::cell_size` is re-measured in every prepaint, so the grid refits to the new font. Naming
the font works.

What doesn't: Nerd Font icons are private-use-area characters (U+E000–U+F8FF, U+F0000–U+FFFFD,
U+100000–U+10FFFD) that the terminal grid gives one cell (`unicode-width` treats them as narrow), but whose
glyphs are wider than a cell in every Nerd Font that isn't a "Mono" variant (about 1.5–2 cells), and in the
symbol fonts people pair with one. `crates/fe-terminal-view/src/element.rs` shapes each cell's cluster at its
natural width (`shape_cluster`) and paints it at the cell's origin, so a wide icon spills over the next cell
and overlaps its glyph (`ls` with icons, starship and lazygit prompts, nvim file trees in the terminal). Other
terminals constrain these glyphs: Ghostty scales a Nerd Font icon to one cell, or two when the next cell is
blank; kitty and WezTerm let it take the following blank cell.

## Change

In `crates/fe-terminal-view/src/element.rs`, where `prepaint` shapes the grid's clusters (and the block cursor's
glyph), a cluster whose first character is in a private-use range above is fitted to its span:

- **Span.** Two cells when the cell after it on the row is blank (a space or empty, with the default
  background: a coloured background means a prompt segment, which keeps one cell) and the cluster isn't in the
  Powerline range U+E0A0–U+E0D7; else one cell. Powerline separators and arrows always take exactly one cell,
  left-aligned, so prompt segments still abut.
- **Fit.** If the shaped width is wider than the span, shape it again at
  `font_size * span_width / natural_width` (no lower than half the font size) and centre it horizontally in
  the span, its baseline unchanged. A glyph that already fits is untouched, and centred only in a two-cell
  span.
- Every other character keeps today's path: natural width at the cell's origin.

Put the span and scale rules in a small pure function (`fn fit_symbol(ch, natural, cell, next_blank) ->
(span, scale)` or similar) next to `shape_cluster`, so they are unit-testable without a window.

`PaintReport`'s `PaintedText` gains the font size it was shaped at (`font_size: Pixels`), filled for every
cluster, so tests can see a fitted glyph.

New tests in `crates/fe-terminal-view/tests/nerd_font_fit.rs`, using a fake `PlatformTextSystem` like
`tests/view.rs`'s `Shaping` (copied into the new file, not shared) that gives private-use characters an
advance of 1.6 cells and Powerline U+E0B0 one of 1.2 cells.

This needs Mac stack 6: `element.rs` was changed by T585 (argon b048afc), and the settings it serves are T578's.
