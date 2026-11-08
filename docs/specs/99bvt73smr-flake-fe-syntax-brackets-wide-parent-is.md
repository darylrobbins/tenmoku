---
task_uid: "01m6tp1srdbgr6k199bvt73smr"
task: "T1498"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In crates/fe-syntax/tests/brackets.rs, wide_parent_is_linear no longer asserts first_time * 2 \u003c parse_time; it still checks that the first lookup is linear in the child count (by comparing two sizes, or at most a loosened ratio against parse time) and keeps the pair_time * 20 \u003c parse_time bound"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test brackets --retries 0 passes 20 times in a row while another cargo build runs on the machine"
    required: true
    manual: false
  - id: "C3"
    text: "cargo clippy -p fe-syntax --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-syntax brackets wide_parent_is_linear fails under load on the first-lookup 2x bound (first_time*2 < parse_time, seen 0.59x and 0.68x)

`fe-syntax` test `brackets::wide_parent_is_linear` (crates/fe-syntax/tests/brackets.rs) fails under machine load on its first-lookup bound `first_time * 2 < parse_time`. It compares two wall-clock times: the first `pair` lookup after a parse (one linear walk over a 200k-child JSON array) must take under half the parse time, best of 5 runs. Both steps are linear, so CPU contention moves their ratio. Seen on main post-merge checks on 2026-11-07: 0.68x (run 01m6tane3h7kwg1k05nznhjt1g, 5c7029b5) and 0.59x at load ~70 (run 01m6tmkc6zgjq25w6s8yrkpsys, c614e31b: "pairing took 217.095084ms then 503.375µs, parsing 368.967917ms").

T570 made it best-of-5 (6daa7bd2); T355 (a95148fb) later added the tight 2x first-lookup bound, which is the one that flakes.

Fix: keep the test's intent (the first lookup is linear in the child count, and later lookups are cheap) but stop comparing the first lookup with parse time at a tight ratio. Prefer showing linearity by size: time the first lookup at two sizes (e.g. 100k and 200k children) and require the larger to be under ~4x the smaller. Or, if that is not robust either, loosen to `first_time < parse_time`. Keep the `pair_time * 20 < parse_time` bound, which passes with a wide margin. Test-only change; no production code.
