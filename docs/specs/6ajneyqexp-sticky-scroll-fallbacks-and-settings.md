---
task_uid: "01m5f7g1f8xbe8zt6ajneyqexp"
task: "T1031"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fold_scopes on nested JSON objects returns their header..=last rows, outermost first, walking only the tree over the rows (crates/fe-syntax/tests/sticky_fold_scopes.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "indent_scopes on an indented plain-text file gives the blocks enclosing the rows, and a scope still open past rows.end comes back without scanning to the end of a 1M-row file (sticky_indent_tests.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "Plain text and a file over its parse limit get indentation rows; a language with a tree but no outline query gets folding rows; default_model = \"indentation\" forces indentation in a Rust file"
    required: true
    manual: false
  - id: "C4"
    text: "max_line_count = 2 caps the band at two rows live (range 1-20, default 5); default_model accepts outline, folding or indentation; template and schema list all three"
    required: true
    manual: false
  - id: "C5"
    text: "scroll_with_editor = false keeps the band's text at column 0 when the editor scrolls right; true (the default) scrolls it; line numbers never move"
    required: true
    manual: false
---

# Sticky scroll fallbacks and settings: folding and indentation models after the outline, plus editor.sticky_scroll max_line_count, default_model and scroll_with_editor applied live

# Sticky scroll 4/4: folding and indentation models, max_line_count, default_model, scroll_with_editor

VS Code's `editor.stickyScroll.defaultModel` is `outlineModel`, falling back to the folding
provider, then to indentation, when a model has nothing for the file. Add both fallbacks and the
remaining settings to `[editor.sticky_scroll]` (card 2 made the table).

## Models

- **Folding**: `pub fn fold_scopes(syntax: &Syntax, rows: Range<u32>) -> Vec<RangeInclusive<u32>>` in
  `crates/fe-syntax/src/fold.rs` (exported from `lib.rs`): the fold candidates (the ones the fold
  gutter uses, `for_each_candidate`) overlapping `rows`, header = the candidate's first row,
  last = its last row, one per header row, outermost first. Only the part of the tree over `rows`
  is walked.
- **Indentation**: `pub fn indent_scopes(text: &Text, rows: Range<u32>, tab_size: usize) ->
  Vec<RangeInclusive<u32>>` in `crates/fe-display/src/sticky.rs`, with fe-editor's indented-block rules
  (blank rows belong to a block, trailing ones don't): headers found by scanning up from
  `rows.start` for rows of lower indentation, ends found scanning down no further than
  `rows.end + 1` (a scope still open there ends past the view; that is all the push-up needs). It
  never scans the whole file from a frame.
- Choice: `outline` when the language has an outline query and the text a tree; else `folding`
  when it has a tree; else `indentation` (plain text, unknown languages, files over the parse
  limit). `default_model = "folding"` starts at folding, `"indentation"` uses indentation only.
  An outline that simply has no scope at the top doesn't fall back.

## Settings (`StickyScrollSettings`, template and schema)

- `max_line_count`: 1-20, default 5: the band's most rows.
- `default_model`: `"outline"` (default), `"folding"` or `"indentation"`.
- `scroll_with_editor`: default true: the band's text scrolls horizontally with the editor's; false
  keeps it at column 0 (line numbers never scroll).
- All three apply live to open editors (kept by `EditorTheme::set_colors`, set by `set_metrics`).

## Implementation notes

- **Upward scan limit.** Scanning up row by row cost 194 ms (release) from row 500,000 of a
  file whose only header is row 0, too slow for a frame, and indentation is the model for files
  over their parse limit, which are exactly the long ones. `indent_scopes` therefore reads row
  indents in one forward pass over a window's text (skipping each row's rest once its indent is
  known), in windows of 64, 256, 1,024 … rows up, and stops after `INDENT_SCAN_ROWS` = 50,000
  rows (about 0.4 ms). A header further above than that, with nothing less indented between, isn't
  pinned; the inner ones still are. Tested by `headers_are_found_a_window_at_a_time_up_to_the_scan_limit`.
- **Blocks open at the bound.** Row `rows.end` is the last row read. A block still open on the
  last text row read ends at `rows.end`; a row heading nothing yet (the last text row itself) isn't
  a scope. When the rows past the last text row are blank, the real block may end earlier, which
  the band only shows if its push-up reaches that far below the view's top.
- **Closed folds** (review round 1): the model is asked for each run of buffer rows the band's
  display rows show, not one range from the first to the last, so the rows a closed fold hides
  between them aren't read: outline and folding get exact scopes from each run, merged one per
  header. Indentation ends a block still open at a run's end on the fold's first hidden row, so
  it ends with the fold header, unless the run after the fold finds it still open there (its
  upward scan crosses the hidden rows, up to `INDENT_SCAN_ROWS`) and gives its real end. A fold
  hiding more rows than that keeps the headers above it from the rows below it, as a block that
  long does without a fold. Tested by `a_closed_fold_in_the_band_rows_isnt_read_for_indentation`
  (a million hidden rows) and `..._for_folding`, which check the rows asked and the rows pinned.
- **Model choice** is in `sticky_scroll::model` (fe-gpui): a syntax that failed or is over its
  parse limit counts as no tree. The cache key now holds the model (indentation with its tab
  size), so changing `default_model` or `tab_size` finds the scopes again.
- **`max_line_count`** is clamped to 1-20 where it's read, as well as bounded in the schema. The
  rows the scopes are found for grow with it (top row to top + max + 1).
- **`scroll_with_editor = false`** also shapes a long header's slice from column 0, not around the
  editor's horizontal scroll.
- `EditorTheme::sticky_scroll` is now the whole `StickyScrollSettings`, re-exported from fe-gpui.
- `sticky_scroll_tests.rs`'s "no band without scopes" case used plain text, which now pins its
  indented blocks: it uses rows with no indentation instead, and its helpers are `pub(super)` for
  `sticky_scroll_settings_tests.rs`. The screenshots example builds the theme's settings from its
  `sticky_scroll` flag.
