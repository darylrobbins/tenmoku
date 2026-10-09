---
task_uid: "01m4fa9z15jefxp9hj6ffj8dhc"
task: "T250"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test encoding_tests: opening windows-1252 ('café naïve'), Shift_JIS, UTF-16LE-with-BOM and UTF-8-with-BOM files in the workbench shows the decoded text with no U+FFFD or U+FEFF, and the status bar encoding item reads windows-1252, Shift_JIS, UTF-16LE and UTF-8 with BOM"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test encoding_tests: typing in an opened windows-1252 file and dispatching editor.save writes windows-1252 bytes; typing '😀' and saving leaves the file byte-identical and the tab dirty"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::status_bar::file_facts passes unchanged"
    required: true
    manual: false
---

# Open files in their detected encoding in the workbench and show it in the status bar

## Problem

The previous card gave `Buffer` a detected encoding (`Buffer::from_reader_detecting`, `Buffer::encoding`,
`FileEncoding::label`) and made `Buffer::write_to` save in it, refusing lossy saves. The app doesn't use it
yet:

- `crates/fe-workbench/src/loader.rs` `read` builds every buffer with `Buffer::from_reader`, as UTF-8 with
  U+FFFD for invalid bytes. It serves both the launch file (`crates/fastedit/src/main.rs` via
  `loader::start`) and files opened from the project panel (`crates/fe-workbench/src/pane.rs`). So a
  windows-1252, Shift_JIS or UTF-16 file still opens as replacement characters or NUL-riddled text.
- The status bar's encoding item (`crates/fe-workbench/src/status_bar.rs`, the
  `text.char_at(0) == Some('\u{feff}')` check in the built-in items) guesses from a leading U+FEFF and can
  only say `UTF-8` or `UTF-8 with BOM`. Once the loader detects, the BOM is stripped from the text, so that
  check would stop working.

## Change

- `loader::read` builds the buffer with `Buffer::from_reader_detecting` (same 256 KiB `BufReader`). A missing
  file stays `Ok(None)`; a new file saves as UTF-8. Update the doc comment, which says it reads "the way
  `EditorModel::open` does": it now detects the encoding, and `EditorModel::open` doesn't.
- In `status_bar.rs`, the encoding item's text is `model.buffer().encoding().label()`. Remove the U+FEFF
  check. Change nothing else in the file: T219, T231 and T247 also edit it.
- Saving needs no change: `editor.save` (cmd-s) goes through `EditorModel::save` → `Buffer::write_to`, which
  now writes the file's encoding and refuses a lossy save with an error the command logs.

Large files stay as they are: T241's fe-fileview is a separate read-only path and the loader doesn't route
to it here.

## Tests

In the new file `crates/fe-workbench/src/loader_encoding_tests.rs`, declared from `loader.rs` with
`#[cfg(test)] #[path = "loader_encoding_tests.rs"] mod encoding_tests;` (not from `src/tests/mod.rs`, which
T144 and others hold). Set up the workbench the way `status_bar_vim_tests.rs` does. Fixtures are byte
literals written to a tempdir. `src/tests/status_bar.rs` `file_facts` must keep passing unchanged.

## Implementation notes

- `tests::search_panel_followups::invalid_utf8_before_a_match_…` assumed the editor decodes invalid bytes lossily. Detection now opens such files in the detected legacy encoding, so the test's `a.txt` carries valid multi-byte UTF-8 after the bad byte to stay on the lossy UTF-8 path, and the final "decodes lossily" assertion (`b.txt` starts with an invalid sequence, so it's no longer UTF-8) is dropped.
- Follow-up: the search panel maps byte offsets assuming a lossy UTF-8 decode, so for files opened in another encoding its selection can be off. Not addressed here.
