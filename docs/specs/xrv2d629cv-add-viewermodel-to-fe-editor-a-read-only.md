---
task_uid: "01m4fkss2jgnk63pxrv2d629cv"
task: "T273"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test viewer::moves_like_the_editor: on texts with multi-byte chars, tabs and empty rows, 500 random sequences of the supported move and select commands leave ViewerModel over Text with the same selection as EditorModel over a Buffer of the same text"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test viewer::copies_a_file_text: over a FileText of a CRLF file with invalid bytes, select_all then copy_text gives the rows joined by \\n with each invalid byte as '?', and a selection longer than COPY_LIMIT returns CopyError::TooLarge with the view's cached_bytes() unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test viewer::refuses_edits: run returns Outcome::ReadOnly for editor.save and for every command the registry marks mutates_buffer, with selection and text unchanged after each; refresh with a longer snapshot keeps the selection"
    required: true
    manual: false
---

# Add ViewerModel to fe-editor: a read-only editor over a TextSource with one selection, the editor's moves, select-all and capped copy, refusing every edit

## Problem

<T-display> lets `DisplayMap` lay out a read-only `TextSource`. Nothing in fe-editor drives one yet. `EditorModel`
(`crates/fe-editor/src/lib.rs`) is built on a `Buffer`: its selections are anchors in a `BufferSnapshot`, and its
commands edit the rope. `docs/decisions/mmap-read-only.md` (T6) says that in the read-only mode "Selection, search
and copy work against the trait; every mutating command is disabled". T241's spec has fe-editor take the trait.

Making `EditorModel` generic over a source would touch most of its 1,559 lines. Ready cards T145 and T147 change
that file too. A read-only file needs no anchors, because its offsets never shift. So this card adds a separate,
small model that reuses the editor's command IDs.

## Change

Add `crates/fe-editor/src/viewer.rs` with `ViewerModel`, exported from `crates/fe-editor/src/lib.rs` (a `mod` line
and a `pub use` line only). It is GPUI-free like the rest of fe-editor.

- `ViewerModel::new(text: Arc<dyn TextSource>, tab_size) -> ViewerModel` builds `DisplayMap::read_only`. There is
  one selection, `anchor` and `head` as plain offsets, and a goal char column for vertical moves.
  `selection() -> Range<usize>` and `head() -> usize` read it, and `resolved_selections() -> Vec<ResolvedSelection>`
  gives it in the form `layout_frame` takes.
- `refresh(text)` takes a later snapshot of the same source through `DisplayMap::replace_text`, and clamps the
  selection to the new length.
- Movement uses the editor's `Movement` meanings and units (`crates/fe-editor/src/lib.rs`): left and right by char
  (clipped), up and down keeping the char column within the display row, line start and end, page up and down by
  `page_rows`, document start and end, each with or without extending the selection. Also `select_all()`, and
  `set_selection(anchor, head)` for the mouse.
- `copy_text() -> Result<String, CopyError>` returns the selected text with `\n` breaks. A selection longer than
  `COPY_LIMIT` (64 MiB) is `CopyError::TooLarge`, and nothing is read.
- `run(command_id, args) -> Outcome` runs the editor's command IDs a viewer supports, as registered in
  `crates/fe-editor/src/commands.rs`: the `editor.move_*` and `editor.select_*` movements, `editor.move` with its
  args, `editor.select_all` and `editor.copy`. The word moves (`move_word_left`, `move_word_right` and their
  selects) return `Outcome::Unsupported` for now.
  It returns `Outcome::ReadOnly` for every command registered with `mutates_buffer(true)` and for `editor.save`
  (registered without the flag). Any other ID is `Outcome::Unsupported`.

The view that paints it is the next card.

## Tests

In the new file `crates/fe-editor/tests/viewer.rs`. Add `fe-fileview` as a dev-dependency of
`crates/fe-editor/Cargo.toml`, and `fe-text` as a dependency if the trait isn't reachable through `fe-buffer`.

## Implementation notes

- Left and right move by char (`clip_offset` either side), where `EditorModel` moves by grapheme; they agree
  wherever each grapheme is one char, which is what `moves_like_the_editor` generates. Columns for up and down
  are counted by walking only the chunks up to the column, never a whole row, so a long row of a mapped file
  costs what the column does.
- `run(id, &serde_json::Value)` returns `Changed`/`Nothing` for moves, `Copy(Result<String, CopyError>)` for
  `editor.copy`, `BadArgs` for `editor.move` args that don't parse, and `ReadOnly`/`Unsupported` as specified.
  The mutating IDs come from `commands::registry()`, built once. Page moves use `set_page_rows` (20 by default).
- An empty selection copies `""`, not the caret's row as `EditorModel::copy_text` does.
