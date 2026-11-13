---
task_uid: "01m79gttehbav2haz0ffm6dcmv"
task: "T601"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "python3 scripts/check_kiln_boundary.py reports no OS branch in kiln-core tests and scripts/check-no-gpui.sh passes"
    required: true
    manual: false
  - id: "C2"
    text: "a_file_from_before_roots_were_recorded_is_found_by_its_paths passes in kiln-core"
    required: true
    manual: false
---

# kiln-core test fixtures: no OS branch in layout_session_tests (follow-up of T594, boundary check)

# kiln-core test fixture branches on the OS (breaks the no_gpui boundary check)

T594 (Mac T1759) landed `kiln/crates/kiln-core/src/layout_session_tests.rs` with
`if cfg!(windows) { r"C:\old\project" } else { "/old/project" }` (line ~174). `scripts/check_kiln_boundary.py`
(stage no_gpui, skipped unless Cargo.toml changes, so the task gate missed it) rejects it:
"branches on the OS outside kiln/crates/kiln-core/src/platform, kiln/crates/kiln-platform/src". It failed the
sync gate of 8d913469.

Fix without any OS branch: build `old_root` from something absolute on every OS, e.g.
`std::env::temp_dir().join("old").join("project")` (the path need not exist; the store only needs it to be absolute),
and keep the JSON escaping the test already needs for a Windows path (serde_json::to_string of the path string).
Same test logic, same assertions. Run `scripts/check-no-gpui.sh` (or `python3 scripts/check_kiln_boundary.py`) and the
kiln-core test. Check extension_manifest.rs (also T594) for the same pattern.
