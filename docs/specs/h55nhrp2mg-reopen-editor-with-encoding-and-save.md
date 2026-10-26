---
task_uid: "01m5tf0m541rft0th55nhrp2mg"
task: "T348"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-text decode_with_encoding: bytes 'caf\\xe9' decoded as windows-1252 give 'café' and as UTF-8 give a replaced char with DecodedFile::replaced true; a UTF-16LE file given UTF-16LE with its BOM strips the BOM; a streaming read over 64 KiB decodes the same as in one piece"
    required: true
    manual: false
  - id: "C2"
    text: "encoding_picker_tests::reopen_with_encoding: a windows-1252 file opened as Shift_JIS shows mojibake; workbench.reopen_with_encoding choosing windows-1252 shows the correct text, keeps the selections, and the status bar label reads windows-1252 Known limits (T354): the vim visual remap after a reopen skips a visual selection suspended by a / or ? command line (review F11), resets a block selection's $ goal so ragged rows shrink (F12), and autoscrolls the caret into view instead of preserving the viewport (F13); T354 adds the fe-vim remap API and fixes all three. Known limit: selections of editors sharing the buffer in other workbenches/diff working sides are not remapped (T357)."
    required: true
    manual: false
  - id: "C3"
    text: "encoding_picker_tests::reopen_refuses_dirty: with unsaved edits the command changes nothing and reports why"
    required: true
    manual: false
  - id: "C4"
    text: "encoding_picker_tests::save_with_encoding: saving a UTF-8 buffer 'café' with windows-1252 writes the single byte 0xE9 and the label updates; saving 'a€' with ISO-8859-2 (a real encoding without '€'; encoding_rs's ISO-8859-1 is windows-1252, which has it) writes nothing, keeps the encoding and shows the unmappable-character refusal"
    required: true
    manual: false
  - id: "C5"
    text: "Clicking the status bar encoding item lists the two actions and then the encoding picker; catalog tests updated; cargo nextest run -p fe-text -p fe-buffer -p fe-workbench and clippy pass; existing loader_encoding_tests pass unedited"
    required: true
    manual: false
---

# Reopen Editor with Encoding… and Save with Encoding… commands and status bar actions, as VS Code's changeEncoding (Mac T1161)

# Reopen Editor with Encoding… and Save with Encoding… commands and status bar actions, as VS Code's changeEncoding (Mac T1161)

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code lets you fix a mis-detected encoding with Reopen Editor with Encoding… and convert a file with Save with Encoding… (workbench.action.editor.changeEncoding, the status-bar encoding item's two actions). fastedit detects and shows the encoding (T239/T249/T250: `crates/fe-text/src/encoding.rs`, `crates/fe-workbench/src/loader.rs:152`, the status bar item `status_bar.rs:62`) but cannot change it, so a wrong chardetng guess (a windows-1252 file shown as Shift_JIS) or a wish to convert to UTF-8 has no route. T251 is only the human check of the existing behaviour.

## Current state
- `Text::from_reader_detecting` (`encoding.rs:305`) sniffs the head and decodes the whole file; there is no entry point that decodes with a given encoding.
- A buffer has `encoding` and `saved_encoding` (`crates/fe-buffer/src/buffer.rs:146`, `:244`, `:221`) and saves through `write_encoded`, which refuses text the encoding cannot represent (`buffer.rs:167-205`, T249).
- The status bar shows the label (`status_bar.rs:99`); commands live in `crates/fe-workbench/src/workbench_commands.rs`; `git_branch.rs` is the pattern for a picker.

## Change
- fe-text: `Text::from_reader_with(reader, FileEncoding)` decoding with the given encoding (BOM stripped if it matches), same streaming and `replaced` reporting. fe-buffer: `Buffer::from_file_with(file, encoding)` and `Buffer::set_encoding(encoding)` (marks the buffer as needing save; the next save writes in it).
- fe-workbench: new `crates/fe-workbench/src/encoding_picker.rs`: a picker of encodings with the active one marked (UTF-8, UTF-8 with BOM, UTF-16 LE / BE, Western windows-1252 and ISO-8859-15, Central European windows-1250 and ISO-8859-2, Cyrillic windows-1251 and KOI8-R, Greek, Turkish, Hebrew, Arabic, Japanese Shift JIS / EUC-JP, Chinese GBK / GB18030 / Big5, Korean EUC-KR, all from `encoding_rs`).
- `workbench.reopen_with_encoding` ("Reopen Editor with Encoding…") re-reads the active file from disk decoded in the picked encoding, keeping selections and scroll; refuses with a status message and no change if the buffer has unsaved edits. `workbench.save_with_encoding` ("Save with Encoding…") sets the picked encoding and saves; if the text has characters the encoding cannot represent, nothing is written and the encoding stays as it was (the T249 refusal message). Clicking the status bar encoding item opens a two-entry picker (Reopen with Encoding / Save with Encoding) as VS Code's.
- No default keys.

## Tests
New `crates/fe-text/tests/decode_with_encoding.rs`, `crates/fe-workbench/src/encoding_picker_tests.rs`; catalog tests updated.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y65).

