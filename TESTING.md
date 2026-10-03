# Testing the core

The core crates (`fe-text`, `fe-buffer`, `fe-display`, `fe-editor`) and the
editor view (`fe-gpui`) are tested in layers. Each layer catches things the others can't.

| Layer | Where | What it proves |
|---|---|---|
| Unit tests | `src/tests/mod.rs` in each crate | Specific behaviour, written in before/after form (`fe-editor` uses `\|` for carets and `[`…`]` for selections) |
| String differential | `fe-text/src/tests/large.rs`, `fe-buffer` `random_edits_match_string_model` | Every query and edit matches a plain `String`. The `fe-text` texts reach 400 KB so the rope's tree, chunk boundaries and line indexes are exercised |
| Reference model | `fe-buffer/src/tests/ideal.rs` | Undo/redo across user and AI origins matches a slow, obviously correct model (characters are never removed, only hidden). Inserted characters are unique, so nothing can match by coincidence |
| Invariants | `fe-buffer/src/tests/invariants.rs`, `fe-editor/src/tests/laws.rs` | Laws that hold for any input: anchors come home after undo, events never skip a version, movement round-trips, multi-cursor typing matches a string, random command streams keep the model well formed |
| Key bindings | `fe-gpui/src/tests.rs` | Real keystrokes go through the real keymap into a focused editor in a headless GPUI window. One table row per binding, plus typing, input-method composition, clipboard and save. `bindings_are_all_tested` fails if a binding has no row |

Mutation testing (`cargo mutants -p <crate>`) is worth running by hand before a
milestone, to find code the tests don't actually check. It isn't part of CI.
Each crate takes a few minutes with `-j 6`. The known survivors are equivalent
mutants (bookkeeping that only saves work, guards a later check repeats),
trivial accessors, and `fe-gpui`'s paint geometry. Some `fe-editor` mutants
are only caught by `fe-gpui`'s tests; add `--test-workspace true` to count them.

## Running

```zsh
cargo test --workspace            # what CI runs on every push
scripts/soak.sh                   # 50k cases per property, release (a few minutes);
                                  # run after changing undo/redo or anchor code
cargo mutants -p fe-buffer -j 4   # by hand, before milestones
cargo llvm-cov -p fe-buffer --summary-only
```

`PROPTEST_CASES` sets the case count for every property test, except the
large-text rope tests, which run a hundredth as many (at least 8). Note that
`proptest!` lets the variable override `ProptestConfig::with_cases`, so a test
that needs its own count has to drive `TestRunner` itself, as `large.rs` does.
CI's `soak` workflow runs `scripts/soak.sh` on demand.

## When a property test fails

The reference-model tests shrink a failing session to a few operations and
print them. Add that session to the `regressions` module in
`fe-buffer/src/tests/ideal.rs`, where `trace` prints it step by step, and fix
it. proptest also saves failing seeds under `proptest-regressions/`; commit
those.

## Known limitation: refused undos

Undo and redo map positions through later edits using offsets. Offsets can't
always say in what order text belongs when several undos and redos put text
back at the same spot, or inside text a later edit replaced and then reverted.
In those cases the buffer refuses with `UndoError::Conflict` rather than guess.
A refusal never changes the text.

`avoidable_refusals_stay_rare` tracks how often a refused transaction's text
was actually intact. It is 1.8% of attempts in its adversarial sessions, with
a budget of 2.2%. Plain undo and redo of a single origin are exact and never
refused. Removing the limitation would mean giving every character an identity,
as collaborative editors do.
