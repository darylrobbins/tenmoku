---
task_uid: "01m57yy3ab8hv1wsx01ccdr7ej"
task: "T996"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Neither test has an absolute wall-clock cap that load can cross; each still bounds a huge gR count against R or by a small fixed cost"
    required: true
    manual: false
  - id: "C2"
    text: "Each test passes 20 runs in a row beside a concurrent cargo build"
    required: true
    manual: false
---

# Flake: fe-vim a_huge_virtual_count_stays_bounded and huge_virtual_counts_stay_bounded time out under load (absolute 90/120 s caps)

## Problem
Two fe-vim tests that bound a huge `gR` count fail whenever the machine is loaded. They take 27-33 s at load ~30, so under load ~150 they cross their absolute caps:
- crates/fe-vim/tests/virtual_replace_oracle.rs `a_huge_virtual_count_stays_bounded` (93.5 s against the 90 s cap at :73);
- crates/fe-vim/tests/virtual_replace_breaks_oracle.rs `huge_virtual_counts_stay_bounded` (the 120 s cap at :61).

Both failed in Mac host stack 28's gate (load ~150, .worktrees/LANDING-stack-28.md) and passed on rerun at load ~30. They came from helium T156/T157.
## Change
Test-only. Keep what the tests prove, that a huge virtual count costs about as much as `R`'s copies and not many times more, but make the bound immune to load:
- measure against a same-process baseline taken next to it, as the oracle test already does with `plain`, and drop the absolute caps;
- or cut the count to one that still exercises the bound (say 2^24) so a run takes well under a second.

Prefer the smaller count if it still fails at base with the bug reintroduced. Prove the fix by running each test 20 times under load (e.g. beside a `cargo build`), as docs/lead.md's flake rule says. There is no regression test, because the fix is to the tests themselves.
