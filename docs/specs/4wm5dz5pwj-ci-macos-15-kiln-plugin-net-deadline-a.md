---
task_uid: "01m6ygp2scha9cqh4wm5dz5pwj"
task: "T310"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln-plugin::net_deadline a_fetch_is_capped_by_what_is_left_of_the_program_share arms a share of about 2 s, well below the 5000 ms request timeout, and still asserts the sent timeout is \u003c= that share"
    required: true
    manual: false
  - id: "C2"
    text: "all kiln-plugin::net_deadline tests pass, 50 consecutive runs of the changed test pass, and only tests/net_deadline.rs changes"
    required: true
    manual: false
---

# CI (macos-15): kiln-plugin net_deadline a_fetch_is_capped_by_what_is_left_of_the_program_share flakes because its 50 ms program share is shorter than a runner stall (Mac T1623)

# CI (macos-15): kiln-plugin's a_fetch_is_capped_by_what_is_left_of_the_program_share arms a 50 ms share, so a stalled runner makes the fetch time out

## Evidence

GitHub CI `check` job (macos-15), run 42520105964, main 7358064f:

    FAIL kiln-plugin::net_deadline a_fetch_is_capped_by_what_is_left_of_the_program_share
      panicked at kiln/crates/kiln-plugin/tests/net_deadline.rs:72:5: assertion failed: host.fetch(request(Some(5000))).is_ok()

It passed on the same run's Linux and Windows jobs and on the runs 42503331207, 42495308040, 42492863021 and 42485508519
(one failure in five runs), and passes here. The test took 0.121 s on the failing run.

## Cause: a timing flake, not a bug

The test arms `program_runs` with 50 ms and fetches through a fake backend that sleeps 0. `Net::fetch_within`
(kiln/crates/kiln-plugin/src/net.rs ~138-165) sets `deadline = now + min(share, timeout)` and returns "timed out" if
`Instant::now() > deadline` after `backend.send`. If the shared macOS runner deschedules the thread for more than 50 ms
between those two points, a correct fetch is refused. The code is behaving as designed.

## Fix (test only)

In kiln/crates/kiln-plugin/tests/net_deadline.rs, arm the first host with a share that is large compared to a stall (for example
2 s) but still well below the 5000 ms request timeout, and assert the sent timeout is <= that share (and > 0). That keeps the
"capped by the share, not the request" check without a 50 ms margin. Leave the second half (unarmed host) unchanged.

## Implementation notes

The first host now arms a 2 s share (request timeout stays 5000 ms); the test asserts the sent timeout is > 0 and <= the share.

Repetition check for C2, run from the worktree: `for i in $(seq 50); do cargo test -q -p kiln-plugin --test net_deadline a_fetch_is_capped_by_what_is_left_of_the_program_share; done` counting `1 passed; 0 failed` lines gave 50/50 passes. The whole `net_deadline` binary (2 tests) also passed 50/50 consecutive runs.
