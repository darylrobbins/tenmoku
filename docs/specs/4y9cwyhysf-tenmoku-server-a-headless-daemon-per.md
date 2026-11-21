---
task_uid: "01m7xbb68ch8s29s4y9cwyhysf"
task: "T396"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "tests/daemon_attach.rs: `stdio_starts_the_daemon_once_and_a_second_attach_reuses_it` passes: `--stdio` connects to the daemon's socket in `$XDG_RUNTIME_DIR/tenmoku/` (else `~/.cache/tenmoku-server/\u003cbuild\u003e/run/`), a mode-700 directory with a flock'd lock file as the liveness test, starting the daemon detached (setsid) when no lock is held"
    required: true
    manual: false
  - id: "C2"
    text: "`dropping_stdio_keeps_the_session_for_its_grace` passes: killing the --stdio process leaves the daemon and its session alive until the grace (default 10 minutes, injectable for tests) ends"
    required: true
    manual: false
  - id: "C3"
    text: "`grace_end_tears_down_the_session` passes: teardown sends TERM then KILL after 2 s to the session's process groups, and the daemon exits 60 s (injectable) after its last session"
    required: true
    manual: false
  - id: "C4"
    text: "A Hello that fails the strict match is refused before any session is made; a Hello asking to resume an unknown session gets a fresh session (resume proper is T1991)"
    required: true
    manual: false
  - id: "C5"
    text: "The crate links no gpui, fe-workbench, fe-gpui or kiln-shell (scripts/check-no-gpui.sh passes) and builds on Linux and macOS"
    required: true
    manual: false
---

# tenmoku-server: a headless daemon per host, user and build, --stdio attach through a unix socket, one session per window with a grace period and process-group teardown (remote 3a) (Mac T2042)

# tenmoku-server skeleton (remote 3a)

D § 3 (binary, two modes, sessions, grace, upgrades: no cross-build attach), § 2.5 (handshake), § 1 (stdio is the protocol, stderr is the connection log).
The crate is `crates/tenmoku-server` (D § 3; the parents' `fe-remote-server` name is superseded). It also exposes a library entry point that serves one session over a given reader/writer, so client tests can run a server in-process without a built binary. The socket and lock pattern is `crates/fastedit/src/ipc.rs`'s (read it; don't change it).
No services yet beyond control (ping, Open refused with a clear status): fs/watch/search are the next card. Static musl release builds are the release card's concern.

## Rules
Read the decision sections named above in `docs/decisions/j8wvgszah0-remote-development.md` (and the design file `docs/designs/remote-development.md` where cited) before starting; they are the spec, this card only cuts a slice of them. Do not widen --scope; if a file outside it is needed, stop and respec (notes and choices go in your lane's `docs/decisions/<lane>-*.md`, which is exempt). New tests go in new files, one fn per behaviour, small fixtures, no sleeps for timing (D § 12 spike notes). Gate tests on the affected crates only. Not here: anything the decision assigns to another phase (D § 13).

## Implementation notes

The choices this card made (file names per build, the pid in the lock, resume before T1991, reaping leaders only after the KILL, the crate's own manual clock and how the tests use it) are in `docs/decisions/4y9cwyhysf-tenmoku-server.md`.
