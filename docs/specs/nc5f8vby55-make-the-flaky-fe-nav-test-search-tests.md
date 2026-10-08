---
task_uid: "01m4cd3znzb0b8nwnc5f8vby55"
task: "T185"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fallback_threads_are_bounded no longer depends on a wall-clock budget: each fallback is known to be blocked in its extraction (through the Gate) before the next request, so searches == FALLBACK_THREADS and gate.started() == FALLBACK_THREADS hold exactly, and after release the next request searches again"
    required: true
    manual: false
  - id: "C2"
    text: "Only the #[cfg(test)] tests module of crates/fe-nav/src/search.rs changes; production code in search.rs is unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "50 consecutive runs of cargo nextest run -p fe-nav -E 'test(=search::tests::fallback_threads_are_bounded)' pass while other cargo builds load the machine, with the load average and pass count recorded in the hand-in"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-nav passes"
    required: true
    manual: false
---

# Make the flaky fe-nav test search::tests::fallback_threads_are_bounded deterministic under load

## Symptom

`search::tests::fallback_threads_are_bounded` (crates/fe-nav/src/search.rs, in-module tests) fails under load. It panicked at search.rs:3129, `assert_eq!(stats.searches, FALLBACK_THREADS)`, in the post-merge run of commit ad13fec (run id 01m4byj823f7e2bf5ypn1688sr). A rerun of the same commit passed.

## Suspected cause (in the test, not production)

The test gives the navigation a 100 ms budget, then makes `FALLBACK_THREADS + 3` requests and expects exactly `FALLBACK_THREADS` fallbacks to have started, each one blocked in its first extraction on the `Gate` and holding its slot. That only holds if every request and every fallback thread beats a 100 ms wall-clock deadline. Under load it can go wrong two ways:

- **Too many searches.** A fallback thread that takes over 100 ms to spawn and walk the two-file tree finds `Instant::now() >= self.deadline` before its first file (`Fallback::run`). It stops without calling `extract`, so it never blocks on the gate, and it frees its slot. A later request then takes that slot again and `searches` goes past `FALLBACK_THREADS`.
- **Too few searches.** If a request has already used its 100 ms (same-file symbols, imports) before it reaches `fallback`, the early `Instant::now() >= deadline` return skips the fallback without taking a slot.

Both are by design: requests degrade under load instead of waiting. The slot bound itself (`Slots::take`, `fetch_update` with `busy < limit`) is race-free, so this is a test-only flake, not a production race.

## Fix direction

Make the test deterministic, with no wall-clock assumption about when a fallback reaches its extraction:

- Give the navigation a long budget (as `a_cancel_mid_request_doesnt_wait_for_an_extraction` does) so no deadline fires during the test.
- End each of the first `FALLBACK_THREADS` requests by cancelling it once the gate shows that request's extraction has started (`gate.started() == i + 1`, a counting variant of `cancel_when_started`), so each fallback is known to be blocked in `extract` and holding its slot.
- Then the extra requests find no free slot and return at once, and `searches == FALLBACK_THREADS` and `gate.started() == FALLBACK_THREADS` hold exactly.
- Keep the release, drain and "next request searches again" half. The 30 s bound on the drain loop is only a guard against hanging, which is fine.

Change only the tests module of search.rs. Production code stays as it is.
