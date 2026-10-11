---
task_uid: "01m4p1wt6x0kjwqaanz6pdtxbc"
task: "T2"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-terminal-view test layout::colours: Foreground/Background/Named/Indexed/Rgb resolve as the spec says; inverse swaps fg and bg, dim is fg at 70% alpha, hidden emits no glyph"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal-view test layout::runs_and_backgrounds: equal-style cells merge into one run starting at its column; non-default backgrounds merge into ranges; a wide char spans two columns and its spacer emits nothing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal-view test layout::cursor: focused gives a filled block, unfocused a hollow block, and Beam, Underline and Hidden map to the matching CursorLayout"
    required: true
    manual: false
  - id: "C4"
    text: "fe-terminal-view test view::sizes_the_pty: a view in an 800x400 window resizes the session to floor(800/cell width) x floor(400/cell height) (checked through stty size) and resizes it again when the window resizes; the shell exiting emits TerminalViewEvent::Exited"
    required: true
    manual: false
---

# Paint a terminal's grid in a GPUI TerminalView with the editor's font and theme colours, and keep the pty sized to the view (Mac T379)

Filed on argon from Mac T379 (card ~/coordination/argon-cards/T379.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-a.md). Mac task uid 01m4p1gj1skrxtbjbrjghy2rkv.

## Problem

The user asked on 2026-10-11 for "a terminal pane that we can run in the app". The core card adds
`crates/fe-terminal` (a pty session with a grid `Snapshot`), but nothing draws it. This card is the static
half of the UI: a GPUI view that paints a terminal's grid and keeps the pty sized to it. Keyboard input,
mouse input and the workbench panel are separate cards that depend on this one.

## Change

**New crate `crates/fe-terminal-view`** (GPUI; depends on `gpui`, `fe-terminal`, `async-channel`, `log`, and
nothing from fastedit's editor crates, so it can move into kiln later). Add `fe-terminal-view` to `UI_CRATES`
in `scripts/check-no-gpui.sh`. Re-export `fe_terminal::TerminalOptions` so the workbench depends on this crate
alone.

- `TerminalStyle { font_family: SharedString, font_size: Pixels, line_height: f32, background, foreground,
  cursor, selection: Hsla, ansi: [Hsla; 16] }` with `TerminalStyle::ansi_dark()` and `ansi_light()` (the
  xterm-like 16 colours each) and `fn ansi_for(background: Hsla) -> [Hsla; 16]` that picks light when the
  background's lightness is above 0.5. The workbench card builds it from `EditorTheme`. Themeable ANSI colours
  (a `[terminal]` table in kiln's theme files) are a later card, after T246, T247 and the second-theme work.
- `TerminalView` (an `Entity`, `Focusable`, `EventEmitter<TerminalViewEvent>`): `TerminalView::new(options:
  TerminalOptions, style, window, cx) -> io::Result<Self>` spawns the session on first layout, once the size
  is known. `set_style(style, cx)`. Key context `terminal` (`pub const KEY_CONTEXT`). A task drains the
  session's events: `Wakeup` notifies (at most once per frame), `Title` is kept (`title()`), `Exited(code)`
  emits `TerminalViewEvent::Exited(code)`, `Bell` is ignored for now.
- A `TerminalElement` (follow `crates/fe-gpui/src/element.rs`'s pattern of shaping with
  `window.text_system().shape_line` and painting with `paint_quad`):
  - Cell width is the advance of `m` in the font at `font_size`; cell height is `(font_size × 1.25)`
    rounded to whole pixels (the editor's 1.6 is too loose for a terminal).
  - In prepaint, cols = ⌊width / cell width⌋ and rows = ⌊height / cell height⌋, at least 2 × 1; when they
    change, `Terminal::resize`.
  - Paint the background, then merged background runs for cells whose bg isn't the default, the selection,
    then text runs (one per span of equal style, each started at `col × cell width` so glyphs stay on the
    grid; wide characters take two cells), then the cursor: a filled block with the cell's character in the
    background colour when focused, a hollow block when not, a 2 px beam or underline per `CursorShape`;
    nothing when hidden.
  - Colours: `Foreground`/`Background` → the style's; `Named(i)` → `ansi[i]`; `Indexed(i)` (16–255) →
    `fe_terminal::indexed_rgb`; `Rgb` as given. Inverse swaps fg and bg; dim is fg at 70 % alpha; hidden
    paints no glyph; bold and italic set the font's weight and style; underline and strikeout as GPUI text
    decorations.
- Put the cell-to-paint logic in a pure function, `layout::rows(&Snapshot, &TerminalStyle, focused) ->
  Vec<RowLayout { backgrounds: Vec<(Range<usize>, Hsla)>, runs: Vec<Run { col, text, fg, bold, italic,
  underline, strikeout }> }>` plus `CursorLayout`, so it's tested without a window.

Tests: `crates/fe-terminal-view/tests/layout.rs` (pure layout) and `crates/fe-terminal-view/tests/view.rs` (a
`VisualTestContext` window, the session running `/bin/sh -c …`, waiting with a 5 s deadline).

**Linux.** Nothing here is platform-specific; Neon checks it renders under Xvfb once the panel card lands.

## Implementation notes

- `layout::rows` and `layout::cursor` are the pure halves; `layout::colour` resolves one `Color`.
  `Run` also carries `cells`, the columns it covers (2 for a wide character).
- A wide character is always a run of its own, and runs are shaped with `force_width` = the cell
  width, so fallback-font glyphs and the glyphs after a wide character stay on the grid.
- Blank cells extend a run of the same style but never start one; trailing blanks are trimmed unless
  underlined or struck out. Hidden cells end the run and draw nothing (their background still paints).
- With a focused block cursor the cursor cell's glyph is left out of the row's runs and carried by
  `CursorLayout::Block { glyph }`, in the cell's resolved background colour, painted over the block.
- Unfocused, the cursor is a hollow block whatever its shape; focused Beam/Underline are 2 px bars.
- `TerminalStyle::line_height` is the multiplier (`TerminalStyle::LINE_HEIGHT` = 1.25).
- `TerminalView::new` fails up front when the cwd isn't a directory; a spawn that fails on first
  layout is logged and emits `Exited(None)`. `terminal()` and `grid_size()` expose the session and
  the size last given to it, for the input cards and tests.
