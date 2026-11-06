---
task_uid: "01m6rva8ks8mfpmxh6avtr9qtc"
task: "T487"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test charset_values (tests/editorconfig_new_properties.rs, top level): latin1, utf-8, UTF-8-BOM, utf-16be and utf-16le give their Charset; utf-32, cp1252 and unset give None; a nearer .editorconfig charset wins over its parent"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test max_line_length_values (tests/editorconfig_new_properties.rs, top level): 80 gives Columns(80), off and OFF give Off; 0, -1, wide and unset give None; the existing fe-config editorconfig tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test loader::charset_tests::charset_decides_ambiguous_bytes: BOM-less \"caf\\xC3\\xA9\\n\" under charset = latin1 reads as \"cafÃ©\" in windows-1252, clean, and saves the same bytes; under utf-8 it reads as \"café\"; BOM-less ASCII under utf-8-bom reads as UTF-8, needs saving, and saves with EF BB BF first; with no .editorconfig files read as before"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test loader::charset_tests::clear_bytes_keep_detection: under latin1 a file with a UTF-8 BOM reads as UTF-8 with BOM; under utf-8 the invalid UTF-8 \"\\xE9t\\xE9 caf\\xE9\\n\" reads in the encoding Buffer::from_file detects; under utf-8 a BOM-less UTF-16LE file (zero bytes at the odd indices) reads as UTF-16LE; under utf-16be a file with a UTF-16LE BOM reads as UTF-16LE"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test pane::editorconfig_charset_tests::new_files_take_the_charset: a missing path opened under charset = utf-16le is clean until typed into, and \"é\" saved writes FF FE E9 00; an untitled tab holding \"é\" saved as a path under charset = latin1 writes E9; a Save As under latin1 refused on \"😀\" leaves the buffer in UTF-8"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test pane::editorconfig_reload_tests::saving_an_editorconfig_in_the_editor_reapplies: with a space-indented a.go and the project .editorconfig open, editing the .editorconfig to [*.go] indent_style = tab, tab_width = 8 and saving it with cmd-s gives a.go Tabs and tab size 8 without reopening; an editor of a file outside that directory keeps its properties; saving the .editorconfig again unchanged leaves an indent style set on a.go since as it is"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test pane::editorconfig_reload_tests::disk_changes_reapply: pane::editorconfigs_changed with the FsChange::Path of a .editorconfig created on disk applies indent_size = 2, end_of_line = crlf and trim_trailing_whitespace = true to an open b.js; after the file changes the new values apply; after it is deleted b.js has editor.tab_size, its detected indentation, save fixes off and still CRLF; FsChange::Rescan re-reads; a path not named .editorconfig changes nothing"
    required: true
    manual: false
  - id: "C8"
    text: "cargo test -p fe-config -p fe-workbench and cargo clippy -p fe-config -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Read .editorconfig charset (the encoding new and ambiguous files open and save in) and max_line_length, and re-apply a saved, created or deleted .editorconfig to open editors (Mac T1419)

## Problem

Three gaps in Tenmoku's EditorConfig support (T467/T30, T478, T481):

1. `charset` is not read. `crates/fe-config/src/editorconfig.rs` reads only `indent_style`, `indent_size`,
   `tab_width`, `end_of_line`, `insert_final_newline` and `trim_trailing_whitespace`. Files open in the encoding
   `Text::from_reader_detecting` detects (BOM, then BOM-less UTF-16 by its zero bytes, then UTF-8 validity of the
   first 64 KiB, then a `chardetng` guess; T249, T250, through `loader::read` in
   `crates/fe-workbench/src/loader.rs`), and new files save as UTF-8, whatever the project's `.editorconfig` says.
2. Open editors never see `.editorconfig` changes. `mod configs` in `crates/fe-workbench/src/pane.rs` (~line 1790)
   reads each editor's properties once, when it opens; saving, creating or deleting an `.editorconfig` applies only
   to files opened afterwards.
3. `max_line_length` is not read.

## Change

### fe-config (`crates/fe-config/src/editorconfig.rs` only; the module is `pub mod editorconfig`, so `lib.rs` is not touched)

- `pub enum Charset { Latin1, Utf8, Utf8Bom, Utf16Be, Utf16Le }` and `EditorConfig::charset: Option<Charset>`,
  from `latin1`, `utf-8`, `utf-8-bom`, `utf-16be`, `utf-16le` in any case. `unset`, missing and other values are
  `None`.
- `pub enum MaxLineLength { Columns(usize), Off }` and `EditorConfig::max_line_length: Option<MaxLineLength>`: a
  positive integer, or `off` in any case. `0`, negative, non-numeric, `unset` and missing are `None`. `EditorConfig`
  stays `Copy`.
- The module doc comment lists the new properties.

### charset decides how a file opens (`crates/fe-workbench/src/loader.rs`)

