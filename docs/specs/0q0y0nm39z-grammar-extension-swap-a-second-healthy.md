---
task_uid: "01m5arbysnbr2ttj0q0y0nm39z"
task: "T233"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "extension_grammar_swap_serves_the_new_highlights (top level, crates/fastedit/tests/extension_grammar_swap.rs): replacing a registered extension's .wasm with the second fixture, whose highlights differ, serves the new highlights without a restart"
    required: true
    manual: false
  - id: "C2"
    text: "cargo deny check passes and the existing extension_grammars tests still pass; no Cargo change"
    required: true
    manual: false
---

# Grammar extension swap: a second healthy grammar fixture with different highlights proves the swapped module's highlights are served (T229 review F3)

## Problem

T229 (Mac T1004, kiln re-registers a grammar extension when only its .wasm or .scm changes) proves that replacing only the `.wasm` re-registers the grammar and that it still highlights, but not that a module with DIFFERENT highlighting is served after the swap: the repo has one healthy grammar module (`tree-sitter-json.wasm`; the other fixtures are hostile) and building a second needs the tree-sitter CLI and wasi-sdk.

## Change

Add a second healthy grammar fixture whose highlights differ from the JSON one (built once with the tree-sitter CLI and wasi-sdk, committed with a note on how it was built, as small as possible), and a test that swaps the `.wasm` of a registered extension for it and sees the new highlights served without a restart. New test in a new file declared from a test file.

## Toolchain on this host (installed 2026-10-21)

`tree-sitter` 0.27.0 is at `~/.local/bin/tree-sitter` (add it to PATH) and needs `node` for `generate`: run `mise exec node@22 -- tree-sitter generate` and `... tree-sitter build --wasm -o <name>.wasm`; the CLI downloads and caches its own wasi-sdk under `~/.cache/tree-sitter` (verified: a probe grammar built a .wasm). Build scratch grammars under `~/.cache/`, not /tmp (quota tmpfs); commit only the built `.wasm` (and `.scm` queries) fixtures, never a toolchain.
