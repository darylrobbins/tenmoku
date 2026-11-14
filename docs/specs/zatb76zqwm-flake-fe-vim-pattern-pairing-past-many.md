---
task_uid: "01m7ch5degsdrpzpzatb76zqwm"
task: "T360"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "pattern_pairing_past_many_other_tags_finishes_in_time no longer depends on wall-clock time under load, and still fails if pairing past many other tags falls back to a timeout with the default limit"
    required: true
    manual: false
  - id: "C2"
    text: "20 runs of the test alongside a parallel cargo build all pass, recorded in the decision file"
    required: true
    manual: false
---

# Flake: fe-vim pattern_pairing_past_many_other_tags_finishes_in_time times out under machine load (2.28 s vs the 2 s default limit) (Mac T1868)

`fe-vim::delimiters_regressions pattern_pairing_past_many_other_tags_finishes_in_time` failed in the Mac stack 170 macOS gate (run 54, 2026-11-14) at machine load 70-120: it took 2.28 s, the search hit the 2 s `DEFAULT_TIME_LIMIT` and the register came back empty. Five reruns at load ~27 passed in 0.14-0.21 s. The stack touched no fe-vim file.

The test exists to prove the pairing search finishes inside the *default* limit, so the T1719-style `unhurried()` fix (a longer limit) defeats it. Make it robust under load without losing its point, for example: assert the work done (a step or node count the search reports, under a bound) rather than wall time, or measure the default-limit path with a deadline clock the test controls. Keep the test's claim: past many other tags, pairing completes with the default limit and does not degrade to a timeout.

Test-only change unless the fix needs a small test seam in crates/fe-vim/src/search.rs. Prove it with repeated runs under load (e.g. 20 runs alongside a parallel build), recorded in the decision file.

## Implementation notes

The test no longer reads the clock: it runs with no wall-clock limit (`Duration::MAX`) and a deterministic budget of 1,000 regex calls (`set_call_limit`), and asserts the search used fewer. Pairing past 20,000 `<b>` takes 5 calls; the quadratic fallback takes 20,000+, which exhausts the budget and fails the object. With a budget of 3 the test fails, so the budget does bite. 20 runs with a parallel `cargo build --workspace` going (load average ~12.8): 20/20 passed.
