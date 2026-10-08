---
task_uid: "01m4cqy8zdshjcxw7ab0p6ae4n"
task: "T189"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "a_call_of_many_strings_is_read_once no longer asserts on absolute wall-clock time; it still fails when per-capture argument validation is reintroduced (shown in the evidence, not committed)"
    required: true
    manual: false
  - id: "C2"
    text: "The test passes 30 of 30 runs under parallel load; the command and pass count are in the evidence"
    required: true
    manual: false
  - id: "C3"
    text: "No change to imports.rs outside #[cfg(test)] items; no other test changes"
    required: true
    manual: false
---

# Make the flaky fe-syntax test imports::tests::a_call_of_many_strings_is_read_once deterministic under load

# Make the flaky fe-syntax test imports::tests::a_call_of_many_strings_is_read_once deterministic under load

`crates/fe-syntax/src/imports/tests.rs` `a_call_of_many_strings_is_read_once` guards against quadratic
work on a `require(...)` call with 20,000 string arguments by asserting the three languages finish in
under 5 s of wall-clock time. Under gate load it took 6.48 s and failed main's fast group on 263d9c6
(run 01m4cpvhggmvg7zj1s92zya941, log ~/.local/state/regatta/runs/01m4cpvhggmvg7zj1s92zya941/test.fastedit.log);
the rerun passed. Earlier runs took 0.4–4.0 s, so the 5 s budget sits inside normal variance under load.
Nothing in the failing range touched fe-syntax.

## Do

Keep what the test protects (each `arguments` node is validated once, not once per string capture:
T168's fix and T181's F7) but stop it depending on the machine's speed. Prefer a deterministic
measure over a wall-clock one, for example:

- count the work: a `#[cfg(test)]` counter of argument-node validations (or query captures processed)
  that the test reads and asserts is linear in the argument count (e.g. at most a small constant times
  20,000, or equal for 20,000 and the per-node count), or
- compare scaling: time N and 4N arguments in the same process and assert the ratio is well under
  quadratic (e.g. < 8x), which load affects both sides of alike.

Whichever you pick, it must still fail if validation becomes quadratic again: show that by temporarily
reintroducing per-capture validation and seeing the test fail (say so in the evidence; don't commit it).

## Proof

This changes only the test (and at most a `#[cfg(test)]` counter), so there's no regression test that
fails at base on demand. Instead run it 30 times under load (e.g. alongside `cargo nextest run -p fe-syntax`
in parallel, or with `stress`-style background builds) and put the command and the pass count in the evidence.

## Out of scope

Any change to how imports are found, beyond a `#[cfg(test)]` counter.
