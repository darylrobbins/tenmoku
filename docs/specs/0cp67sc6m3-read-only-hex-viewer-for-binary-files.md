---
task_uid: "01m7j1pss40892rm0cp67sc6m3"
task: "T1895"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-fileview: FileView::bytes(range) returns raw bytes clamped to len() (ReadError::Changed on a short read) and find_bytes finds any byte needle incl. 0x00/0x0A/0x0D, forward/backward, wrapping, stoppable; find keeps its behaviour: bytes_reads_raw_bytes_across_a_chunk_edge, bytes_past_a_truncation_is_changed, find_bytes_finds_nuls_and_newlines, find_bytes_wraps_both_ways (crates/fe-fileview/tests/hex_bytes.rs) and the existing fe-fileview tests pass (cargo nextest run -p fe-fileview)"
    required: true
    manual: false
  - id: "C2"
    text: "Rows are offset | 16 hex bytes (gap after the 8th) | ASCII (0x20..=0x7E as is, else \".\"), offsets 8 hex digits or 16 past 0xFFFF_FFFF bytes, a partial last row, \"Empty file\" for an empty one, in the theme mono font: hex_viewer::model_tests::row_text_formats_offset_hex_and_ascii, ::offsets_widen_past_4_gib, ::last_row_is_partial, ::an_empty_file_says_so (cargo nextest run -p fe-gpui)"
    required: true
    manual: false
  - id: "C3"
    text: "Selecting in one column highlights the same bytes in the other (focused column in selection, the other in an existing weaker EditorTheme colour, no new theme tokens); click, drag, shift-click, arrows, shift+arrows, page up/down, home/end, secondary-up/down; typing changes nothing: hex_viewer::tests::dragging_in_hex_highlights_the_same_bytes_in_ascii, ::clicking_ascii_selects_the_byte_in_hex, ::shift_arrows_extend_by_byte_and_row, ::typing_edits_nothing"
    required: true
    manual: false
  - id: "C4"
    text: "Only on-screen rows are read and the scroll position is a u64 row: a 5 GiB sparse file (File::set_len) shows its first screen with FileView::cached_bytes \u003c= 2 * CHUNK_BYTES and secondary-down shows its last row at the right offset: hex_viewer::tests::a_5_gib_sparse_file_scrolls_to_its_last_row_reading_only_the_screen"
    required: true
    manual: false
  - id: "C5"
    text: "hex.go_to_offset (ctrl-g in HexViewer) opens an input in the header taking decimal or 0x hex; it selects that byte and scrolls it into view; past the end or unparseable shows an inline error and keeps the selection; escape closes it and refocuses the rows: hex_viewer::tests::go_to_offset_takes_decimal_and_0x, ::go_to_offset_past_the_end_says_so"
    required: true
    manual: false
  - id: "C6"
    text: "hex.find (secondary-f in HexViewer) opens a find input with a Hex/Text toggle and Match Case (text mode); hex queries parse pairs with optional spaces (\"de ad BE EF\", \"deadbeef\"), an odd digit count or non-hex is an inline error; hex.find_next/hex.find_previous select the next/previous match from the selection, wrapping, and say wrapped/not found; the scan runs on the background executor and stops on a new query, close or drop: hex_viewer::find_tests::hex_query_finds_bytes_with_nul_and_newline, ::text_query_matches_case_by_toggle, ::find_wraps_and_reports_not_found, ::a_new_query_stops_the_running_search"
    required: true
    manual: false
  - id: "C7"
    text: "secondary-c (hex.copy) copies the selection as hex when the hex column is focused and as text when the ASCII column is; hex.copy_as_hex gives uppercase space-separated pairs (\"DE AD BE EF\"), hex.copy_as_text the bytes as lossy UTF-8; more than fe_editor::COPY_LIMIT bytes is refused with a toast as FileViewer refuses: hex_viewer::tests::copy_follows_the_focused_column, ::copy_as_hex_and_as_text, ::copy_over_the_limit_is_refused"
    required: true
    manual: false
  - id: "C8"
    text: "The tab is never dirty and has nothing to save; its status bar shows READ_ONLY_STATUS and the status::CURSOR item as the offset (\"Offset 0x1A2F\") or the selected range and its byte count; a file changed or truncated on disk shows the changed banner with Reload, never a panic: hex_viewer::pane_item_tests::never_dirty_and_shows_read_only_and_the_offset, ::a_truncated_file_shows_changed_with_reload"
    required: true
    manual: false
  - id: "C9"
    text: "PaneItem KIND \"fastedit.hex_viewer\", registered in fe-workbench lib.rs: serialize keeps path, anchor, head and top row; restore opens the file off the main thread with the selection clipped to its length; a missing file is not restored; split shares the Arc\u003cFileView\u003e: hex_viewer::pane_item_tests::restores_path_and_selection_clipped, ::a_missing_file_is_not_restored, ::split_shares_the_view, tests::hex_viewer::a_saved_layout_reopens_the_hex_tab (cargo nextest run -p fe-workbench)"
    required: true
    manual: false
  - id: "C10"
    text: "A non-image file with a NUL in its first 8 KiB (BINARY_PROBE = 8 \u003c\u003c 10) opens in a HexViewer from the tree, quick open and Pane::open, checked before the large-file threshold; files starting with a UTF-16/UTF-32 BOM, text files, images and SVGs open as before: tests::hex_viewer::a_binary_file_opens_in_hex, ::text_images_and_bom_files_open_as_before, ::a_large_binary_opens_in_hex_not_the_file_viewer (loader::test_support::lower_threshold); image_viewer_open_tests, svg_view_as_text_tests, large_file_tests pass"
    required: true
    manual: false
  - id: "C11"
    text: "hex.open_as_hex \"Open as Hex\" (context editor || ImageViewer) replaces the active editor, large-file FileViewer or image tab in place; a dirty editor stays, with a toast; hex.open_as_text \"Open as Text\" (context HexViewer) and the header Text button give an editor, or the text FileViewer for a file at or past the large-file threshold without reading it into a buffer; both are cataloged with titles in commands.ftl: tests::hex_viewer::open_as_hex_replaces_an_editor_in_place, ::open_as_hex_refuses_a_dirty_editor, ::open_as_text_from_hex_gives_an_editor, ::open_as_text_of_a_large_binary_gives_the_file_viewer, ::hex_commands_are_cataloged_in_their_contexts"
    required: true
    manual: false
  - id: "C12"
    text: "The minimap is off in hex mode (the user: \"Let's disable the minimap for this mode.\"): with [editor.minimap] enabled = true a binary file's hex tab draws no minimap and its rows reach the pane's right edge less the scrollbar, a settings reload toggling the minimap leaves it off, Open as Text gives an editor with its minimap on and Open as Hex drops it again: tests::hex_viewer::hex_mode_has_no_minimap"
    required: true
    manual: false
  - id: "C13"
    text: "fastedit.toml binds secondary-f, ctrl-g, secondary-c and the find.next/find.previous keys to the hex commands in context HexViewer only, on macOS and Linux, leaving the editor's keys alone; fe-gpui command_tests' context-free list includes them: the_hex_keys_resolve_in_the_hex_viewer_on_each_platform, the_hex_keys_leave_the_editor_alone (crates/fe-keymap/tests/hex_viewer_keys.rs) and fe-gpui command_tests pass"
    required: true
    manual: false
  - id: "C14"
    text: "A UI audit surface hex_viewer (audit/hex_viewer.rs, its mod line and SURFACES entry in audit_scenes.rs, a Captures row in docs/decisions/25w38k498b-ui-polish-audit.md): the demo project with a small binary file open in hex and bytes 0x10..0x1F selected, probes in the header and the upper rows only (the inspector card adds a strip at the bottom); cargo run -p fe-workbench --example screenshots -- audit \u003cdir\u003e exits 0 and writes hex_viewer_dark.png and hex_viewer_light.png (Mac)"
    required: true
    manual: false
  - id: "C15"
    text: "Every user-facing string is in hex.ftl, commands.ftl or pane.ftl, debug_selector ids use verbatim!, the l10n lint and clippy --workspace pass, and hex_viewer.rs's module doc states the reuse (FileView bytes and find, no LineIndex) and that the view is read-only with no minimap"
    required: true
    manual: false
