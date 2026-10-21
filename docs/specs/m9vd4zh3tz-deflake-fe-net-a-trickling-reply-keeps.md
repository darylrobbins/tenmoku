---
task_uid: "01m5ea6fvdp0hhhym9vd4zh3tz"
task: "T1019"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it bounds its wait by a named server silence constant, well over the expected ~1.3 s, and still fails if the idle timeout never fires"
    required: true
    manual: false
  - id: "C2"
    text: "the test passes 20 of 20 runs under parallel build load; the slowest time is recorded in the hand-in; cargo nextest run -p fe-net and clippy pass"
    required: true
    manual: false
---

# Deflake fe-net a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it: bound its wait by the server's silence, not 2 s

## Problem

`crates/fe-net/tests/client.rs` `a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it` fails under load: it asserts the whole exchange took under 2 s, but its expected time is ~1.26 s (eight pieces 120 ms apart, then a 300 ms idle timeout), so only ~0.7 s of slack absorbs scheduler overrun. On the Mac at loadavg 18-27 it took 2.2-2.4 s in 5 of 5 runs (T1013's finish gate, 2026-10-21), while passing alone in 1.8 s. It blocked T1013.

## Change

Test only; production code is unchanged. The 2 s bound exists to prove the idle timeout fired rather than the reply waiting out the server's silence. Measure it against that silence instead of a tight absolute:

- The server's silence after the last piece becomes a named constant (e.g. `SILENCE = Duration::from_secs(10)`), used by the server's `thread::sleep`.
- The assertion becomes `waited < SILENCE / 2` (or an equivalent bound well under the silence and well over the expected ~1.3 s), with the message naming both.
- Leave the 120 ms piece spacing and the 300 ms idle timeout as they are, unless repeated load runs show a piece arriving late enough to trip the timeout early; then widen the timeout to 500 ms and say so.

Prove it with repeated runs under load, not a regression test (a test-only fix can't fail at base on demand): run the test 20 times while `cargo build --workspace` or the gate of another lane runs, and record the slowest time in the hand-in.

## Implementation notes

Added `SILENCE` (10 s) used by the server's sleep; the assertion is now `waited < SILENCE / 2`. Piece spacing (120 ms) and idle timeout (300 ms) unchanged. Ran the test 20 times under a concurrent `cargo build --workspace` at loadavg ~27: 20/20 passed, slowest 2.462 s (nextest-reported, includes process start), which would have failed the old 2 s bound.
