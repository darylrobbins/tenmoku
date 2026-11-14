---
task_uid: "01m7c2mjcqwmj46ewhrkh7j94h"
task: "T378"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::editorconfig_clean_open: a BOM .cs file under [*.cs] charset = utf-8 and an LF file under end_of_line = crlf both open with the tab clean; after one typed character and a save, the bytes on disk have the .editorconfig charset and line ending"
    required: true
    manual: false
  - id: "C2"
    text: "Saving an unedited file leaves its bytes unchanged; an explicit encoding or line-ending change by the user still marks the tab modified (tests in editorconfig_clean_open.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench editorconfig passes, with editorconfig_charset_tests updated where it pinned dirty-on-open"
    required: true
    manual: false
---

# Open files clean under .editorconfig: a charset (BOM) or end_of_line that differs from the file no longer marks the tab modified until the user edits; the next save after an edit converts (Mac T1828)

User report (macOS, main 81d23b0a3): C# files show as modified the moment they are opened. The earlier BOM fix (T1763) only stripped the BOM from the git diff base; it never touched the tab's dirty state.

Cause: `Buffer::is_dirty` (crates/fe-buffer/src/buffer.rs ~799-806) is true when `line_ending != saved_line_ending` or `encoding != saved_encoding`. Load sets both saved values to the disk format (~486-510). Opening then applies .editorconfig to the live values only:
- crates/fe-workbench/src/loader.rs `read_in_charset` (~230-251) ends with `buffer.set_encoding(wanted)`, so a BOM file under `charset = utf-8` (or a plain file under `utf-8-bom`) is dirty on open. Visual Studio writes a BOM on .cs files, so dotnet repos hit this all the time.
- crates/fe-workbench/src/pane.rs `apply_config` (~2312-2335) calls `set_line_ending` when `end_of_line` differs from the file.

Wanted behaviour (other editors apply .editorconfig on save, not on open):
1. A file opens clean whatever .editorconfig says about charset and end_of_line.
2. After the user edits the text, saving writes the .editorconfig charset and end_of_line (the existing save-path tests in editorconfig_save_tests.rs keep passing).
3. Saving a file the user has not edited stays a no-op on its bytes.
4. An explicit encoding or line-ending change by the user (the status-bar pickers or commands) still marks the tab dirty, as now.
5. The status bar shows the file's format as it is on disk until a save converts it; say in the decision file which one you chose if that is awkward.

Suggested shape: a fe-buffer method that sets the pending save format without moving the dirty baseline (or sets live and saved together on open), used by both loader.rs and pane.rs. Keep `is_dirty` meaning "the bytes a save writes differ from the disk" only once the user has made a change.

Tests: regression tests in a new crates/fe-workbench/src/tests/editorconfig_clean_open.rs declared from src/tests/mod.rs, written against base APIs only, with assert_eq! (not a bare assert! with a message). Update editorconfig_charset_tests.rs where it pins dirty-on-open.

## Implementation notes

`Buffer::set_pending_format` records the .editorconfig line ending and encoding a save writes once the text has been edited; the live values (and so the status bar) keep showing the file as it is on disk until a save converts it. Explicit `set_line_ending`, `set_encoding` and `change_line_ending` replace the pending value and mark the tab dirty as before.