---

# Read-only hex viewer for binary files: offset | hex | ASCII rows, Open as Hex, binary files open in hex by default, minimap off, go to offset, find by hex or text, copy as hex or text

## Ask

The user, verbatim: "Should we offer a hex editor view for binary files?" The lead proposed a read-only viewer first; the user: "Let's card it. Let's disable the minimap for this mode."

So this card is a read-only hex view of a file: no byte editing, no dirty state, no save. The byte inspector (u8 to f64, little/big endian) is the follow-up card that depends on this one.

## What it looks like

A pane tab, `HexViewer` (crates/fe-gpui/src/hex_viewer.rs), drawn in the editor theme's mono font (`EditorTheme.font_family` / `font_size`). Each row is:

`offset | 16 hex bytes (a wider gap after the 8th) | ASCII column`

- The offset is 8 hex digits, or 16 for a file longer than 0xFFFF_FFFF bytes, in `gutter_foreground`.
- In the ASCII column 0x20..=0x7E show as themselves and every other byte as `.`.
- The last row can be partial. An empty file shows one line, "Empty file".
- A one-line header strip at the top holds the Go to Offset and Find inputs while they are open, plus a "Text" button (Open as Text), placed like the image viewer's `image-viewer-open-as-text` button.
- Use no new theme tokens. A new token group breaks `design_role_tokens`, so use the existing `EditorTheme` colours: `selection` for the focused column's selection, and an existing weaker colour (for example `identifier_highlight` or `find_match_other`) for the same bytes mirrored in the other column. `find_match` marks the current find match.

