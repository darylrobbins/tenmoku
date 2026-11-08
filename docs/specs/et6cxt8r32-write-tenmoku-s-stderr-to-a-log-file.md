---
task_uid: "01m6w994kgc2kha9et6cxt8r32"
task: "T1608"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "When fd 2 is /dev/null at startup, Tenmoku points it at an append-mode log file (macOS ~/Library/Logs/Tenmoku/tenmoku.log, Linux $XDG_STATE_HOME/tenmoku/tenmoku.log), so logger lines, panics and library stderr land there; a terminal, pipe or user-redirected file is left as is. Test: log_file::tests::redirects_only_a_null_stderr"
    required: true
    manual: false
  - id: "C2"
    text: "The log rotates past 5 MB to tenmoku.log.1, keeping one old file. Test: log_file::tests::rotates_past_the_size_limit"
    required: true
    manual: false
  - id: "C3"
    text: "When Tenmoku writes to the log file (fd 2 was /dev/null), each launch's first line in that file gives version, pid, launch kind and display count. Lead ruling: the line goes only to the log file (a terminal, pipe or user-redirected stderr is left as is, per C1); the launch kind is inferred from the environment (TENMOKU_FOREGROUND, a terminal, LaunchServices' parent) with no new IPC marker; the display count is real on macOS and '?' where the platform has no enumeration before app start (Linux). Test: log_file::tests::launch_line_is_first"
    required: true
    manual: false
---

# Write Tenmoku's stderr to a log file when it would go to /dev/null, so a detached or LaunchServices launch leaves evidence (T1507 follow-up 1)

Follow-up 1 of T1507 (docs/decisions/h9n3r1vpgs-freeze.md, "Card 1"). The design is in that decision file and lands with
T1507.

**Why.** `tenmoku .` from a terminal respawns itself detached, with stdin, stdout and stderr on `/dev/null`
(`crates/fastedit/src/ipc.rs:787-804`). A launch from the Dock or Finder also gets `/dev/null`. So everything the logger
prints (`crates/fastedit/src/main.rs:450-454`, `eprintln!`) is lost, and so are panic messages. That includes gpui's
`could not start display link` and `failed to retrieve next drawable`.

The lead's sample of a frozen window (pid 62551, 2026-11-07) showed no `CVDisplayLink` running, while stderr was
`/dev/null`. Any log line that could have explained it was thrown away.

**Behaviour.**
1. At startup, before `init_logging`, check what fd 2 is.
2. If it is `/dev/null` (same device and inode as `/dev/null`), point fd 2 at the log file with `dup2`, opened for
   append:
   - macOS: `~/Library/Logs/Tenmoku/tenmoku.log`;
   - Linux: `$XDG_STATE_HOME/tenmoku/tenmoku.log`, default `~/.local/state`.

   One path then catches the logger's lines, panics, and anything a library writes to stderr.
3. A terminal, a pipe, or a file the user redirected to (`TENMOKU_FOREGROUND=1 … 2>log`, `ipc_tests.rs:338`) is left as
   it is.
4. Before opening the file, rotate it: if it's over 5 MB, rename it to `tenmoku.log.1`, replacing the old one. Only one
   old file is kept.
5. The first line of each launch gives the version, pid, launch kind (detached, LaunchServices or foreground) and the
   display count.
6. Log levels don't change: gpui errors already pass at Warn.

**Tests.**
- `log_file::tests::redirects_only_a_null_stderr`: the pure decision, by what fd 2 is. Null → redirect. Terminal, pipe or
  regular file → leave it.
- `log_file::tests::rotates_past_the_size_limit`, in a temporary directory: a file over the limit becomes `.1`, a second
  rotation replaces `.1`, and a file under the limit is appended to.
- `log_file::tests::launch_line_is_first`: opening the log in a temporary directory writes the launch line before any
  record.

## Implementation notes

- Per the rev 2 ruling: the launch line goes only to the log file; launch kind is inferred from the environment with no
  new IPC marker; the display count is real on macOS (`CGGetActiveDisplayList`) and `?` on Linux.
- The redirect test runs `stderr_kind` and `redirect` on real descriptors (null, pipe, regular file).
  `install_follows_the_real_stderr` runs `install` in a subprocess with null and with piped stderr. A pseudo-terminal
  case is not covered: it would need a pty dependency, outside this task's scope.
