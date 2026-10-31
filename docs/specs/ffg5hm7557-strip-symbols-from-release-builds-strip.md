---
task_uid: "01m69cn67rsckxnrffg5hm7557"
task: "T1256"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "[profile.release] sets strip = \"symbols\" with a comment; [profile.bench] sets strip = \"none\""
    required: true
    manual: false
  - id: "C2"
    text: "TESTING.md says release binaries carry no symbols and to profile with the bench profile"
    required: true
    manual: false
  - id: "C3"
    text: "cargo build --workspace --all-targets and cargo clippy --workspace --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Strip symbols from release builds (strip = "symbols", ~14 MB smaller), keeping them in the bench profile

## Problem

The release binary is 101.6 MB (main 4b965db). About 15 MB of it is the Mach-O symbol table (__LINKEDIT,
~100k symbols): `[profile.release]` sets only `lto = "thin"` and `codegen-units = 1`, and with no debug info
Cargo's default strips debuginfo but keeps symbols. The rest is real content (26 static tree-sitter
grammars ~35 MB, Rust code ~40 MB, unwind tables ~6 MB) and is out of scope.

## Change

- `[profile.release]`: add `strip = "symbols"` with a one-line comment (~14 MB smaller; the user asked).
- `[profile.bench]` inherits release: set `strip = "none"` there so `cargo bench` binaries keep symbols
  for profilers (samply, Instruments).
- Note it in TESTING.md (or wherever release builds are described) in one line: release binaries carry no
  symbols; profile with the bench profile.

Nothing else changes: no panic strategy, opt-level or grammar changes.