## Reuse plan

`crates/fe-gpui/src/file_viewer/` is the large-file text viewer. It is a `ViewerModel` over `FileText`, and it needs a `LineIndex` to find its rows. The hex view does not need an index: row `r` starts at byte `16 * r`. So:

- **Reuse `fe_fileview::FileView`.** It is a pread-backed view with a 16 MiB chunk cache. It records the file's length at open, reports `ReadError::Changed` on a short read, and has `check()` for freshness. Its raw `read_bytes` is private today: make it public as `FileView::bytes(range) -> Result<Vec<u8>, ReadError>`, clamped to `len()`. The hex view reads only the rows on screen. It never builds a `LineIndex` and never uses `ViewerModel` or `FileText`.
- **Reuse `fe_fileview::find`'s block scan** (`scan_forward` / `scan_backward` past the cache, stoppable). Add `find_bytes(view, needle: &[u8], case_sensitive, within, from, direction, stop)`, which takes any bytes including 0x00, 0x0A and 0x0D. `find` keeps its current behaviour and delegates to it after its newline check. Copy the hex view's background search from `file_viewer/find.rs`: a `FindState` on the background executor, stopped by a new query, by closing the bar, or by dropping the view, with the same `Hold` test seam.
- **Copy FileViewer's patterns, not its code:** the changed-on-disk banner with Reload (`check()` on focus and every `CHECK_INTERVAL`), the status items `READ_ONLY_STATUS` (`file_viewer/pane_item.rs`, make it reachable from `hex_viewer`) and `status::CURSOR`, and the `PaneItem` impl (`split` shares the `Arc<FileView>`; `serialize` / `restore` off the main thread, as `file_viewer/pane_item.rs` does).
- **Scrolling:** keep the scroll position as a `u64` top row plus a pixel fraction. A row count times the row height overflows f32 precision for multi-GiB files, so never hold the position as one absolute pixel value. Wheel, scrollbar drag and keyboard all go through the row.

## How it opens

