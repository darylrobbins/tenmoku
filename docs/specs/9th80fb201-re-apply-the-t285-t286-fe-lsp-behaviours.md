---
task_uid: "01m7d6wepyfbqjhg9th80fb201"
task: "T385"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The three restored tests pass: hints_at_the_end_of_the_range_are_dropped, no_provider_before_initialization_is_none_not_retry, a_delta_is_not_asked_across_two_pending_size_crossings (cargo nextest run -p fe-lsp)"
    required: true
    manual: false
---

# Re-apply the T285/T286 fe-lsp behaviours on the A5 shape: inlay hint range end, no semantic-token provider before initialization is Ok(None), no delta across two pending size crossings

# Re-apply the T285/T286 fe-lsp behaviours on the A5 shape

The option-2 sync took the Mac's A5 routing in `crates/fe-lsp` and deleted three argon-only tests
that pinned behaviours from the argon review follow-ups T285 (inlay hints) and T286 (semantic
tokens). Re-apply those behaviours on the A5 shape and restore the three tests:

- Inlay hints: a hint at the end of the requested range is dropped
  (`hints_at_the_end_of_the_range_are_dropped`, `crates/fe-lsp/src/tests/hover/inlay_hints.rs`).
- Semantic tokens: no provider before initialization is `Ok(None)`, not a retry
  (`no_provider_before_initialization_is_none_not_retry`, `crates/fe-lsp/src/tests/encoding/semantic_tokens.rs`).
- Semantic tokens: no delta is asked across two pending size crossings
  (`a_delta_is_not_asked_across_two_pending_size_crossings`, same file).

The old fixes: `git show 2a717d6f 3616f9a5`. The deleted tests: `git show 2a717d6f 3616f9a5 -- crates/fe-lsp/src/tests`.
When it is done, batch it with a note; the Mac lead mints its Mac task id at landing.
