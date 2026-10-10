---
task_uid: "01m4kkdankaa4w8yg6t88d3m49"
task: "T332"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In tests::lsp::restart_and_show_log, s.wait_item(Some(\"rust-analyzer: starting…\")) runs after the first s.fake.wait_initialized() and before the first s.run(\"lsp.restart\"); the test's other steps and assertions are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "Only crates/fe-workbench/src/tests/lsp.rs changes: no production code, no change to crates/fe-lsp/src/tests/fake.rs or its WAIT, no .config/nextest.toml retry"
    required: true
    manual: false
  - id: "C3"
    text: "The decision file shows the cause: with a temporary, uncommitted 200 ms sleep before status.send(ServerStatus::Starting) in Server::run (crates/fe-lsp/src/server.rs), the test fails at base with 'fake: timed out waiting for initialized: []' at the launch-2 wait (lsp.rs ~605), and passes with the fix"
    required: true
    manual: false
  - id: "C4"
    text: "20 consecutive runs of scripts/ci-env.sh cargo nextest run --profile ci -p fe-workbench -E 'test(=tests::lsp::restart_and_show_log)' pass while the full fe-workbench suite runs alongside, with the load average and results in the decision file"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# Flaky under load: fe-workbench tests::lsp::restart_and_show_log: wait for the server's starting status before the first lsp.restart

## Problem

`fe-workbench tests::lsp::restart_and_show_log` fails under load (Mac 1-min load 50–260) with

```
thread 'tests::lsp::restart_and_show_log' panicked at crates/fe-lsp/src/tests/fake.rs:433:13:
fake: timed out waiting for initialized: []
```

after about 10.1 s, on its only try (no nextest retry override). It's seen in main's fast CI run
`01m4kjdxkaasb71vzzrfgya34t` (dc8234a, 10-10 14:54), gate run `01m4kga8cbyddra42b3hd6yab7` (c972647,
10-10 14:30) and 7 older runs: `01m4f9nwqcs5sn06pj53335sgr`, `01m4fgea8nzym3df9djmf26tz4`,
`01m4fgpxz9sj6z3122mw5fv3jz`, `01m4fjd84a4n96vvdyt2v11gsw`, `01m4fjfe3avbf0km6w9eh12vn5`,
`01m4gavmvyxb6ajfmbnva9bb8y`, `01m4gcehgpzew2spz6dfaqkg7f`. The logs are in
`~/.local/state/regatta/runs/<run>/test.fastedit.log`. Run alone, the test passes in about 0.06 s.

The 10 s fake `WAIT` isn't too short, and the launch isn't slow. The test's first `lsp.restart` is
sometimes dropped, so launch 2 never happens and its record stays `[]`:

- `s.open("src/main.rs")` draws, and the frame's deferred `Workbench::sync_lsp`
  (`crates/fe-workbench/src/workbench.rs` ~1110) calls `manager.open`, which spawns the server thread.
  That thread sends `ServerStatus::Starting` (`crates/fe-lsp/src/server.rs` ~371) into the workbench's
  event channel. The workbench only records it in `lsp.servers` when its event-loop task runs
  (`Workbench::start_lsp`, `lsp_event`, `crates/fe-workbench/src/lsp.rs` ~827), on the gpui test
  executor.
- `s.fake.wait_initialized()` then blocks the test thread on the fake's condvar. Nothing ticks the
  executor there.
- `s.run("lsp.restart")` dispatches the command synchronously in `cx.update`, before `draw` runs
  anything. `Workbench::run_lsp_op` (`lsp.rs` ~945) needs `lsp.server(buffer)`, which needs the
  `lsp.servers` entry. If `Starting` arrived after `open`'s last `run_until_parked` (the server thread
  wasn't scheduled in time, as happens under load), the entry is missing. `run_lsp_op` then returns
  silently, nothing restarts, and `s.fake.wait_initialized_at(2)` (`crates/fe-workbench/src/tests/lsp.rs`
  ~605) times out on an empty launch-2 record about 10 s after the test starts, as every failure shows.

The other `wait_initialized` callers don't run a server command straight after a blocking fake wait.
`status_item_follows_the_server`, for one, waits for the status item, which draws, first. The second and
third `lsp.restart` in this test are safe: the restart itself inserts the restarted server into
`lsp.servers` (`lsp.rs` ~965), and the third comes after a `wait_item`.

## Change

Test-only, in `crates/fe-workbench/src/tests/lsp.rs`, `restart_and_show_log`. Right after the first
`s.fake.wait_initialized();`, add `s.wait_item(Some("rust-analyzer: starting…"));`. It draws until the
workbench has applied the server's `Starting` status, so `lsp.restart` acts on a server the workbench
knows about, as a user's restart from the status item does. Add a one-line comment saying why. Leave the
rest of the test and its assertions as they are.

Prove the cause before the fix, and record it in the decision file. Add a temporary, uncommitted
`std::thread::sleep(Duration::from_millis(200))` just before `status.send(ServerStatus::Starting)` in
`Server::run` (`crates/fe-lsp/src/server.rs`). The test then fails at base with
`timed out waiting for initialized: []` from line ~605, and passes with the fix. Remove the sleep. If
the test still fails at launch 2 with the fix and the sleep in place, stop and report: the cause is
somewhere else, and the card needs respeccing.

Change nothing else: no production code, no change to fe-lsp's fake or its `WAIT`, and no
`.config/nextest.toml` retry.

## Implementation notes

- Cause check (C3): with a temporary, uncommitted 200 ms `thread::sleep` before `status.send(ServerStatus::Starting)` in `Server::run` (load avg ~41), the test at base failed after 10.5 s with `fake: timed out waiting for initialized: []` at fake.rs:433, from the launch-2 wait. With the fix and the same sleep it passed (1.2 s). The sleep was removed.
- Load check (C4): 20 consecutive `scripts/ci-env.sh cargo nextest run --profile ci -p fe-workbench -E 'test(=tests::lsp::restart_and_show_log)'` runs passed (20/20) while a full fe-workbench suite ran alongside; 1-min load average 37–41 (macOS).
- `cargo nextest run -p fe-workbench`: 252 passed, 2 skipped. `rgt ci run --fast` passed.