- `crates/fe-workbench/src/loader.rs`: `open` gains `Opened::Binary { view: Arc<FileView> }`. A file that isn't an image and has a NUL in its first 8 KiB is binary. Define `BINARY_PROBE: usize = 8 << 10`, as `chat_apply.rs` and `agent_tools.rs` do, and leave those two alone. The probe runs before the `LARGE_FILE_BYTES` check, so a large binary file opens in hex, not in the text FileViewer. A file starting with a UTF-16 or UTF-32 BOM (`FF FE`, `FE FF`, `00 00 FE FF`) is text, as today. Images, SVGs and text files open exactly as they do now.
- `crates/fe-workbench/src/pane.rs`:
  - `file_landed` opens `Opened::Binary` in a new `new_hex_viewer`, and `Tab::hex_viewer()` sits beside `Tab::viewer()` / `image_viewer()`.
  - `Pane::open_as_hex(path)` replaces the active editor, FileViewer or ImageViewer tab in place (`replace_in_place`). An editor with unsaved changes stays, with a toast from `pane.ftl`, as `open_as_image` refuses with `workbench-pane-save-before-image`.
  - `Pane::open_as_text` from a hex tab replaces it with an editor. For a file of `loader::large_bytes` or more it gives the text FileViewer instead and never reads that file into a buffer.
- `crates/fe-workbench/src/workbench_commands.rs` (append-only), next to `image.open_as_image` / `image.open_as_text`:
  - `hex.open_as_hex` "Open as Hex", context `editor || ImageViewer`. The FileViewer sets the `editor` key context too.
  - `hex.open_as_text` "Open as Text", context `HexViewer`.
  - Add their titles to `crates/fe-workbench/l10n/en-US/commands.ftl` as `command-hex-open_as_hex` and `command-hex-open_as_text`.
- `crates/fe-workbench/src/lib.rs` (append-only): `kiln_shell::layout::register_item::<fe_gpui::HexViewer>(cx)` with `KIND = "fastedit.hex_viewer"`.

## The view's commands and keys

The view's own commands live in a `Registry` in `hex_viewer.rs`, built as `image_viewer::registry()` is, with context `HexViewer` (`KEY_CONTEXT`). Wire it into `crates/fe-gpui/src/commands.rs` (`Catalog`, `command_list`) the way `image` is. The commands:

- `hex.go_to_offset`
- `hex.find`
- `hex.find_next`
- `hex.find_previous`
- `hex.copy`
- `hex.copy_as_hex`
- `hex.copy_as_text`

Their titles go in a new `crates/fe-gpui/l10n/en-US/hex.ftl` (`command-hex-…`), listed in `crates/fe-gpui/src/l10n.rs`.

Add bindings to `crates/fe-keymap/keymaps/fastedit.toml`, in context `HexViewer` only, as the image viewer's zoom keys are:

- `secondary-f`: `hex.find`
- `ctrl-g`: `hex.go_to_offset`
- `secondary-c`: `hex.copy`
- the keys `find.next` and `find.previous` use in `editor`: `hex.find_next` and `hex.find_previous`

Add them to the context-free binding list `crates/fe-gpui/src/command_tests.rs` pins.

Navigation keys are handled in the view, as FileViewer handles them:

- arrows move one byte or one row
- shift+arrows extend the selection
- page up/down
- home/end go to the row's start or end
- secondary-up/down go to the file's start or end

Typed text does nothing: the view is read-only.

## Minimap

The user's explicit ask: the minimap is off in hex mode. The pane applies `fe_gpui::minimap_options` only to `Tab::editor()` tabs (`sync_tabs`, `reload_settings`), and the hex view isn't an `EditorView`. So the view must not grow a minimap of its own, and no `[editor.minimap]` setting may give it one. A test pins this (C12).

## Out of scope

- Byte editing.
- The byte inspector (next card).
- Screen-reader text for the rows (follow-up).
- A project-tree "Open as Hex" menu item.
- Column-width or grouping options.

## Mac

This card adds a UI audit surface (`hex_viewer`). The audit renders with the real Metal renderer, so the card needs the Mac and is not linux-ok. Everything else is headless (gpui test platform).

## Notes for the worker

- debug_selector ids: write the strings with `verbatim!`, or the R3 l10n lint flags them.
- The l10n lint must pass for every new string.
- Tests go in the new test files listed in the scope.
- Run the gate on the affected crates only: fe-fileview, fe-gpui, fe-keymap, fe-workbench.
