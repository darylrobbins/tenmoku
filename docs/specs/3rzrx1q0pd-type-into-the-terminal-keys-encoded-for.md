---
task_uid: "01m4p1wt961xzays3rzrx1q0pd"
task: "T3"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-terminal-view test keyboard::typed_text_reaches_the_shell: simulate_input of 'héllo' and an IME composition (replace_and_mark_text_in_range, then replace_text_in_range) each write their UTF-8 bytes once, and marked text is not written before it commits"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal-view test keyboard::special_keys: ctrl-c writes 03, enter 0d, up ESC[A and ESC O A once the program sends ESC[?1h; on macOS alt-left writes ESC b"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal-view test keyboard::secondary_keys_propagate: cmd-p reaches a parent's key listener and writes nothing to the pty"
    required: true
    manual: false
  - id: "C4"
    text: "fe-terminal-view test keyboard::copy_and_paste: with a selection cmd-c puts its text on the clipboard, and without one writes nothing; cmd-v writes the clipboard wrapped in ESC[200~...ESC[201~ after ESC[?2004h, and otherwise with \\n turned into \\r"
    required: true
    manual: false
---

# Type into the terminal: keys encoded for the pty, IME composition, and the platform's copy and paste (Mac T380)

Filed on argon from Mac T380 (card ~/coordination/argon-cards/T380.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-a.md). Mac task uid 01m4p1gjnrqwmyhzjvq21rh2rr.

## Problem

The user asked on 2026-10-11 for "a terminal pane that we can run in the app". After the view card,
`TerminalView` paints a shell but nothing typed reaches it. This card is the keyboard half of its input:
keys, IME composition, copy and paste. The mouse is the next card.

## Change

In `crates/fe-terminal-view` (this card owns the crate's input code; the mouse card follows it):

- **Keys.** An `on_key_down` listener on the view's element maps a `gpui::Keystroke` to
  `fe_terminal::keys::{Key, Mods}` and writes `keys::encode(key, mods, &snapshot.modes)` to the pty, then
  stops propagation. Printable characters without ctrl or alt are left to the input handler (below), so IME and
  dead keys work. A keystroke carrying the platform's secondary modifier (cmd on macOS) is never consumed: it
  propagates, so app commands (the palette, cmd-w, cmd-q, ctrl-` from the panel card) keep working. On macOS,
  option is not meta: option-letter composes through the input handler, as Terminal.app's default does;
  alt-left and alt-right send `ESC b` and `ESC f`, and alt-backspace `ESC 0x7f`. Elsewhere alt prefixes ESC.
  Any input scrolls to the bottom and clears the selection.
- **Text and IME.** Implement `gpui::EntityInputHandler` for `TerminalView` and register an
  `ElementInputHandler` in paint when focused. `replace_text_in_range` writes the text's UTF-8 to the pty;
  `replace_and_mark_text_in_range` keeps the marked (preedit) text, which paints at the cursor, underlined, over
  the grid without reaching the pty until committed; `unmark_text` drops it; `bounds_for_range` returns the
  cursor cell so the IME window sits there.
- **Copy and paste.** The platform's copy and paste keys, handled in the same listener before propagation:
  cmd-c and cmd-v on macOS, ctrl-shift-c and ctrl-shift-v elsewhere. Copy writes `Terminal::selected_text()`
  to the clipboard and is a no-op with no selection (so it never swallows ctrl-c, which reaches the shell as
  0x03). Paste reads the clipboard and writes `keys::encode_paste(text, modes.bracketed_paste)`. A
  `TerminalEvent::ClipboardStore` (OSC 52) writes the clipboard.
- **Focus.** When `modes.focus_reporting` is on, focus in and out send `CSI I` and `CSI O`.

Tests in `crates/fe-terminal-view/tests/keyboard.rs`, driving a focused view with `simulate_keystrokes` and
the input handler, the session running `cat` or `/bin/sh -c 'stty raw -echo; od -An -tx1'` to observe the
exact bytes, waiting with a 5 s deadline.

**Linux.** The ctrl-shift copy and paste keys are the Linux convention. On Linux the workbench binds many plain
ctrl- keys (secondary is ctrl there), which kiln's matcher takes before the terminal sees them; that keymap
policy is the panel card's note and a Neon follow-up, not this card's.

## Implementation notes

- `crates/fe-terminal-view/src/input.rs` decides what a keystroke does (`key_input`): copy, paste, a key to
  encode, fixed bytes (macOS option-left/right/backspace), or pass. Pass covers printable text, which the input
  handler takes, and any keystroke with the platform modifier, which propagates. On Linux the platform modifier
  is super, so `cmd-p` in the tests is super-p there; plain ctrl keys stay the terminal's (see the Linux note).
- Copy with nothing selected writes nothing and lets the key propagate.
- The input handler's only document is the marked text, so `text_for_range`, `marked_text_range` and
  `selected_text_range` are in its UTF-16 units. The platform's `paste` goes through `encode_paste` as well.
- Focus reporting (`CSI I` / `CSI O`) is in, but no criterion covers it and there is no test for it.
- The tests run `stty raw -echo opost` and `dd bs=64 count=1 | od -An -tx1 -v` in a loop (portable to BSD
  `od`, which has no `-w`). After the first read the program turns on `?1h` and `?2004h`, so one session
  covers both modes.
