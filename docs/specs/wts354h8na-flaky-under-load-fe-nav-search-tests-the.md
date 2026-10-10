---
task_uid: "01m4k9na78w752cywts354h8na"
task: "T328"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The hand-in confirms from the code that the failure (gate.started() == 0 at search.rs:1294 in runs 01m4k5x4h5, 01m4ehwzkk, 01m4c5bz0z) is the 300 ms budget running out before Fallback::run reaches the extraction, and that production honours the deadline either way"
    required: true
    manual: false
  - id: "C2"
    text: "the_deadline_doesnt_wait_for_an_extraction retries with a fresh Gate and a doubled budget (300 ms up to a 10 s cap) when gate.started() is 0, and on the attempt where gate.started() == 1 still asserts that defs.items is empty and defs.partial is set, then releases the gate; if no attempt starts the extraction, it fails with a message saying so"
    required: true
    manual: false
  - id: "C3"
    text: "No production code changes: only the tests module of crates/fe-nav/src/search.rs changes (apart from Regatta's own spec file), and .config/nextest.toml is untouched"
    required: true
    manual: false
  - id: "C4"
    text: "With the 1-min load average at least 2x hw.ncpu (ambient load counts; any busy loops added only below that, self-terminating, in their own process group), about 10 consecutive runs of cargo nextest run -p fe-nav -E 'test(=search::tests::the_deadline_doesnt_wait_for_an_extraction)' all pass, with the load averages and results in the decision file"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-nav passes"
    required: true
    manual: false
---

# Flaky under load: fe-nav search::tests::the_deadline_doesnt_wait_for_an_extraction: retry with a doubled budget when the deadline beats the walk

# Flaky under load: fe-nav search::tests::the_deadline_doesnt_wait_for_an_extraction

## Problem

`crates/fe-nav/src/search.rs` `search::tests::the_deadline_doesnt_wait_for_an_extraction` fails in gates
under heavy machine load (load average about 150-260 on this 14-core Mac) and passes alone:

- run `01m4k5x4h5sddmkfd8hwsqr8ch`: FAIL after 0.396 s, panicked at `search.rs:1294:9`;
- run `01m4ehwzkkkse6zbr7v8wkw4vx`: FAIL after 0.380 s, panicked at `search.rs:1289:9`;
- run `01m4c5bz0zmmdnbgc0b0akeqm4`: FAIL after 0.508 s, panicked at `search.rs:3093:9` (older layout).

Each time it's the same assertion, `assert_eq!(gate.started(), 1)` with `left: 0, right: 1`. (Logs:
`~/.local/state/regatta/runs/<run>/test.fastedit.log`.) These are the only failures of it in the ~1,660
runs under `~/.local/state/regatta/runs/`. It's a different cause from T326 (fe-nav's lossy parse-cap
flake in `tests/extract.rs`).

Cause: the test's timing assumption, not production. The test gives the request a 300 ms budget and an
extraction that blocks on a `Gate` until released, then asserts that the extraction had started by the time
the answer came. But before `Fallback::run` (`search.rs`, `impl Fallback`) reaches the extraction, the
request has to get to `SearchNavigation::fallback`, which returns at once if the deadline has passed, then
canonicalize the root, spawn the `fe-nav fallback` thread, and walk the project with `search`; and
`Fallback::run` gives up without extracting when `Instant::now() >= self.deadline`. Under load all of that
can take more than 300 ms, so the request answers at its deadline (correctly empty and partial) without the
extraction ever starting, and the `started() == 1` assertion fails. Production does what it should: the
deadline is honoured either way.

## Change

Test-only, in `crates/fe-nav/src/search.rs`'s `tests` module. No production code changes.

Make `the_deadline_doesnt_wait_for_an_extraction` retry on a deadline that beat the walk: run the request
with a fresh `Gate` and a budget starting at 300 ms; if `gate.started()` is 0 after the answer, the
deadline came before the extraction did, which proves nothing, so release that gate and try again with
double the budget, up to a cap of 10 s. Once an attempt has `gate.started() == 1`, assert, as now, that
`defs.items` is empty and `defs.partial` is set, then release the gate. If even the 10 s attempt didn't
start the extraction, fail with a message saying so. Since the gated extraction blocks until released,
an answer coming back while `started() == 1` is the proof that the deadline doesn't wait for it.

Keep `callers(2)`, `unbuilt`, `with_extract(gate.extract())` and the existing assertions; don't change
`Gate`, other tests, or `.config/nextest.toml` (no retries override).

Prove it with repeated runs under load: about 10 runs of the test while the 1-minute load average is at
least 2x `hw.ncpu` (ambient load counts; any load generator added must stop on its own, e.g. busy loops
under `timeout`, in their own process group, killed by the script when it ends). Record the load average
and results in the decision file.

## Implementation notes

- C1: `SearchNavigation::fallback` returns at once if `Instant::now() >= deadline` (search.rs:245), `wait` stops at the deadline (:346), and `Fallback::run` gives up without extracting once the deadline passes (:428, :436). Under load the 300 ms budget can run out before the extraction starts, so `gate.started() == 0`; the answer is then correctly empty and partial. Production honours the deadline either way.
- C2: the test retries with a fresh `Gate` and a doubled budget (300 ms, 600 ms, … capped at 10 s) while `gate.started() == 0`; it asserts empty/partial on the attempt that started, and fails with "no extraction started within a budget of …" otherwise.
- C4: hw.ncpu = 14 (2x = 28). Ambient load only, no generator added. 10 consecutive runs of the test, all PASS (0.32–0.46 s). 1-min load averages at each run: 73.76, 67.97, 67.97, 67.97, 64.13, 64.13, 60.43, 60.43, 58.48, 57.56.
- C5: `cargo nextest run -p fe-nav`: 106 passed.