`loader::read` (on the reader's thread, as now) reads the file's `editorconfig_for(path).charset`. With none, it
reads exactly as now. With one, the charset's encoding is: `latin1` → windows-1252 (encoding_rs's, the WHATWG,
mapping of the `latin1` label: `fe_buffer::Encoding::for_label(b"latin1")`), `utf-8` → UTF-8 without a BOM,
`utf-8-bom` → UTF-8 with a BOM, `utf-16le` / `utf-16be` → that UTF-16 with a BOM (EditorConfig has no BOM-less
UTF-16 value, and the BOM keeps the file recognisable).

- The bytes clearly disagree with the charset when `FileEncoding::detect` finds a BOM of another encoding (any BOM
  under `latin1`), or BOM-less UTF-16 by its zero bytes when the charset is not that UTF-16, or when decoding the
  whole file in the charset's encoding replaces malformed input (`DecodedFile::replaced`; never for windows-1252).
  Then detection wins: the file opens as it does today, saves back in that encoding, the status bar shows it, and
  one `log::warn!` names the file, the charset and the encoding used.
- Otherwise (no BOM or the charset's own BOM, and the bytes decode cleanly: UTF-8-valid, `chardetng`-guessed and
  empty files are all ambiguous), the file opens decoded in the charset's encoding and saves in it, BOM included.
  When the file's own BOM state differs from the charset's (a BOM-less file under `utf-8-bom`), the buffer needs
  saving, as `end_of_line` on a file with other line endings already does (T478).
- Every `loader::read` caller gets this (pane opens, workspace edits, Problems panel, refactor preview, compare
  with saved). Reopen with Encoding (`loader::read_with`) and reloads on disk changes keep the encoding
  asked for or the buffer's, as now. Project-wide replace (`search_replace.rs`, `Buffer::from_file`) is out of
  scope: it writes back in the encoding it read, so it changes no bytes it didn't edit.

### charset picks the encoding new files save with (`pane.rs`, `workbench_files.rs`)

- A tab for a file missing on disk (`Opened::Text(None)` → `loader::buffer_or_new`) takes the charset's encoding
  when `configs` applies its properties, and an untouched one stays clean: closing it asks nothing. Without a
  fe-buffer change this can be `Buffer::reload` of an empty `DecodedFile` in that encoding with stamp `None`,
  which marks it saved with no edit and no undo step, the file still known missing; if the user typed before the
  read finished, `Buffer::set_encoding`.
