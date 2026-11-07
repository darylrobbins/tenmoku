---
task_uid: "01m6sfvwjs597s7yr8nsd84v69"
task: "T1426"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "scripts/check-release-version.sh v0.1.1 exits 0"
    required: true
    manual: false
  - id: "C2"
    text: "git diff of Cargo.lock changes only workspace crates version lines from 0.1.0 to 0.1.1"
    required: true
    manual: false
---

# Bump the workspace version to 0.1.1 for the v0.1.1 release

Bump the workspace version to 0.1.1 for the v0.1.1 release (user-approved 2026-11-06: tag v0.1.1 before the Windows port PR lands).

Changes, nothing else:
- Cargo.toml `[workspace.package]` `version = "0.1.0"` -> `"0.1.1"`.
- Cargo.lock: refresh with `cargo check --workspace` (docs/release/macos-signing.md, "Releasing"). Only the workspace crates' version lines should change; vendored tree-sitter crates keep their own versions.
- README.md Install step 1: `Tenmoku-0.1.0-macos-arm64.zip` -> `Tenmoku-0.1.1-macos-arm64.zip`. Leave the "0.1 build is ad-hoc signed" wording.

No code, tests or settings change. The lead tags the landed commit `v0.1.1` and pushes the tag; the release workflow does the rest.
