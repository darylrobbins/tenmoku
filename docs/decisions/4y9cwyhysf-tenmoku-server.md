# tenmoku-server: daemon, `--stdio` attach and sessions (T396, remote 3a)

The choices behind `crates/tenmoku-server` that
[`j8wvgszah0-remote-development.md`](j8wvgszah0-remote-development.md) ("D")
§ 3 left open. The protocol it serves is fe-remote-proto's
([`7142g56xt6-remote-proto-mux.md`](7142g56xt6-remote-proto-mux.md)).

## Shape

- **A library and a thin binary.** `Daemon` (claim, run, snapshot),
  `stdio::attach`, `Paths` and `serve` are the library. `main.rs` only picks
  `--stdio`, `daemon` or `--version`. `serve(link, config)` runs one session
  over any `Link` and ends it with the link (nothing else could resume it),
  so a client's tests can run a server in-process without the binary.
- **Unix only.** The library is `#![cfg(unix)]`, and on Windows the binary
  only says it runs on Linux and macOS. The workspace still builds and
  clippies on the Windows runner. The only dependencies are fe-remote-proto
  and libc.

## Files and the lock

- `Paths::new(build, runtime_dir, home)`: `$XDG_RUNTIME_DIR/tenmoku/`, else
  `~/.cache/tenmoku-server/<build>/run/`. The socket, lock and log are
  `<build>.sock`, `.lock` and `.log`, so in the shared XDG folder two builds'
  daemons sit side by side (D § 3, upgrades: no cross-build attach). A
  relative or empty `XDG_RUNTIME_DIR` counts as unset, as the XDG spec says.
- **The pattern is `crates/fastedit/src/ipc.rs`'s, copied rather than
  shared,** because that file is the app's (GPUI) and not to change: a
  mode-700 folder that must be this user's and not a symlink, a lock file
  `flock`ed for the daemon's whole life as the liveness test, a socket
  replaced when the lock is free, mode 600, a fallback to
  `/tmp/tenmoku-server-<uid>/` when the path is too long for `sun_path`
  (104 on macOS), and a keeper that puts the socket and lock back every 5 s
  if a cleaner deletes them.
- **The daemon writes its pid in the lock file.** The ipc pattern doesn't
  need one. Here it is how a person, and the attach test, find the daemon.

## `--stdio`

- Connects to the socket. If that fails and the lock is free, it starts
  `<current exe> daemon` with `setsid` in a `pre_exec` (D § 3 says setsid;
  ipc.rs only makes a process group), stdin and stdout on `/dev/null`, cwd
  `/`, stderr appended to `<build>.log`. Then it retries every 20 ms for up to
  20 s. A daemon that exits with failure first ends the attach with the log's
  path. Two attaches that race both start one: the daemon that gets the lock
  serves, and the other exits 0.
- **The splice is two plain copies.** Stdin's EOF becomes the socket's write
  shutdown, so the daemon reads EOF and hangs up in turn. The daemon's EOF,
  or a failed write to stdout (ssh gone), exits the process. Stdout is
  flushed after every read, since std's stdout is line-buffered.
- **Known limit:** a daemon that exits after its linger just as an attach
  connects drops that connection before its Welcome. The attach then exits,
  and the client's reconnect (phases 4 and 9) runs it again. The daemon
  narrows the window: once it has decided to exit, a Hello is refused with
  "shutting down; connect again" rather than given a session.

## Sessions

- **One session per link**, made in the handshake's accept callback. The
  mux refuses a Hello of another protocol or build before it calls the
  callback, so no session is made for one (C4).
- **Resume, before T1991.** A Hello resuming a session that is waiting out
  its grace attaches to it, with empty `streams`, as there are no streams
  yet. Any other resume (unknown, already attached, or already torn down)
  gets a fresh session, and the client sees a different token.
- **A link that is lost starts the grace.** The server doesn't ping (the
  client does, D § 2.5). A dead ssh kills `--stdio`, which closes the socket.
  A detach from an old link doesn't touch a session that a newer link has
  taken.
- **Control only.** Pings are the mux's. `Open` and every request on the
  fixed service channels end at once with `Status::Error(Unsupported)` and a
  sentence naming what's missing. Upload data that follows a refused write
  is credited back by the mux.

## Teardown and exit

- **KILL is due `TERM_WAIT` (2 s) after the TERM actually went,** however
  late the reaper was to send it, so a group always gets its two seconds to
  shut down (review x77peg F1). The linger counts from the last session's
  grace end.
- **A grace that is over is over, whether or not the reaper has got to it.**
  A resume and a `spawn_group` first end every overdue session under the
  same lock, so a resume that arrives late gets a fresh session and a spawn
  into an overdue session fails with `NotFound` (F3).
- **Process groups belong to a session** through `Daemon::spawn_group`,
  which spawns a command with `process_group(0)` and keeps the leader's
  `Child`. Teardown is `kill(-pgid, SIGTERM)`, then `SIGKILL` 2 s later to
  every due group at once, then a wait for the leaders on a thread of the
  session's own, so a leader stuck in the kernel holds up only its own
  reaping, not the reaper's other deadlines (F2). A reaper thread that
  can't start leaves its leaders unreaped (zombies until the daemon exits,
  their groups KILLed all the same) and records the teardown with no exits.
  That path uses the reaper's own guard, never the lock again (review 2d43nx
  F4). The `test-support` feature's `testing::fail_reaper_threads` injects
  the failure. **The leader is reaped
  only after the KILL:** while
  it is an unreaped child its pid, and so the group id, can't be handed to
  another process, so neither signal can reach a stranger's group. The
  services of the next cards spawn through this. A leader that exits early
  stays a zombie until its session ends; a service that needs the exit
  status sooner will need its own reaping, and the same rule.
- **The daemon exits once it has no session, no teardown waiting for its
  KILL (one being reaped doesn't hold it), and `linger` (60 s) has passed since the last session ended** (or
  since it started, for a daemon whose client never came). It removes its
  socket if it is still the file it bound, then lets go of the lock.
- `Daemon::snapshot` gives the sessions, the teardowns waiting for their
  KILL, those being reaped, and the last 16 teardowns with each leader's
  exit status, for the log and tests. `Daemon::serve` serves a link as one
  of the daemon's sessions without the socket, so a test can hold the
  reaper still (a claimed daemon that isn't run) while a resume or spawn
  crosses a grace's end.

## Time and tests

- **The grace, the KILL and the linger read an injected `Clock`.** It is the
  crate's own, as fe-remote-proto's `ManualClock` can wake only that crate's
  timers. The reaper is one thread waiting on one condvar. A move of the
  manual clock notifies it, and `Daemon::settle` waits until the reaper has
  acted on the clock's time now. A test can then say both "not yet" and
  "now", with no sleeps (D § 12).
- **The daemon tests run the daemon in the test's process** on a manual
  clock, with the real `--stdio` binary attaching to it (C2) or a client on
  its socket. The attach test (C1) runs the binary end to end, the daemon
  included, and kills that daemon by the pid in the lock when it's done.
- A raw socket link needs its writer to shut down the socket's write half
  on drop (`socket_link`). The mux's reader holds another copy of the socket,
  so dropping the writer alone wouldn't close it, and the peer would never
  read EOF.
