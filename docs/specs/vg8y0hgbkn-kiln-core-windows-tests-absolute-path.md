---
task_uid: "01m796pg9n2ka352vg8y0hgbkn"
task: "T594"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_file_from_before_roots_were_recorded_is_found_by_its_paths builds its root with an OS-absolute path and still passes on Linux and Mac"
    required: true
    manual: false
  - id: "C2"
    text: "manifest_errors_name_their_key_and_position covers a backslash absolute path (portable-path error) and a forward-slash absolute path (must be relative) and passes on Linux and Mac"
    required: true
    manual: false
---

# kiln-core Windows tests: absolute-path fixtures in layout_session and extension_manifest (Mac T1759)

Two Windows-only failures in kiln-core tests, both test fixtures that assume Unix paths.

(a) kiln/crates/kiln-core/src/layout_session_tests.rs:183
`a_file_from_before_roots_were_recorded_is_found_by_its_paths`: got [""] expected ["/old/project", ""].
"/old/project" is not absolute on Windows, so the store's root is dropped. Build the root from
`tempfile`/`Path::new(if cfg!(windows) { r"C:\old\project" } else { "/old/project" })` (and the
matching file path in the JSON, JSON-escaped) so the test is the same logic on every OS. Introduced
by 7391c87fa (T1666).

(b) kiln/crates/kiln-core/tests/extension_manifest.rs:210 `manifest_errors_name_their_key_and_position`:
on Windows the absolute test path "C:\\themes\\dark.toml" contains a backslash, so the error is the
portable-path one ("use / in extension paths", see extension_portable_paths.rs) instead of
"must be relative". Decide which check should win: if backslash-first is intended (it is the
documented portable rule), assert that message on Windows for this input and add a forward-slash
absolute ("C:/themes/dark.toml") case that must say "must be relative"; the Unix side is unchanged.
Do not change production check order unless a bug is shown; if it is, respec as a bugfix.

Both edits keep running on Linux/Mac: assert nothing OS-specific that fails there. Only Windows CI
proves the Windows branch.
