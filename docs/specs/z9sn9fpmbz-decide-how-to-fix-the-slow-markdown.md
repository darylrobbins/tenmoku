---
task_uid: "01m41d7w568avtnnz9sn9fpmbz"
task: "T33"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/markdown-reparse.md records a decision among an upstream issue (drafted in the record, not posted), a grammar fork or patch (scope, licence and upkeep assessed, no dependency change) and accepting the cost, with the reason"
    required: true
    manual: false
  - id: "C2"
    text: "The record has fresh numbers from cargo bench -p fe-syntax -- \"markdown/reparse keystroke\" on current main (manual)"
    required: true
    manual: false
  - id: "C3"
    text: "PERF.md's Syntax section points to the record"
    required: true
    manual: false
---

# Decide how to fix the slow Markdown keystroke reparse: upstream issue, grammar fork, or accept it

Follow-up to T4. A Markdown keystroke reparse on 10k lines costs ~6 ms (target 5 ms); cost tracks the distance to the end of the file and is in tree-sitter's parser (reduce/summarize/reuse walk), not the md scanner or lexer. See PERF.md Syntax section.

Write a decision record, `docs/decisions/markdown-reparse.md`, in the style of the existing ones, that chooses between:

1. **An upstream issue** against tree-sitter (or tree-sitter-md, whichever owns the walk). Draft the issue in the record: a minimal reproduction, the measurements, and where the time goes. Don't post it; the user posts it.
2. **A grammar fork or patch.** Assess what it would change, its size, its licence (CLEAN_ROOM.md needs an audit) and the upkeep. Don't add or change a dependency: Cargo.toml and Cargo.lock changes come in a later task once the user picks this.
3. **Accepting ~6 ms** for now, with the reason.

Re-measure with `cargo bench -p fe-syntax -- "markdown/reparse keystroke"` on current main and put the numbers in the record. Add a one-line pointer to the record in PERF.md's Syntax section.
