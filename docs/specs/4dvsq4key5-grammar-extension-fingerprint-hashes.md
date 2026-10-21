---
task_uid: "01m5arbkypy6fp6a4dvsq4key5"
task: "T232"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "extensions_fingerprint::a_same_length_mtime_preserving_rewrite_is_detected (top level, kiln/crates/kiln/tests/extensions_fingerprint.rs): rewriting a listed file with the same length and its mtime restored changes the extension's fingerprint"
    required: true
    manual: false
  - id: "C2"
    text: "cargo deny check passes and kiln's existing extension tests still pass; no Cargo change"
    required: true
    manual: false
---

# Grammar extension fingerprint hashes content off the UI thread, so a same-length mtime-preserving rewrite re-registers (T229 review F4)

## Problem

T229 re-registers a grammar extension when only its `.wasm` or `.scm` changes, detected by kiln's fingerprint of the listed files (`extension_files`: size and mtime). A same-length, mtime-preserving rewrite of a listed file is not detected (review finding F4).

## Change

Fingerprint the listed files by content (a hash), computed off the UI thread, so such a rewrite re-registers the grammar; keep the cheap size+mtime check as the fast path that skips hashing when nothing changed. New test in a new file declared from a test file: a rewrite of the same length with the mtime restored re-registers.
