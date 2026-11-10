---
task_uid: "01m728ap03nf4mgv2g0as8cah6"
task: "T320"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "crates/fe-net/tests/client.rs's a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it keeps its assertions (headers read, body hello world, then NetError::Timeout well inside SILENCE) with piece gaps at most a quarter of the idle timeout and the head plus chunk still longer than it"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-net --test client a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it passes 20 times in a row while other heavy builds or tests run (stress-ng or a parallel cargo build), recorded in the finish notes"
    required: true
    manual: false
---

# Flake: fe-net client a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it times out on the head under load (120 ms piece gap vs 300 ms idle timeout) (Mac T1717)

`fe-net::client a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it` (crates/fe-net/tests/client.rs:322) failed in main's post-merge fast check on f7ecfb8f0 (run 01m724wkvre8merqcwb09ec1sf, 2026-11-10 07:2xZ, Mac load ~150): `get(...)` at line 350 returned `Err(Timeout)` before the head arrived.

Cause: the test server sleeps 120 ms between the head's pieces and the client's idle timeout is 300 ms. Under heavy load a server thread can oversleep by more than 180 ms, so the idle timeout fires during the head, which is what the test says must not happen. The production code is not at fault; the test's margins are too tight for a loaded machine.

Fix the test only: keep what it proves (each piece arrives inside the idle timeout while the head and the chunk together take longer than it, then a silence trips it, bounded well under SILENCE), but with margins a loaded machine can meet, e.g. a larger idle timeout and piece gap with a ratio of at least 4x, and enough pieces that their sum still exceeds the timeout. Keep the test's run time small (a few seconds at most). Don't change crates/fe-net/src.
