---
task_uid: "01m5fqy9bg1yrm1fr1abw9wc6a"
task: "T212"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "runtime_grammar's short_deadlines uses a 10 s load deadline shared with the loopinit test's elapsed assertion, and cargo nextest run -p fe-syntax --test runtime_grammar passes"
    required: true
    manual: false
---

# Stop fe-syntax runtime_grammar tests failing on a 2 s wasm load deadline when the machine is loaded

## Problem

`fe-syntax::runtime_grammar` tests flake under load with `Err(Deadline { file: ".../tree-sitter-json.wasm", deadline: 2s })`: `a_bad_export_is_an_error_not_an_abort` (argon's sync gate of 2026-10-21, passed on rerun) and `hostile_grammars_are_errors_not_crashes` (`lexnull: Err(Deadline {...})`, the next sync gate). Each failed one of 4667 tests while three lanes and a gate were building. The tests' own comment says the load deadline is meant to be "long enough for the JSON fixture's load (a few milliseconds) on a loaded machine"; it isn't: compiling the wasm module takes seconds when the machine is saturated.

## Fix

In `crates/fe-syntax/tests/runtime_grammar.rs` `short_deadlines()`, raise `GrammarDeadlines::load` from 2 s to 10 s. The one test that waits for the load deadline to fire (the `loopinit` test, which asserts `started.elapsed() >= Duration::from_secs(2)`) must assert against the same value: share one `LOAD_DEADLINE` const between `short_deadlines()` and that assertion. Keep `PARSE_DEADLINE` as it is. Change nothing outside this test file.
