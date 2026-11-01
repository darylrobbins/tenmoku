---
task_uid: "01m69pbajh5734h62ct6v3dk20"
task: "T272"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "diff_rows turns a block that parse_edit_block reads as EditBody::Sections into removed rows (search lines), added rows (replace lines) and a gap between sections, as byte ranges of the block's code, and gives None for any other code. reply::tests::diff_rows_split_sections_into_removed_and_added and reply::tests::diff_rows_are_none_for_code_that_is_not_sections pass."
    required: true
    manual: false
  - id: "C2"
    text: "A finished reply's sections block draws chat-diff-\u003cturn\u003e-\u003cblock\u003e with one tinted row per line (diff_removed_row / diff_added_row, syntax colours kept) and no merge-marker line laid out. Its header keeps the path, Copy (the block as written, markers included) and Apply. Whole-file and malformed blocks stay code. a_search_replace_block_is_drawn_as_a_diff and whole_file_and_malformed_blocks_stay_code (crates/fe-chat-view/tests/edit_diff.rs) pass."
    required: true
    manual: false
  - id: "C3"
    text: "While a turn streams, its last code block is drawn as code and earlier sections blocks as diffs. Once the turn finishes, every sections block is a diff. the_block_still_streaming_stays_code passes."
    required: true
    manual: false
  - id: "C4"
    text: "The new screenshot scenes chat_diff_dark and chat_diff_light are defined in the screenshots example and show a two-section block as a diff; their macOS goldens (chat_diff_dark.png, chat_diff_light.png) are generated on the Mac stack, not on argon, and the existing goldens are unchanged. ElementOptions::code_body and MarkdownStyle::code_runs_in are documented, with style::tests::code_runs_in_cut_the_runs_to_the_range passing. cargo test -p fe-markdown-view -p fe-chat-view and clippy are clean."
    required: true
    manual: false
---

# Show a chat reply's SEARCH/REPLACE code block as a diff: each section's search lines as red removed rows and replace lines as green added rows under the file's header, with Copy and Apply kept (Mac T1262)

## Goal

A chat reply's code block that is a SEARCH/REPLACE edit is drawn as a diff rather than as raw
merge markers. Each section's search lines appear as removed (red) rows and its replace lines as
added (green) rows, under the block's usual header: language, file path, Copy, and Apply. Copy
still copies the block exactly as written (markers included), since that is what Apply and the
model use. Whole-file blocks, selection blocks without markers, and blocks that don't parse stay
code.

## Facts found

- `crates/fe-chat/src/edits.rs:66-79`: `parse_edit_block(info, code)` returns
  `EditBody::Sections(Vec<(search, replace)>)` when any line is a marker. Each text keeps its
  lines' `\n` (`:29-31`). `:128-169`: markers must form whole sections with only blank lines
  between them, otherwise `Malformed { line }`; an unclosed section is `Malformed` at its
  opening. A path outside the root is `EditBlockError::Path`.
- `crates/fe-chat-view/src/reply.rs:195-215` `reply_element(turn, document, style, streaming, …)`
  draws through `fe_markdown_view::document_element`, and `:235-284` draws the header. Copy
  copies `CodeBlock::new(info, code).code`, the block as written (`:243-250`).
  `reply.rs:91-110` `path_blocks` already calls `parse_edit_block` per block.
- `crates/fe-markdown-view/src/blocks.rs:299-350`: a code block's body is the scrolling div
  `md-code-<n>` around one `StyledText` with `style.code_runs(code, highlights)`
  (`style.rs:118`). `BlockKind::Code` carries `highlights: Vec<fe_syntax::HighlightSpan>`, byte
  ranges of `code` (`crates/fe-markdown/src/lib.rs:58-66`). `CodeHeader` (`blocks.rs:72-82`)
  doesn't carry them.
- Diff colours exist in the editor theme: `EditorTheme::diff_added_row`, `diff_removed_row`
  (and the `_word` tints) at `crates/fe-gpui/src/lib.rs:154-158`. `view.rs:915` builds the reply
  style from `self.composer.read(cx).theme()`.
- While a turn streams, an unclosed fence is code to the end (`reply.rs:132-134`), so only the
  last code block in document order can still be growing. Earlier ones are closed and final.

## Changes

**fe-markdown-view** (`src/blocks.rs`, `src/style.rs`, `src/lib.rs`):
- `ElementOptions` gains `code_body: Option<CodeBodyFn>`, where
  `CodeBodyFn = Rc<dyn Fn(CodeBody<'_>, &mut Window, &mut App) -> Option<AnyElement>>`.
  `CodeBody` holds the block's `index`, `info`, `language`, `code` and `highlights`. `Some`
  replaces the code text inside the block's scrolling body: the body keeps its sideways scroll
  and vertical padding, but drops its horizontal padding so the host's rows can be tinted edge to
  edge. `None` draws the code as today.
- Add `MarkdownStyle::code_runs_in(code, highlights, range) -> Vec<TextRun>` (`src/style.rs`),
  the runs for `code[range]` alone. With it a host keeps syntax colours on lines it lays out
  again, and fe-chat-view doesn't need a fe-syntax dependency to slice `HighlightSpan`s.

