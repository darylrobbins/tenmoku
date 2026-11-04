---
task_uid: "01m6m9ytmehbssqb6kz0qc2fpg"
task: "T449"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T262

Findings of the review of T262 that don't block the merge (below merge.block_severity):

- F1 medium kiln/crates/kiln-plugin/src/manifest.rs:168: The path check rejects every ParentDir component, including valid paths such as components/../p.wasm that stay inside the extension. This differs from the loader's containment rule, which permits internal parent traversal while rejecting traversal above the extension root and symlink escapes.
  fix: Use the loader's depth-based lexical containment check, followed by the existing canonical containment and file checks. Add tests for valid internal parent traversal and escaping symlinks.
  instances: kiln/crates/kiln-plugin/src/manifest.rs:168
