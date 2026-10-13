---
task_uid: "01m4t6fwrg5hy77t2fg37aezj1"
task: "T84"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-terminal test link_marks::url_keeps_combining_marks: link_at on every cell of the URL in \"see https://x.test/cafe\\u{301}/menu\" (the mark in the e cell extra) gives LinkTarget::Url(\"https://x.test/cafe\\u{301}/menu\") over cols 4..28"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal test link_marks::url_ending_in_combining_mark_keeps_it: link_at on \"https://x.test/e\\u{301}\" gives LinkTarget::Url(\"https://x.test/e\\u{301}\") over the whole row"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal test link_marks::path_with_combining_mark_links_to_existing_file: with a temp cwd holding cafe\\u{301}.rs, every cell of \"cafe\\u{301}.rs:3:7\" in \"--\u003e cafe\\u{301}.rs:3:7 here\" gives LinkTarget::File { path: cwd/cafe\\u{301}.rs, line: Some(3), column: Some(7) } over cols 4..15"
    required: true
    manual: false
  - id: "C4"
    text: "Link cell ranges are unchanged: the existing fe-terminal links tests pass unchanged, and cargo test -p fe-terminal and cargo clippy -p fe-terminal --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Keep combining marks in terminal link targets, so a URL or file path with a decomposed accent opens what the screen shows (Mac T634)

Filed on argon from Mac T634 (card ~/coordination/argon-cards/T634.json rev 1). Mac task uid 01m4t67qznwrxb45ep56h5e3w2. No dependencies. Title the host commit "... (Mac T634)".

## Problem

`RowText::new` in `crates/fe-terminal/src/links.rs` (lines 75-91) builds a row's text from each cell's
`ch` only and drops `cell.extra`, where a cell keeps its combining marks. Every link target built from
that text loses the marks: the URL string (`url_at`, links.rs:116) and the path candidate
(`existing_path`, links.rs:302). `Snapshot::row_text` (snapshot.rs:42) does include `extra`, so the
text a link opens differs from the text on screen.

Checked 2026-10-13 at e6988aba with a one-row snapshot, the mark stored as the previous cell's `extra`:

- `"see https://x.test/cafe\u{301}/menu"`, `link_at` col 4 gives
  `Url("https://x.test/cafe/menu")`, not `Url("https://x.test/cafe\u{301}/menu")`: cmd-click opens a
  different URL.
- `"https://x.test/e\u{301}"`, col 0, gives `Url("https://x.test/e")`.
- In a cwd holding `cafe\u{301}.rs` (a decomposed name, as macOS tools often print it), the row
  `"--> cafe\u{301}.rs:3:7 here"` at col 4 gives `None`: the lookup tried `cafe.rs`, which doesn't exist.

## Change

In `crates/fe-terminal/src/links.rs`:

- Give `RowText` a `marks: Vec<Option<String>>` parallel to `chars`, filled from `cell.extra`, and a
  `fn text(&self, span: Range<usize>) -> String` that writes each character followed by its marks.
- Pass `&RowText` to `url_at`, `path_at` and `existing_path` (the run as a `Range<usize>`). They keep
  scanning and classifying on the base `chars`, so `url_end`, `is_path_char` and `location` are
  unchanged, but build the URL and the path strings with `text.text(..)`.
- Cell ranges (`Link::cols`) are unchanged. No public API change.

## Tests

New file `crates/fe-terminal/tests/link_marks.rs`. Build one-row snapshots from a string, one cell per
base character, putting each combining mark (U+0300-U+036F) into the previous cell's `extra`; use
`tempfile` (already a dev-dependency) for the path case. The pinned regression tests are
`url_keeps_combining_marks`, `url_ending_in_combining_mark_keeps_it` and
`path_with_combining_mark_links_to_existing_file`; all three fail at the base.
