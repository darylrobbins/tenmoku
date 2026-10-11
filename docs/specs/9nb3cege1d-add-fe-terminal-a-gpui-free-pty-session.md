---
task_uid: "01m4p1wt4fqfc8bt9nb3cege1d"
task: "T1"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-terminal test session::runs_a_command_in_cwd: /bin/sh -c 'pwd; printf hello' spawned in a temp dir shows that dir and hello in snapshot(), then TerminalEvent::Exited(Some(0)) arrives"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal test session::input_resize_and_scrollback: bytes written to cat echo back; after resize to 100x30 stty size prints '30 100'; after 2000 lines of output scroll(-50) gives display_offset 50 showing earlier lines, and input() returns it to 0"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal test session::selection_and_drop: Word and Line selections return their text via selected_text(); dropping the Terminal ends the child (kill(pid,0) gives ESRCH within 5 s)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-terminal test keys::encodes_xterm: encode gives ctrl-a 01, ctrl-[ 1b, alt-x 1b 78, up CSI A and SS3 A in app-cursor mode, ctrl-right CSI 1;5C, shift-tab CSI Z, backspace 7f; encode_paste wraps in ESC[200~/ESC[201~ and strips an inner ESC[201~ when bracketed, else turns \\n into \\r; indexed_rgb(16)=(0,0,0), (231)=(255,255,255), (232)=(8,8,8)"
    required: true
    manual: false
  - id: "C5"
    text: "The gate's deny and no_gpui stages pass with deny.toml unchanged, and docs/decisions/terminal.md records alacritty_terminal 0.26 (Apache-2.0) and the rule that Zed's GPL terminal crates are never read"
    required: true
    manual: false
---

# Add fe-terminal, a GPUI-free pty session on alacritty_terminal that spawns the login shell in a directory, takes input and resizes, and exposes grid snapshots, selection and xterm key encoding (Mac T378)

Filed on argon from Mac T378 (card ~/coordination/argon-cards/T378.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-a.md). Mac task uid 01m4p1gh8f4yvz5ej2b2jzjmb4.

## Problem

The user asked on 2026-10-11: "let's also spec out a terminal pane that we can run in the app". fastedit has no
terminal or pty code: nothing under `crates/` or `kiln/` spawns a pty or parses VT output (`rg -i
'openpty|forkpty|alacritty|portable.pty'` finds nothing). `app-shell.md` §1 and §4.2 always planned "later
terminals" as items the shell hosts, and kiln-shell already has a bottom dock (`DockPosition::Bottom`,
`kiln/crates/kiln-shell/src/dock.rs:46`).

This card is the GPUI-free core: a pty session with a VT grid, tested headless. Cards after it render it
(fe-terminal-view), take keyboard and mouse input, and put it in a bottom panel of the workbench.

## Change

**Library (decided).** `alacritty_terminal` 0.26 (Apache-2.0, crates.io; checked 2026-10-11) for the VT parser,
grid, scrollback, selection and the pty, using its own `tty` module and `EventLoop`. Its dependencies are all
on `deny.toml`'s allowlist (vte, polling, rustix, rustix-openpty, signal-hook, parking_lot, base64, home,
regex-automata, unicode-width: MIT and/or Apache-2.0). Not `portable-pty` (MIT, fine, but a second pty layer
beside the one alacritty_terminal already ships). Depend on it unmodified; no deny.toml change. Zed's
`terminal` and `terminal_view` crates are GPL-3.0: never read them (CLEAN_ROOM.md).

**Record it** in a new `docs/decisions/terminal.md`: the library choice and why, licences, the clean-room
rule above, the crate split (`fe-terminal` core, `fe-terminal-view` GPUI) and the macOS-first / Linux notes
below.

**New crate `crates/fe-terminal`** (GPUI-free; `scripts/check-no-gpui.sh` covers it automatically). Add
`alacritty_terminal = { version = "0.26", default-features = false }` to `[workspace.dependencies]` (an
append; Cargo.toml and Cargo.lock are shared append-only). Expose fastedit's own types, never alacritty's, so
the view never names alacritty:

- `TerminalOptions { cwd: PathBuf, shell: Option<(String, Vec<String>)>, scrollback: usize (default 10_000) }`.
  `shell: None` is the user's login shell (alacritty's default: `/usr/bin/login -flp` on macOS, the passwd
  shell elsewhere). The child's env adds `TERM=xterm-256color`, `COLORTERM=truecolor`,
  `TERM_PROGRAM=fastedit` through `tty::Options::env`. Never call `alacritty_terminal::tty::setup_env`: it
  calls `env::set_var` in a multi-threaded app.
- `Terminal::spawn(options, size: GridSize { cols, rows, cell_width, cell_height }) -> io::Result<(Terminal,
  async_channel::Receiver<TerminalEvent>)>`. The event listener forwards alacritty `Event`s as
  `TerminalEvent::{Wakeup, Title(String), Bell, Exited(Option<i32>), ClipboardStore(String)}`; `PtyWrite` is
  written back to the pty inside the crate. `Wakeup`s may be coalesced.
- `input(&self, bytes: impl Into<Cow<'static, [u8]>>)`, `resize(GridSize)`, `scroll(lines: i32)`,
  `scroll_to_bottom()`; `input` scrolls to the bottom.
- `snapshot(&self) -> Snapshot`: the visible rows at the current display offset, each a `Vec<Cell { ch, extra:
  Option<String> (zero-width), fg: Color, bg: Color, flags: CellFlags }>`; `Color::{Foreground, Background,
  Named(u8 /*0–15*/), Indexed(u8), Rgb(u8, u8, u8)}`; `CellFlags` bold, italic, dim, underline, strikeout,
  inverse, hidden, wide, wide_spacer; the cursor (`row`, `col`, `CursorShape::{Block, Beam, Underline,
  Hidden}`); `display_offset`, `history_size`; the selection as viewport ranges; `modes: Modes` (app_cursor,
  app_keypad, bracketed_paste, alt_screen, alternate_scroll, focus_reporting, mouse_report_click,
  mouse_drag, mouse_motion, sgr_mouse). `fn indexed_rgb(i: u8) -> (u8, u8, u8)` for 16–255 (xterm cube and
  greys). The lock on `Term` is held only to copy.
- Selection: `select(kind: SelectionKind::{Simple, Word, Line}, row, col, side)`, `extend_selection(row, col,
  side)`, `clear_selection()`, `selected_text() -> Option<String>`, in viewport coordinates.
- `keys::encode(key: Key, mods: Mods, modes: &Modes) -> Option<Vec<u8>>` (xterm): `Key::{Char(char), Enter, Tab,
  Backspace, Escape, Up, Down, Left, Right, Home, End, PageUp, PageDown, Insert, Delete, F(u8)}`. ctrl-letter
  → 0x01–0x1a, ctrl-space and ctrl-@ → 0x00, ctrl-[ \ ] ^ _ → 0x1b–0x1f; alt prefixes ESC; backspace 0x7f;
  enter `\r`; shift-tab `CSI Z`; arrows/home/end `SS3` in app-cursor mode and `CSI` otherwise, with modifiers as
  `CSI 1;{1+mods}X`; F1–F12, insert/delete/page keys as xterm. `keys::encode_paste(text, bracketed) ->
  Vec<u8>`: bracketed wraps in `ESC[200~`…`ESC[201~` and strips any `ESC[201~` inside; otherwise newlines
  become `\r`.
- `Drop` sends `Msg::Shutdown` and does not join on the caller's thread (alacritty's `Pty` drop SIGHUPs and
  waits for the child on the event-loop thread).

