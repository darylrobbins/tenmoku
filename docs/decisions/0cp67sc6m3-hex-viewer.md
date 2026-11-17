# Hex viewer: design calls (T1895)

The view is `crates/fe-gpui/src/hex_viewer.rs`; its module docs are the
reference. These are the calls the spec left to the worker.

- **Selection is whole bytes, at least one.** The anchor and head are byte
  offsets, both inclusive, so the head byte is always selected (an empty file
  selects nothing). That is how hex editors behave, and it makes "the offset"
  in the status bar the head byte. Copy of a one-byte selection copies that
  byte.
- **Where Find Next starts.** Forward starts past the selection when the
  selection is the match found last or more than one byte, else at the head
  byte, so a one-byte caret finds a match on itself; backward ends before the
  selection. A query typed in the input searches forward from the
  selection's first byte, so typing on over a match keeps it. Closing the input
  stops a running search but keeps the query: `hex.find_next` and
  `hex.find_previous` still step through it from the rows.
- **Matches don't overlap.** As in the editor's find, the next match starts
  at or past the current one's end, so `aa` in `aaa` finds 0..2 only; a
  selection of several bytes (a drag) is searched past, like a match.
- **A BOM file is text, whatever follows.** A file starting with `FF FE`,
  `FE FF` or `00 00 FE FF` opens as it did before, as the spec says, even if
  the bytes after the mark aren't valid UTF-16 or UTF-32; Open as Hex shows
  it in hex.
- **Copy in an input copies nothing.** The copy keys are bound in
  `HexViewer`, which the header's inputs are inside of, and the text input
  has no copy of its own; copying the rows from there would surprise.
- **The inputs reuse the find bar's key context.** The header's inputs sit in
  an element with key context `find_bar`, so the existing bindings give them
  Enter (`find.next`), Shift-Enter (`find.previous`), Escape (`find.close`),
  `find.toggle_case` and the text input's editing keys. In Go to Offset,
  Enter goes. No new bindings were needed beyond C13's, and the editor's
  keys are untouched. In the app the find commands are the workbench's typed
  commands, handled at the pane's root, so `Pane::run_find` hands them to a
  hex viewer whose input has focus (`HexViewer::run_input_command`) before
  its own find bar sees them; fe-gpui alone, and the palette, send them as
  `RunCommand`s, which the view takes too (review round 1, F1).
- **Rows scroll sideways in a narrow pane.** A row is 76 or 84 chars; a pane
  narrower than that scrolls it with the wheel's horizontal delta, clamped to
  the row's end, and keys bring the head's cell into view. The header's input
  and its message shrink, so Text and Reload stay in the pane (F2).
- **What a search said shows with the input closed.** Wrapped and Not found
  go in the header's info while no input is open, until the selection is
  moved by hand (F5). The current match's outline clears with a new query and
  on Not found (F4); opening Go to Offset stops the Find input's search (F3).
- **Go to Offset closes on success** and focuses the rows; an error keeps it
  open with the reason beside it.
- **Colours.** Focused column: `selection`; the other: `identifier_highlight`;
  the current match: a 1 px `find_match` outline (a fill would hide under the
  selection, which lands on the match); the caret: a 2 px `cursor` underline
  under the head byte in the focused column while the rows have focus. The
  scrollbar thumb takes kiln-ui's existing `scrollbar.thumb` colours. No new
  tokens.
- **Scrollbar.** The view draws its own 8 px (`fe_ui::SCROLLBAR_WIDTH`) strip
  right of the rows, driven by the `u64` top row (kiln-ui's `ScrollArea`
  scrolls by one pixel offset, which multi-GiB files overflow). A press off
  the thumb centres it there.
- **The header** shows the file's length when nothing else does, the open
  input, the banner while the file changed, Reload, and the Text button.
- **Open as Hex opens synchronously.** `FileView::open` is an `open` and an
  `fstat`, no read, so it runs on the main thread as the image viewer's
  construction does; a failure is a toast.
- **Open as Text of a large binary** hands the hex viewer's own `FileView` to
  the text viewer with a new index (`loader::index`), so nothing is read twice.
- **A copy over the limit** is a `HexViewerEvent::CopyTooLarge`, which the
  pane shows as a toast (`workbench-pane-hex-copy-too-large`).
- **`READ_ONLY_STATUS` moved to fe-gpui's `lib.rs`.** `file_viewer`'s
  `pane_item` module is private to it, and `file_viewer.rs` isn't in scope, so
  the constant lives at the crate root and `file_viewer/pane_item.rs` uses it
  from there.

## Edits outside the listed scope

- `crates/fastedit/src/main.rs` and `crates/fe-workbench/src/workbench.rs`
  each match `loader::Opened` exhaustively (the launch file), so the spec's new
  `Opened::Binary` needs one arm in each: a binary launch file opens in a hex
  tab (`Pane::add_hex_viewer`).
- `crates/fe-workbench/src/git_live_tests.rs`'s
  `binary_new_files_have_no_base` opened two binary files (`a\0b…`) and
  expected editors. They open in hex viewers now (C10), so the test opens them
  with `Pane::open_as_text`, keeping what it checks: an editor of a binary
  file has no Git base.

## Audit capture

`hex_viewer_{dark,light}.png`: the header strip at y 97–122, row 0x10 at
y 145–165, its hex cells in `selection` and its ASCII cells in
`identifier_highlight`; the status bar reads `Offset 0x10–0x1F, 16 bytes`.
The full run (`cargo run -p fe-workbench --example screenshots -- audit
<dir>`) wrote 81 captures and exited 0 on the Mac.
