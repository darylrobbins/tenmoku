---
task_uid: "01m6v1s021j030stb84mc486wj"
task: "T511"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-plugin's process test nothing_outlives_a_call_that_ends still proves neither call waits for the holder's background program or for the ignored sleep: the sleeps it guards against last far longer than its elapsed bound, and the bound leaves several seconds of headroom over the nominal ~600 ms"
    required: true
    manual: false
  - id: "C2"
    text: "the test passes 20 runs in a row under host load 30+, with the slowest time and the load recorded in the Implementation notes"
    required: true
    manual: false
  - id: "C3"
    text: "only kiln/crates/kiln-plugin/tests/process.rs changes, and cargo clippy -p kiln-plugin --all-targets -- -D warnings, cargo nextest run -p kiln-plugin and scripts/check-no-gpui.sh pass"
    required: true
    manual: false
---

# Give kiln-plugin's nothing_outlives_a_call_that_ends headroom under load (flake)

Load flake seen in T505's finish gate (run 01m6tyg39pbm2veh8mq1n9wnyc, 2026-11-07, load ~40): `kiln-plugin::process nothing_outlives_a_call_that_ends` panicked at `kiln/crates/kiln-plugin/tests/process.rs:407` with "2.178032158s". It passed alone in 1.86s at load ~37, so the bound has almost no headroom on helium.

# Give kiln-plugin's nothing_outlives_a_call_that_ends headroom under load

## Problem
The test spawns two programs under a 300 ms deadline: `holder`, a script that leaves `sleep 3 &` holding stdout and stderr, and `sleep 5` with 1 MiB of unread stdin. It asserts both time out and that the two calls together took under 2 s (`started.elapsed() < Duration::from_secs(2)`). The nominal time is about 600 ms. Under gate load the kill, reap and pipe teardown stretch it past 2 s.

The bound can't just be loosened. It is what proves a call doesn't wait for the holder's background `sleep 3` or for `sleep 5`. A bound of 3 s or more would pass even if the call waited for the holder.

## Change
Test only, in `kiln/crates/kiln-plugin/tests/process.rs`:
- Lengthen the sleeps the bound guards against, for example the holder's background `sleep 30 &` and `sleep 30` in place of `sleep 5`. A call that waited for either would then take 30 s.
- Loosen the elapsed bound to about 10 s, well below the sleeps, with a comment saying why the gap is there.
- Keep every other assertion, including the thread-count check after the calls, as it is.
- Make sure the lengthened sleeps don't outlive the test: nothing may be left running when it ends, as the test's name says. If the existing kill path already guarantees that, say so in a comment.

No production code changes. The kiln/ tree has a boundary check (scripts/check-no-gpui.sh, the no_gpui gate stage): comments and identifiers under kiln/ must not name fe-* crates or fastedit.

## Tests
This is a test-only fix, so it can't fail at base on demand. Prove it with repeated runs under load instead: run `cargo nextest run -p kiln-plugin --test process nothing_outlives_a_call_that_ends` 20 times while the host is busy (load 30+, or alongside a workspace build), and record the slowest elapsed time and the load in the Implementation notes.

## Not in scope
Other timing tests in kiln-plugin, and any change to the spawn or deadline code.

## Implementation notes

- Sleeps lengthened to 30 s (holder's background `sleep 30 &`, and `sleep 30`); the elapsed bound is now 10 s. The `sleep 30` with stdin is the call's own child, ended by the call's kill. The holder's background sleep is not, so the holder writes its pid to a file and a `Reaper` guard kills it when the test ends (also on assertion failure).
- 20 consecutive runs, all passed, at host load 40-63 (load average 63.5 at the end). Slowest was 4.512 s for the whole nextest test (including startup and the success-path call), against 1.8-2.0 s typical. No `sleep 30` was left running afterwards.
- clippy -D warnings, `cargo nextest run -p kiln-plugin` (147 passed) and scripts/check-no-gpui.sh pass.