## Implementation notes

- The status bar's encoding item runs a third command,
  `workbench.change_encoding` ("Change File Encoding", VS Code's
  `workbench.action.editor.changeEncoding`), which lists the two actions; the
  item comes from `fe-gpui`'s `pane_item.rs` without a command, so
  `StatusBar::items` gives an editor's encoding item that command.
- Revision 2 (review ndpgkj F6): no ISO-8859-1 row. `encoding_rs` follows the
  WHATWG Encoding Standard, where the ISO-8859-1 label is windows-1252 (listed
  already, and able to save '€'), and `FileEncoding` can't name a true Latin-1;
  a stand-in that read as windows-1252 differed from Latin-1 in 0x80–0x9F. C4's
  refusal case uses ISO-8859-2, a real encoding without '€', instead.
- Refusals and failures show as toasts. One encoding operation runs per buffer
  at a time, and none starts while the buffer is saving (F1, F2). Save with
  Encoding checks the text in the new encoding off the main thread
  (`SaveSnapshot::in_encoding` written to a sink), then saves through
  `EditorView::save_in_encoding` (revision 3 adds `crates/fe-gpui/src/pane_item.rs`
  to the scope for it, review c1jh53 F1): the encoding rides with that save in
  the editor's save queue, set only while its snapshot is prepared (after any
  save hook) and made the buffer's once the save is recorded, before the queue
  lets the next save start, and only if the save is recorded: one the file
  changed on disk after fails, keeping the encoding (n22s0p F10). A Save or
  Save As queued behind it writes in the new encoding if it succeeded and in
  the old one if it failed. An untitled buffer
  only takes the encoding, for its first save to write in, and only if it isn't
  saving and its text didn't change while it was checked (F5, nytrgh F7,
  c1jh53 F8).
- Reopen is `EditorModel::reload` of the file decoded with
  `Text::from_reader_with`, so it's one undoable edit, as a reload from disk
  is. It applies the read only if the buffer's text version, path, encoding and
  disk stamp are all still those it started from. Each editor's carets and
  selections, and vim's visual anchor and cursor in visual mode (n22s0p F11),
  are mapped row by row: the chars a row's old and new decoding share are
  found by a char diff a 4K-char window at a time, changes slid to the end of
  a run of one repeated char, each window going on from what the one before
  shared in its first half (a run crossing the half cut there, n22s0p F4), so
  shared text keeps its place however long the row; a column at the start of a shared
  run goes past text new before it, and one in a changed run goes as far into
  the new run (F4, c1jh53 F4, F9). A BOM is dropped only when it's the picked
  encoding's own, and `bom` then reports the file's.
