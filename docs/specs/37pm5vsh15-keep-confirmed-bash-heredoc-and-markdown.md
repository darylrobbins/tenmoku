---
task_uid: "01m42e2xzavs160f37pm5vsh15"
task: "T50"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-syntax/tests/upstream_mismatches.rs has one #[ignore]d test per Bash input in fuzz/regressions/syntax_edits/bash-* (6) and one per distinct Markdown symptom in the T12 decision record (including markdown-inline-strikethrough-incremental), each decoded into plain Rust and asserting incremental == fresh parse (tree for Bash, highlights for Markdown); each ignore reason names the grammar and points to docs/decisions/khgdxezget-fuzzing.md"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-syntax passes (the new tests are ignored), and cargo test -p fe-syntax --test upstream_mismatches -- --ignored fails on every test at this commit, showing each still reproduces"
    required: true
    manual: false
---

# Keep confirmed Bash heredoc and Markdown inline fuzz mismatches as ignored upstream regression tests

Decided by the user, 2026-10-03, from T12's confirmed fuzz findings (docs/decisions/khgdxezget-fuzzing.md):
Bash heredoc (6 inputs) and Markdown inline (49 inputs) incremental-vs-full mismatches are upstream grammar
bugs. fastedit posts the drafted upstream issues (the user posts them) and keeps the repros next to the code
as `#[ignore]`d regression tests. No vendored patch, no change to how Markdown's inline layers are parsed.
Java, Python, C++ and JavaScript findings are upstream-only: no tests here.

## Work

A new test file, `crates/fe-syntax/tests/upstream_mismatches.rs`, with one test per case, each:
- built from a fuzz/regressions/syntax_edits input decoded by hand into plain Rust (text, then the edits),
  so the test doesn't depend on the fuzz harness or `arbitrary`'s encoding;
- asserting what the fuzz harness asserts: after the edits, an incremental `Syntax` gives the same tree
  (Bash) or highlights (Markdown) as a fresh parse of the final text;
- `#[ignore = "upstream: <grammar> — <one-line symptom>; see docs/decisions/khgdxezget-fuzzing.md"]`.

Cover all 6 Bash inputs and a representative set of Markdown inputs: one per distinct symptom in the
decision record's mapping table (at most ~8), including `markdown-inline-strikethrough-incremental`.
A file-level doc comment says how to run them (`cargo test -p fe-syntax --test upstream_mismatches -- --ignored`)
and that a test starting to pass means upstream fixed it: un-ignore it.

Don't touch fuzz/README.md (T47) or the decision record (T48).
