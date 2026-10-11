# Terminal pane: library, licences and crate split

Status: **build.** Decided 2026-10-11. Source card Mac T378 (argon T1), from
the user's request "let's also spec out a terminal pane that we can run in the
app". `app-shell.md` §1 and §4.2 already planned terminals as items the shell
hosts, in kiln-shell's bottom dock.

## Library: alacritty_terminal 0.26

`alacritty_terminal` 0.26 (crates.io, **Apache-2.0**; checked 2026-10-11) is
the VT parser, grid, scrollback, selection and pty. fastedit uses its own `tty`
module (openpty, spawn, SIGCHLD, resize) and `EventLoop` (the thread that reads
the pty into the shared `Term` and writes input), unmodified, with
`default-features = false` (no serde).

Why it: it is the engine of a mature terminal, maintained, permissively
licensed, and covers parser, grid and pty in one crate with no GUI attached.
Its dependencies are all on `deny.toml`'s allowlist (MIT and/or Apache-2.0,
some also Zlib or LLVM-exception): vte, cursor-icon, polling, rustix,
rustix-openpty, signal-hook, parking_lot, base64, home, regex-automata,
unicode-width, bitflags, libc, log. `deny.toml` is unchanged.

Not `portable-pty` (MIT, fine to use, but a second pty layer beside the one
alacritty_terminal already ships). Not a hand-written parser: VT handling is a
long tail of compatibility work alacritty has already done.

`rustix-openpty` 0.2 (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT,
already in the lock through alacritty_terminal) is also a direct dependency:
fe-terminal opens the pty itself (below).

## Clean room

Zed's `terminal` and `terminal_view` crates are **GPL-3.0**. They are never
read, for this card or any later terminal card, under the same rule as Zed's
other GPL crates (`CLEAN_ROOM.md`). Behaviour is learned from alacritty's
Apache-2.0 source, xterm's documentation (ctlseqs) and by running terminals.

## Crate split

- **`fe-terminal`** (this card): GPUI-free core, tested headless.
  `scripts/check-no-gpui.sh` covers it. It exposes fastedit's own types only
  (`Terminal`, `TerminalOptions`, `GridSize`, `TerminalEvent`, `Snapshot`,
  `Cell`, `Color`, `CellFlags`, `Cursor`, `Modes`, `SelectionKind`, and
  `keys::{Key, Mods, encode, encode_paste}`), so the view never names
  alacritty_terminal and the engine could be swapped behind it.
- **`fe-terminal-view`** (later cards): the GPUI element that renders a
  `Snapshot`, turns key and mouse input into `keys::encode` and selection
  calls, and lives in a bottom panel of the workbench.

## Session design

- `Terminal::spawn` returns the handle and an `async_channel::Receiver` of
  `TerminalEvent::{Wakeup, Title, Bell, Exited, ClipboardStore}`. `PtyWrite`
  (device reports) and text-area size requests are answered inside the crate;
  clipboard reads (OSC 52 paste) and palette queries go unanswered for now.
  `Exited` is sent once, on alacritty's `Exit` (which follows its drain of the
  child's last output) with the code from `ChildExit`, or as `Exited(None)`
  by the pty thread when the loop stops any other way (an I/O error, drop).
- `spawn` checks that `cwd` is a directory the user can enter (`stat`, then
  `access(X_OK)`, which leaves the app's cwd alone): alacritty's spawn ignores
  a failed `chdir` and would run the shell in the app's own directory.
- **An unfinished synchronized update is flushed on the child's exit.** After
  `ESC[?2026h` the VTE parser buffers output until the update ends or its
  150 ms timeout fires, and alacritty's loop stops on the child's exit with
  neither, so that output never reached the grid. The parser is private to the
  loop, so `FlushingPty` wraps the pty and, once the child has exited and its
  output is read, feeds the parser `ESC[?2026l` through the reader. Where no
  update is open that does nothing. Shutdown on drop and I/O-error stops still
  discard the buffer: nothing reads the grid after a drop, and an error stop
  has no further reads to feed.
- Empty `input` is dropped: alacritty's writer treats a zero-byte write as
  "would block" and never moves past it.
- The child's environment adds `TERM=xterm-256color`, `COLORTERM=truecolor`
  and `TERM_PROGRAM=fastedit` through `tty::Options::env`. **Never call
  `alacritty_terminal::tty::setup_env`**: it calls `env::set_var`, which is
  unsound in a multi-threaded app.
- `snapshot()` holds the `Term` lock only to copy the viewport.
- `Drop` sends `Msg::Shutdown` and never waits. `EventLoop::spawn` returns the
  pty through its join handle, and alacritty's `Pty` drop SIGHUPs and waits for
  the child, so a small `fe-terminal pty` thread joins the loop and does that
  wait off the app's thread.
- **The pty's user side is held open until the event loop stops.**
  fe-terminal opens the pty with `rustix-openpty` and passes it to
  `tty::from_fd`, keeping a duplicate of the user fd. Without it, on Linux, a
  child that writes and exits at once (`sh -c 'pwd; printf hello'`) hangs up
  the pty before the loop reads: the loop skips a readable event flagged HUP,
  and its drain-on-exit read loses bytes when the next read fails with EIO.
  With the user side open the controller never sees HUP or EIO, and the
  drain reads to `EAGAIN`.

## Platforms

- **macOS first.** `shell: None` runs alacritty's default login shell,
  `/usr/bin/login -flp $USER`, so the user's profile runs as in Terminal.app.
- **Linux.** alacritty's tty is unix-wide, so the crate builds and tests on
  Linux unchanged (Neon and argon run its tests); `shell: None` is the passwd
  shell. The child also inherits `ALACRITTY_WINDOW_ID` and `WINDOWID` (set to
  0) from alacritty's spawn code, which fe-terminal can't remove.
- **Windows** (ConPTY) is out of scope.