**fe-chat-view** (`src/reply.rs`, `src/view.rs`):
- A pure `pub(crate) fn diff_rows(code: &str) -> Option<Vec<DiffRow>>`, where
  `DiffRow { kind: Removed | Added | Gap, range: Range<usize> }`:
  - It returns `Some` only when `parse_edit_block("", code)` gives `EditBody::Sections`. The
    empty info string means a bad `path=` doesn't stop the diff; the header still shows the
    path.
  - It returns each search line as `Removed` and each replace line as `Added`, as byte ranges of
    the block's `code`, so highlights and Copy keep using the original text.
  - It puts a `Gap` row between sections.
  - A section whose search or replace is empty shows only the other side.
- `reply_element` passes a `code_body` that draws those rows:
  - The rows use the block's highlight runs, a `-`/`+` sign column, and full-width tints
    `diff_removed_row` / `diff_added_row`, taken from the editor theme that `view.rs` passes in.
  - Debug selectors are `chat-diff-<turn>-<block>` on the body and
    `chat-diff-<turn>-<block>-removed-<k>` / `-added-<k>` on each row (k counted per kind).
  - Rows scroll sideways together as one body.
- While the turn streams, the reply's last code block is always drawn as code, so a block that
  is still arriving never flips between code and diff. Every earlier block that parses as
  sections is drawn as a diff at once. A finished turn draws every sections block as a diff.
- The header is unchanged (path, Copy of the block as written, Apply). The thinking element
  passes no `code_body`.

**Goldens**: add a scene to `crates/fe-chat-view/examples/screenshots/scenes.rs`, a finished
reply with a two-section ```` ```toml path=deny.toml ```` block, dark and light, with new golden
files `chat_diff_dark.png` / `chat_diff_light.png`. Existing goldens stay unchanged. Generate
them on macOS (Metal).

## Tests

- Unit tests in `reply.rs`'s `mod tests`: `diff_rows_split_sections_into_removed_and_added`
  (two sections: the row kinds and ranges, the gap, an empty replace, a trailing line without a
  `\n`) and `diff_rows_are_none_for_code_that_is_not_sections` (a whole file, unclosed markers,
  stray text between sections).
- New `crates/fe-chat-view/tests/edit_diff.rs` (top-level fns, harness as `tests/markdown.rs`,
  recording text system):
  - `a_search_replace_block_is_drawn_as_a_diff`: a finished reply with a two-section
    ```` ```toml path=deny.toml ```` block draws `chat-diff-0-0` with as many removed and added
    rows as the sections have lines. No line equal to `<<<<<<< SEARCH`, `=======` or
    `>>>>>>> REPLACE` is laid out. The header still lays out `deny.toml`. `chat-copy-0-0` copies
    the block with its markers, and `chat-apply-0-0` emits `ApplyBlock { turn: 0, block: 0 }`.
  - `whole_file_and_malformed_blocks_stay_code`: neither draws a `chat-diff-*`, and both draw
    `md-code-<n>`.
  - `the_block_still_streaming_stays_code`: in a streaming reply, an earlier closed sections
    block is a diff while the last block (one complete section, fence still open) is code. Once
    the turn finishes, both are diffs.

## Conflicts

- Depends on the pinned-header card, which depends on the sideways-scroll card. All three edit
  `blocks.rs` and `reply.rs`.
- The block-outcome card comes after this one (same header).
- T1252 (blocked, lent) retunes `diff_*_row` / `diff_*_word` theme values. This card only reads
  them; the goldens would move if T1252 lands first, so regenerate them then.
- T1235 (blocked) holds `crates/fe-chat-view/tests/**`.

## Implementation notes

- `CodeBody` passes the block's code as the fence holds it, trailing `\n` included, as
  `CodeHeader` does, so `diff_rows`' ranges and the highlights index the same text. A row's range
  leaves out its `\n` (and a `\r` before it). A gap row is an empty range where the next section's
  `<<<<<<< SEARCH` starts, drawn one code line tall and untinted.
- `MarkdownStyle::code_runs` is now `code_runs_in(code, highlights, 0..code.len())`, so the code
  path's runs are unchanged; `style::tests::code_runs_in_cut_the_runs_to_the_range` covers the
  slicing.
- `reply_element` takes the tints as one more argument (`DiffTints`, from the composer's editor
  theme in `view.rs`), with `#[allow(clippy::too_many_arguments)]` as elsewhere in the repo.
- `tests/edit_diff.rs`: frames are drawn while a scripted reply streams in, so the block is laid
  out as code before the turn finishes, and the window's line cache then skips lines drawn again.
  `a_search_replace_block_is_drawn_as_a_diff` therefore clears the recorded lines and bumps the
  editor font size by 2 (the code is laid out again at the new size) before asserting that no
  marker line is laid out.
- Goldens: `chat_diff_dark` / `chat_diff_light` are scenes in `examples/screenshots/scenes.rs`
  (a `Chat::Diff` scene kind; the existing `Chat::Transcript` scenes are drawn exactly as before).
  As with T250, they can't be rendered here: this lane ran on argon (Linux), and
  `VisualTestAppContext` needs Metal. The scenes were type-checked by building the example with
  its macOS `cfg` forced on, where the only error was the Metal-only `VisualTestAppContext`
  import. Generate the two PNGs on the Mac after the sync with
  `UPDATE_SCREENSHOTS=1 cargo run -p fe-chat-view --example screenshots`, and check that
  `chat_dark` / `chat_light` still match.
- Rev 2: the goldens `chat_diff_dark.png` / `chat_diff_light.png` were rendered with Metal on the
  Mac stack and committed to the lane (eec13029). A skip-when-missing workaround tried in review
  round 2 was reverted (544426c9). `chat_dark.png` / `chat_light.png` are unchanged.
