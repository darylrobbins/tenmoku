---
task_uid: "01m425ntccbjbzhn9t0afhhq8c"
task: "T48"
rev: 1
type: "docs"
complexity: "xs"
criteria:
  - id: "C1"
    text: "docs/decisions/khgdxezget-fuzzing.md: line ~27 says runs before 13f6e28 used registry tree-sitter-yaml 0.7.2; lines ~51–52 tie the unbudgeted / 4 KB rate explanation to the f2ec93a final run; line ~220 says every Markdown input was minimized (see the mapping table) and one was analysed in depth for the upstream draft"
    required: true
    manual: false
---

# Follow-ups from review of T12

Review F8 (low) on T12 (review wmfqtz), not filed automatically: some prose in docs/decisions/khgdxezget-fuzzing.md wasn't updated when the final runs changed.

- Line 27: say that runs before 13f6e28 used registry 0.7.2.
- Lines 51–52: tie the unbudgeted/4 KB rate explanation to the f2ec93a final run.
- Line 220: say that every Markdown input was minimized (see the mapping table) and that one was analysed in depth for the upstream draft.

Docs only; no code.
