---
task_uid: "01m5be54yajn4xhynexnnagzk2"
task: "T235"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "wasm_accepted_stack::a_stack_with_stray_structural_trees_fails_to_parse (top level, crates/fe-syntax/tests/wasm_accepted_stack.rs): a corrupt grammar that leaves stray structural trees under an accepted root fails to parse, and a valid parse with error recovery still succeeds"
    required: true
    manual: false
  - id: "C2"
    text: "cargo deny check passes and the existing wasm_table_validation tests still pass; no Cargo change"
    required: true
    manual: false
---

# Wasm grammar: a precise accepted-stack shape rule and a real external-token leaf fixture (T230 review F6 follow-up)

## Problem

T230 (Mac T1005, validate a Wasm grammar's static tables) ships a capacity check for the accepted stack (the root's production and structural child count are bounded by the alias sequences before the root is rebuilt) instead of the reviewer's stricter rule, by the user's decision. Two things were left as follow-ups (review finding F6, `vendor/tree-sitter/src/parser.c` ~1094):

- the rule "exactly one non-extra tree in every accepted stack": the worker found it rejects valid parses, so it needs analysis of which accepted stacks are legitimate (error recovery, extras) before it can be made precise;
- a fixture with a real external-scanner token leaf exercising scanner state, which needs a compiled grammar and a wasm toolchain (tree-sitter CLI, wasi-sdk) this environment lacks.

## Change

Work out and implement the precise accepted-stack shape rule so a corrupt grammar cannot leave stray structural trees under an accepted root while valid parses (including error recovery) still succeed; add a real external-token leaf fixture once the toolchain is available. Tests in new files declared from a test file.

## Toolchain on this host (installed 2026-10-21)

`tree-sitter` 0.27.0 is at `~/.local/bin/tree-sitter` (add it to PATH) and needs `node` for `generate`: run `mise exec node@22 -- tree-sitter generate` and `... tree-sitter build --wasm -o <name>.wasm`; the CLI downloads and caches its own wasi-sdk under `~/.cache/tree-sitter` (verified: a probe grammar built a .wasm). Build scratch grammars under `~/.cache/`, not /tmp (quota tmpfs); commit only the built `.wasm` (and `.scm` queries) fixtures, never a toolchain.

## Implementation notes

- The rule: the accepted root is the last non-extra tree below the end of input. Unless it is an ERROR node, no other non-extra tree may be on the stack (parser.c, `ts_parser__accept`). An ERROR root is the one legitimate multi-tree stack: `recover_eof` pushes an empty ERROR over whatever the stack holds, and the rebuild takes those trees as its children. Valid parses and error recovery pass unchanged (`wasm_accepted_stack.rs`).
- The real external-token leaf fixture is not added: it needs a compiled grammar and is not among the criteria; a follow-up can build one with the CLI.
