---
task_uid: "01m41br10jy0h2zabyxtv354bf"
task: "T32"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1

Findings of the review of T1 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/syntax.rs:423: The new caller-supplied predicate runs through tree-sitter 0.27's extern "C" progress trampoline. If the predicate or injected driver clock panics during parsing, the process aborts before the outer catch_unwind can catch it. The existing panic test exercises only the separately protected read callback.
  fix: Catch predicate panics inside the progress callback, record the payload and return Break, then resume unwinding after parse_with_options returns so the existing parse failure handling catches it. Add regression coverage for both a panicking predicate and an injected clock that panics during a progress check.
  instances: crates/fe-syntax/src/syntax.rs:423

- F2 medium crates/fe-syntax/src/driver.rs:65: Switching buffers drops the task but retains the previous buffer's terminal failure state. After parsing or highlighting buffer A panics, sync(buffer B) calls Syntax::edit, which preserves failed=true; parse_until then reports completion without parsing B. Every subsequent buffer remains unhighlighted.
  fix: Reset buffer-specific syntax state when the buffer ID changes, for example by constructing a fresh Syntax with the existing language and new snapshot. Add a test that injects a failure for A, switches to B, and verifies B parses and highlights successfully.
  instances: crates/fe-syntax/src/driver.rs:65
