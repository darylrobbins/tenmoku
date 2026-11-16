# Breakpoints over the line numbers, the frame arrows and the debug tints (debugger D6, T1849)

The design calls this card made in building D6 of the debugger record
(`docs/decisions/r1b9dw3g0g-debugger.md` §5, §11 D6, §13) to D0's design
(`docs/decisions/skgc7fe5z9-debugger-design.md`, `docs/design/design-system.md`
Debugger), and the Mac look check (C7).

## Calls

- **Five `[editor]` keys, not two, and D0's fallbacks.** The spec says "two
  `[editor]` keys … with fallbacks to the accent and muted tints"; D0, which
  the spec points to for the values, settles four roles: `debug_current_line`,
  `debug_frame_line`, `debug_breakpoint` and `debug_breakpoint_disabled`,
  falling back to `active_line`, `active_line`, `error` and
  `gutter_foreground` ("§5 named the accent and muted tints; an opaque accent
  band would lose the code text"). Review round 2 (F14) added a fifth,
  `debug_frame_arrow`, for the frame arrows and the current line's edge,
  falling back to `cursor`: D0 draws them in the accent, which the editor's
  palette has no key for. Yohen's and Hakuji's cursor is their accent, but
  Catppuccin Latte's cursor is 2.3:1 on its gutter and Monokai's kiln accent
  1.5:1, so neither the cursor nor the kiln token serves every theme. Yohen
  and Hakuji set it to their accent, D0's hare's fur. The fallbacks are
  `fe_config::theme::DEBUG_FALLBACKS`; C6's two band keys are among the
  five, and `crates/fe-config/tests/debug_tints.rs` covers all of them.
- **What "a theme without them" is.** A theme with an `[editor]` table of its
  own that leaves a key out gets that key's fallback in the theme's own
  colour. A theme with no `[editor]` table (Kiln's base themes) draws the
  editor in the bundled palette, as every other editor colour does, so it
  gets Yohen's or Hakuji's debug colours with the rest
  (`editor_palette::bundled_palettes` pins that). `crates/fastedit/src/theme.rs`'s
  `palette_follows_kiln`, whose user theme sets only `[editor] background`,
  now expects the fallbacks and the badge tints (added to the scope by a
  respec).
- **Every bundled theme sets them** before the minimap keys: Yohen and
  Hakuji to D0's values; the others to their own colours, worked out once by
  a script: the current line is the theme's cursor mixed into its editor
  ground until the comment colour is within 3% of its contrast on the cursor
  line; the caller frame is the cursor line made opaque; the breakpoint is the
  error colour, the arrow the theme's accent (else its cursor), and the
  disabled grey the gutter's. Then each glyph is moved toward white or black
  as little as gets it to D0's floor (review round 2, F14): 3:1 (3.05 in the
  script, for rounding) on the gutter, the cursor line and the hover tile,
  the arrow 3:1 on the current line's band where it's the edge, and the grey
  at least 0.1 OKLab L from the red. In the Tenmoku themes and VS Code Dark
  Modern that makes the grey lighter than the red; Hakuji's is too.
  `debug_tints::every_bundled_themes_glyphs_keep_the_contrast_floor` holds
  every bundled theme to it. The colour-blind
  tables are left alone: the breakpoint stays red there, apart from the accent
  arrow and the grey in lightness.
- **Sizes (D0's kiln-ui roles).** `glyph::{ICON, BREAKPOINT, ARROW_HEIGHT,
  HALF, STROKE}`, `height::TOOLBAR`, `width::TITLE_LABEL_MAX` and
  `width::CALL_STACK_DEFAULT` are in `kiln/crates/kiln-ui/src/roles.rs`. The
  new `glyph` module needs its re-export in `kiln-ui/src/lib.rs`, added to
  the scope by a respec. `run_widget.rs`'s own 200 px constant isn't
  moved (out of scope); D8 can point it at the role.
- **The view's API.** `EditorView::set_breakpoints(version, Vec<BreakpointMark>)`
  shows the marks, rows of the text at `version` anchored at each row's start
  with `Bias::Right` (as the store anchors breakpoints), so they follow edits;
  marks for another version than the buffer's show nothing until the host
  sets them again, as the view can't place rows of a text it doesn't have.
  `set_breakpoint_handlers(Option<BreakpointHandlers>)` carries the click
  (`on_toggle(row)`) and the menu (`menu(row) -> Vec<BreakpointMenuItem>`).
  A mark has a breakpoint state and a frame arrow, the two slots.
- **Slots.** D0's x 14–26 and 29–41 are measured from the right of the
  number column (42 px at 13 px code), so when five-digit numbers widen the
  column to the left the glyphs keep to the numbers. They scale with the code
  size as the gutter's columns do.
- **Clicks.** A plain left press on the column waits for its release on the
  same row to toggle, so a press that moves 3 px first is handed on as the
  press it would have been (a drag selecting rows), as the design asks. A
  right press asks the host for the row's menu once the press is handled
  (the host reads the editor).
- **The menu** is drawn by the view, under the press: the workbench's own
  render and its popup code aren't in this card's scope, and the view is
  where the press lands. It's a Role::Menu named "Breakpoint" whose items are
  Role::MenuItem nodes named by their titles. Add Breakpoint, or Disable or
  Enable Breakpoint and Remove Breakpoint; Run to Line and the Edit items are
  D8's and D17's.
- **Tooltips and names.** Each shown row of the column is a target built in
  the frame (an absolutely placed child per row), with GPUI's tooltip after
  its rest and, while a screen reader is attached, a Button node with a
  Click action (`on_a11y_action`, which toggles as a click does and adds no
  mouse handling of its own; review round 2, F13) named as the tooltip
  reads: "Add breakpoint, line 7", "Breakpoint, line 7, not bound:
  ‹message›", "Disabled breakpoint, line 7", "Stopped here, line 7",
  "Caller frame, line 7". The design's "with the key" waits for D8's F9.
- **Bands.** `Tint::DebugCurrent` and `Tint::DebugFrame` row backgrounds are
  opaque (`diff_color`) and paint first in the backgrounds layer, so other
  bands, highlights, matches and the selection draw over them, and they cover
  the cursor line in the code. The 2 px edge is in the last 2 px of the
  gutter's gap to the code, or at the code's left without a gutter: the
  accent on the current line and the chrome's muted text on a caller frame,
  which the view reads from the kiln theme (`fe_ui::ui_theme`), as the diff
  view does for its chrome, since no editor key carries muted text.
- **Workbench wiring** (`debug_gutter.rs`): the root's store gives each file
  editor's breakpoints with their lines on disk
  (`BreakpointStore::placements_in`, added by a respec after review round 1,
  read-only), so the session's verification, keyed by the line on disk it
  was sent on, matches each exactly, merged rows or not. A line the adapter
  reports is placed in the buffer as the store places breakpoints
  (`BreakpointStore::rows_of_lines`, the store's own disk-to-buffer line
  mapping), so edits between it and the line it was set on move it right;
  for the session's life only. A mark drawn elsewhere than its own row
  carries that row as its target (`BreakpointMark::target`), anchored in the
  view as the mark is, so right after an edit, before anything refreshes,
  its click and menu still act on it (round 2, F10). Two drawn on one row
  show the first, and the row's click and menu act on that one. The stopped
  frame's and a caller's rows, lines of the program on disk, are placed
  through the same mapping, so frames stay on their text through edits and
  refreshes (F11). A line added or enabled again while a session runs (or
  any line, after a save moves them) waits, a ring, for the session's next
  word on its file, so an older breakpoint's verification is never shown
  for it; a send the adapter refuses leaves it a ring (F12). While a session runs, an enabled
  breakpoint it hasn't said anything of yet (setBreakpoints pending or
  failed) is a ring with no message. A Save As, or a buffer swapped in under
  an editor, shows it again (the gutter observes each editor's file and
  buffer). `Workbench::band_debug_frame` is D9's seam for a caller frame: it
  takes only during a stop, lasts that stop's epoch (any state event, a
  retired stop included, clears it) and yields to the current frame on the
  same row (a recursive call). Stop paths are compared resolved, so a stop
  reported by a symlinked path bands the file. A menu closes when its text,
  file or marks change under it, and Add never removes.
- **Carried findings.** T1497 F1: a theme with its own `[editor]` table no
  longer inherits the bundled palette's TODO-badge colours for names its
  `[syntax]` table leaves out, so those badges take their kinds' diagnostic
  tints (`debug_tints::a_theme_without_badge_names_gets_none_of_the_bundled_ones`).
  T1550 F5: the current match's search now runs inside the frame's search
  budget, before the rows' matches, respects the frame's timeout latch, and
  is kept from frame to frame while the pattern, text and selection stay;
  `element.rs`'s `current_match_limit_tests` count the searches across
  redraws after a timeout on one row and across rows. They sit in
  `element.rs` because a new test file there isn't in the card's scope.

## The Mac look check (C7)

`crates/fe-workbench/examples/screenshots/audit/debug_gutter.rs` shows every
D6 state in `src/main.rs`, each on its own row, and both bands: an enabled
breakpoint, an unverified one, a disabled one, the stopped frame on a
breakpoint with the current line's band, and a caller frame's band and
outlined arrow. It drives the editor's public API (`set_breakpoints` and a
band provider), as `debug_gutter.rs` does, so it needs no adapter. Its
probes are token probes where a role has a kiln token (the breakpoint red is
`status.error`'s value, the arrow and the current edge `accent.base`, the
caller edge `text.muted`) and, for the colours no token has (the bands, the
disabled grey), editor probes, which the runner (`audit_scenes.rs`) now
checks against the theme's editor palette the same way: exactly, at the
capture's own scale, so they judge 1× and 2× alike and nothing skips on
`SCREENSHOTS_ALLOW_OTHER_SCALE`. A last step fails the scene unless the frame
drew every glyph and both bands.

### Recorded (2026-11-15, the lane's Mac: Mac15,11, Retina, captures at 2×)

`cargo run -p fe-workbench --example screenshots -- audit <dir>` printed, for
this card's surface:

```text
wrote <dir>/debug_gutter_dark.png
ok      debug_gutter_dark probes (4)
ok      debug_gutter_dark editor probes (3)
wrote <dir>/debug_gutter_light.png
ok      debug_gutter_light probes (4)
ok      debug_gutter_light editor probes (3)
```

The pixels at the seven points, read back from the captures:

| Point | Yohen | Hakuji | Role |
| --- | --- | --- | --- |
| (302, 146) enabled dot | `#f2716f` | `#a1100e` | `debug_breakpoint` (= `status.error`) |
| (302, 226) disabled dot | `#827468` | `#7c828a` | `debug_breakpoint_disabled` |
| (313.5, 266) current arrow | `#e0874c` | `#a84a16` | `accent.base` |
| (362, 266) current band's edge | `#e0874c` | `#a84a16` | `accent.base` |
| (378, 266) current band | `#351501` | `#fff4ed` | `debug_current_line` |
| (362, 306) caller band's edge | `#b8a99a` | `#4b525b` | `text.muted` |
| (378, 306) caller band | `#2a1a11` | `#f6f7f9` | `debug_frame_line` |

Looked at, both themes: the dot on `use`, the ring on the doc comment, the
grey dot on `let`, the dot and filled arrow on the loop body over its band
with the rust edge, and the outlined arrow on `println!` over the caller band
with its muted edge; those rows' numbers are gone, the others' stay.

The same run fails 22 other captures' probes (`chrome_*`, `settings_*`,
`shortcuts_editor_*`, `changes_*`, `git_log*_*`, `problems*_*`,
`terminal*_*`, and the `yohen` and `hakuji` chrome captures). Those surfaces
draw nothing this card changes (the title bar, panels, the settings tab);
they are drift on main from the B3 chrome cards, not this card's.