Tests in `crates/fe-terminal/tests/{session,keys}.rs`. Session tests spawn `/bin/sh -c …` explicitly (never the
user's login shell, whose rc files vary) and wait for a condition on the snapshot with a 5 s deadline, never a
fixed sleep.

**Linux.** alacritty's tty is unix-wide, so this crate builds and tests on Linux unchanged; Neon runs its
tests there. Windows (ConPTY) is out of scope.

## Implementation notes

- fe-terminal opens the pty with `rustix-openpty` (a new workspace dependency,
  already in the lock through alacritty_terminal) and hands it to
  `tty::from_fd`, keeping a duplicate of the user side open until the event
  loop stops. Without it C1 failed on Linux every time: the child hangs up the
  pty before alacritty's loop reads, the loop skips the HUP-flagged readable
  event, and its drain-on-exit read drops the bytes when the next read is EIO.
  `docs/decisions/terminal.md` records this.
- The selection in `Snapshot` is `Vec<SelectionSpan { row, start_col, end_col }>`,
  one span per visible row, end exclusive.
- `Snapshot` also carries `cols`, and `row_text`/`text` helpers for tests and
  accessibility; `Terminal::pid()` exposes the child's pid (C3 needs it).
- `indexed_rgb` also answers 0–15 with xterm's default ANSI colours.
- Ctrl also maps xterm's digit aliases (ctrl-2…8) and ctrl-/ and ctrl-?;
  ctrl-backspace sends 0x08; F13 and up encode to `None`.
- From review: `Exited` is sent on alacritty's `Exit` (after its drain on exit),
  carrying the code `ChildExit` reported, so the child's last output is in the
  grid when it arrives; the pty thread also sends a deduplicated
  `Exited(None)` when the loop stops for any other reason, including drop.
  Empty input is dropped (an empty write stalls alacritty's writer). `spawn`
  fails with `NotFound`/`NotADirectory` for a bad `cwd`, since alacritty
  ignores a failed `chdir`. Selection spans cover both cells of a wide
  character when an end lands on either half.
