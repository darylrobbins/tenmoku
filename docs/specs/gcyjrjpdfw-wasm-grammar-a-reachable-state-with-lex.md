---
task_uid: "01m5baqbq9wxprkagcyjrjpdfw"
task: "T234"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "wasm_empty_eof_state::a_state_with_no_lex_state_and_an_empty_eof_entry_is_an_error (top level, crates/fe-syntax/tests/wasm_empty_eof_state.rs): loading such a grammar returns an error and never dereferences NULL"
    required: true
    manual: false
  - id: "C2"
    text: "cargo deny check passes and the existing wasm_table_validation tests still pass; no Cargo change"
    required: true
    manual: false
---

# Wasm grammar: a reachable state with lex_state UINT16_MAX and an empty EOF entry is an error, not a NULL dereference (T230 review F7)

## Problem

T230 (Mac T1005, validate a Wasm grammar's static tables) left one medium review finding as a documented limit (F7, `vendor/tree-sitter/src/parser.c` ~1680): the null-lookahead guard runs only when an action exists. A reachable state with `lex_state` UINT16_MAX and an EMPTY EOF entry passes table validation, skips the guard loop, and dereferences NULL through `ts_subtree_is_keyword` (~1788). It is a pre-existing sibling of the repaired accept path.

## Change

Extend the guard so a null lookahead is handled when the EOF entry is empty too (or reject such a state during table validation), with a hostile-grammar fixture in a new test file that loads such a grammar and gets a `GrammarError`/parse error instead of a NULL dereference.