## Known limits (review round 3, follow-up T1881)

The lead filed T1881 after round 3 left only medium and low findings, and
added round 4's F16 and F17 to it; the criteria name it.

- **F11 (C3, C4).** A save that moves lines resets the store's disk baseline;
  the next refresh places the stopped and caller frames, and a line an
  adapter reported for a moved breakpoint, by their original lines against
  it, so they can leave their text. Fix: anchor them for the stop or the
  session, or map against a snapshot taken when they were reported.
- **F12 (C3).** Any `DebuggerEvent::Breakpoints(path)` clears the file's
  pending lines, an adapter's `breakpoint` event for another breakpoint
  included, not only the answer to the replacement send. A precise fix needs
  D7 to say which event answers the last send.
- **F14 (C6).** The contrast test puts the hover tile over the plain gutter;
  over the cursor line, enabled dots in Tenmoku Dark, Ember and Light and
  Monokai, and Tenmoku Light's frame arrow, sit just under 3:1.
- **F15 (C1).** The screen-reader test calls `activate_breakpoint_row`
  directly: GPUI's test platform can't dispatch an accessibility action to
  the row's node.
- **F16 (C4), round 4.** A frame band is a decoration over its line's text,
  so a newline typed inside a stopped or caller line stretches it over two
  rows (the arrow stays on one) until the next refresh. Fix: draw the band
  from an anchored row, or rebuild it on text changes.
- **F17 (C5), round 4.** The breakpoint menu takes no keys: Escape doesn't
  close it, and arrows and Enter reach the editor under it (in vim's insert
  mode, Enter edits the file). Fix: focus the menu, handle arrows, Enter and
  Escape, and give focus back on close.
