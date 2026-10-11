---
task_uid: "01m4p1wtanzb4jhda10j260te0"
task: "T4"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-terminal-view test mouse::encode: SGR press, release, drag and wheel with modifiers, the X10 encoding, X10 coordinates over 223 dropped, and None while reporting is off"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal-view test mouse::drag_selects: dragging across 'hello world' selects the dragged text, a double click selects a word, a triple click the line, and shift-click extends the selection"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal-view test mouse::wheel: one line-delta notch scrolls 3 lines of 500 lines of output; on the alt screen with alternate scroll on it writes ESC[A and display_offset stays 0"
    required: true
    manual: false
  - id: "C4"
    text: "fe-terminal-view test mouse::reporting: after ESC[?1000h ESC[?1006h a click writes CSI \u003c0;x;yM then m; with shift held it selects and writes nothing"
    required: true
    manual: false
---

# Select, scroll back and report the mouse in the terminal: drag, double- and triple-click selection, wheel scrollback, and SGR mouse reporting for full-screen programs (Mac T381)

Filed on argon from Mac T381 (card ~/coordination/argon-cards/T381.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-a.md). Mac task uid 01m4p1gks252v9817e2wd0n3nc.

## Problem

The user asked on 2026-10-11 for "a terminal pane that we can run in the app". After the view and keyboard
cards the terminal shows output and takes keys, but the mouse does nothing: no selection to copy, no
scrolling back through output, and full-screen programs (htop, less, nvim with `mouse=a`) get no mouse
events.

## Change

In `crates/fe-terminal-view`, mouse listeners on the terminal element, with positions mapped to (row, col,
side) by the cell size the element laid out:

- **Selection.** Left press starts a `SelectionKind::Simple` selection, drag extends it, a double click
  selects a `Word`, a triple click a `Line`, shift-click extends the current selection. Dragging above or below
  the element scrolls the scrollback a line per frame while held. The selection paints (the view card already
  draws `snapshot.selection`) and stays until input or a new press clears it; copy is the keyboard card's.
- **Wheel.** Pixel deltas accumulate and scroll whole lines (`Terminal::scroll`); line deltas scroll 3 lines
  each. On the alternate screen with `modes.alternate_scroll` on and mouse reporting off, the wheel sends up or
  down arrow keys (`keys::encode`, honouring app-cursor mode) instead of scrolling.
- **Mouse reporting.** When a program enables reporting (`mouse_report_click`, `mouse_drag`, `mouse_motion`),
  presses, releases, drags, motion and the wheel go to the pty as SGR sequences (`CSI < b;x;y M/m`) when
  `sgr_mouse` is on, and in the legacy X10 encoding otherwise (coordinates above 223 dropped), with shift,
  alt and ctrl in the button byte. Holding shift bypasses reporting so the user can still select. Put the
  encoding in a pure `mouse::encode(event, modes) -> Option<Vec<u8>>` so it's tested without a window.
- The pointer is an I-beam over the grid, an arrow while reporting is on.

Tests in `crates/fe-terminal-view/tests/mouse.rs`: the pure encoder, and a `VisualTestContext` window with
`simulate_mouse_down`/`move`/`up` and scroll-wheel events over a session running `/bin/sh -c …`, waiting with
a 5 s deadline.

**Linux.** Identical; middle-click paste of the primary selection on Linux is a Neon follow-up.

## Implementation notes

- `crates/fe-terminal-view/src/mouse.rs` holds the pure `encode(Event, &Modes)` (with `Button`, `Action`,
  `Event`, `reporting`) and the view's handlers. The element inserts a hitbox in prepaint and, in paint,
  sets the pointer and registers window mouse listeners carrying that frame's `Frame` (bounds, cell size,
  grid size and modes), so the mouse goes by the modes of the last frame drawn rather than snapshotting the
  grid on every event. Tests settle a frame after a mode change before using the mouse.
- Presses and the wheel need the pointer over the hitbox; moves and releases are heard anywhere so a drag
  continues outside the element. A press stops propagation only when it was reported or selected.
- Smooth (pixel) wheel motion accumulates in lines by the cell height; a reporting program hears one wheel
  press per `WHEEL_LINES` (3) lines, so one per notch. Reversing direction drops the remainder.
- Drag-past-the-edge scrolling happens in the element's prepaint (`TerminalView::drag_scroll`), which asks
  for another animation frame while there is more history in that direction.
- Shift-click extends only when there is a selection; otherwise it starts a simple one.