- Save As (`Workbench::save_editor_as_with`, which already reads the destination's `.editorconfig`) of a buffer
  that has no path yet (an untitled tab) to a path under a charset sets the buffer to the charset's encoding before
  the save, and back to the previous encoding if the save fails (for example on a char latin1 can't represent,
  which T249's refusal reports as now). Save As of a file that has a path keeps its encoding.
- Out of scope: re-decoding open text. An `.editorconfig` change (below) doesn't reopen open files in a new
  charset; files opened afterwards take it.

### open editors follow `.editorconfig` changes (`pane.rs`, `workbench_root.rs`)

- New `pub(crate) fn editorconfigs_changed(changes: &[fe_project::FsChange], cx: &mut App)` in `pane.rs`, beside
  `mod configs`: each `FsChange::Path` whose file name is exactly `.editorconfig` re-reads (in the background, as
  `configs::read` does) the properties of every open editor, in any pane or window, whose path is under that
  file's directory; an `FsChange::Rescan` re-reads every open editor; other paths do nothing.
- Callers: `Workbench::follow_root` (`crates/fe-workbench/src/workbench_root.rs`) calls it for the project panel's
  `PanelEvent::FsChanged` (the project watcher: edits by other programs, `git checkout`), and for the paths (the `PathBuf` of each `(PathBuf, bool)`) of
  `PanelEvent::Created` and `PanelEvent::Deleted` (the tree's own New File and Delete, without waiting for the
  watcher).
- An editor whose file is named `.editorconfig` triggers the same re-read for its directory each time a save of it
  completes (cmd-s, vim `:w`, auto save, saving on close), and once when Save As gives it that name (`sync_paths`).
  So it works for an `.editorconfig` outside the project root, which no watcher sees.
- Applying a re-read: if the properties equal the ones applied, nothing changes (so a save in the editor followed
  by the watcher's event for it, or a rescan, is a no-op, and indentation the user set since stays). Otherwise the
  old ones are dropped as `sync_paths` drops them (tab size back to `editor.tab_size`, indentation re-detected from
  the text), then the new ones apply as on open: indentation, tab width (kept across settings reloads), `end_of_line
  = lf | crlf`, and `SaveFixes` from `trim_trailing_whitespace` and `insert_final_newline` (both off when unset). A
  removed `end_of_line` leaves the buffer's line ending as it is. When reads for one editor overlap, the last one
  started wins.
- `charset` and `max_line_length` are kept with the rest but, as said, not applied to open text.

### max_line_length

Parsed and exposed on `EditorConfig` only. Mac main has no wrap column: soft wrap is on/off at the window edge
(`editor.soft_wrap`), and the wrap column is T1402 (`editor.wrap_column`, lent to helium). Mac main does have
`editor.rulers`, but that is a list of guides painted from the window-wide `EditorTheme`, with no per-editor
column; a per-editor ruler means fe-gpui `view.rs`/`lib.rs` changes that T1402 also makes. Wiring
`max_line_length` to the wrap column (with `off` meaning none), and a right-margin ruler if wanted, is left for a
card after T1402 lands.

## Tests

New files only, on temp directories:

- `crates/fe-config/tests/editorconfig_new_properties.rs`.
- `crates/fe-workbench/src/loader_charset_tests.rs`, declared from `loader.rs` with `#[cfg(test)] #[path] mod
  charset_tests;` (as `loader_encoding_tests.rs` is).
- `crates/fe-workbench/src/editorconfig_charset_tests.rs` and `crates/fe-workbench/src/editorconfig_reload_tests.rs`,
  declared from `pane.rs` with `#[path]`, as `editorconfig_tests.rs` is. The watcher test calls
  `pane::editorconfigs_changed` with `FsChange`s; it doesn't wait on a real watcher.

The existing tests in `crates/fe-config/tests/editorconfig*.rs`, `editorconfig_tests.rs`,
`editorconfig_save_tests.rs` and `loader_encoding_tests.rs` pass unchanged.

## Conflicts

No running or ready task touches these files. `pane.rs` is also in T1400 (pin tabs), and fe-config's `lib.rs`
(left untouched here) in T1397 and T1403; all three are lent to helium and blocked on the Mac, so no dependency
(an abandoned dependency would stay unsatisfied). Nor on T1402, for the same reason.

## Helium notes

- "Every open editor, in any pane or window" includes editors outside tabs: a DiffView's working side is an
  editor too. Enumerate through `Workbench::file_editors` (`crates/fe-workbench/src/git_diff.rs:733`: the tabs'
  editors plus each diff view's working side), as `:wa`, `:qa` and disk sync do, rather than pane items only, and
  add a test with a diff tab open on a file under the changed `.editorconfig`. Calling it needs no scope change.

## Implementation notes

- `loader::read_in_charset` decodes once in the charset's encoding with `Buffer::from_file_with`. fe-buffer's
  `replaced_on_read` is crate-private, so `loader::replaced_on_read` tells malformed input from the buffer: no
  U+FFFD in the text means nothing was replaced; with one, the save snapshot's own refusal of replaced bytes
  (`write_to(io::sink())`, which refuses before writing) answers. A clash seeks back and reads with
  `Buffer::from_file`, as before.
- New files: `loader::buffer_or_new` records the ids of the empty buffers it makes for missing files, and
  `loader::take_new_file` hands each out once. An editor's first properties always take its buffer's mark, charset
  or not, so a charset added later never applies. The charset's encoding applies only while the buffer is still
  known missing: not when it was saved before the read landed. A buffer that was clean before its properties
  applied then gets `Buffer::did_save`. That way it stays clean, has no undo step, and keeps its stamp, so the file
  is still known missing. A buffer typed into first gets only `set_encoding`.
- Save As: `EditorView::save_as_in` (fe-gpui `pane_item.rs`, added to the scope by a respec) is
  `save_as_with` plus an `untitled_encoding`. The workbench passes the destination's charset when the buffer is
  untitled at the request. Each request keeps its place in the buffer's shared save queue, and decides when its
  snapshot is prepared, at once or once the save before it is over: the buffer still untitled (an earlier Save As
  failed, or there was none) means it writes in the charset; a path (an earlier Save As gave it one) means it
  writes in the buffer's encoding, as any Save As of a file does. The buffer takes the charset in the same update
  as `finish_save_as`, so a save queued behind it starts from it; a failed save leaves the buffer's encoding
  untouched. `save_as_with` passes `None`.
- A save of an `.editorconfig` editor is heard by observing the view, which redraws after every save
  (`spawn_save`), and draining a `Buffer::subscribe` queue for `BufferEvent::Saved`. fe-gpui has no save event, and
  this way it needs none. Every editor of an `.editorconfig` is heard, a diff view's working side included.
- `editorconfigs_changed` does its work in `cx.defer`, because its callers (`follow_root`, and `sync_paths` inside
  a pane update) hold the workbench or pane that `file_editors` reads. Directories and editors' files are compared
  as given and, on the background executor, resolved through symlinks (up to the nearest folder that exists). The
  tab size a dropped config falls back to is the window's `editor.tab_size` when the read lands.
- `Pane::sync_tabs` reads a diff view's working side's properties too (as `file_editors` lists it), so it has them
  from the start, as a tab's editor does. `Pane::sync_paths` follows a working side given a new path (a Save As or
  vim `:sav` from the diff). It is told apart by the path its properties were read for, because the tab's path is
  the diff's. It drops the old properties, reads the new ones and hears saves under an `.editorconfig` name.
