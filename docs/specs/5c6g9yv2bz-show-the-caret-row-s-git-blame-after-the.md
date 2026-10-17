---
task_uid: "01m4xqgkypnhp2qd5c6g9yv2bz"
task: "T127"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_blame_tests::annotation: with line blame toggled on, the caret's row shows '\u003csummary\u003e, \u003cauthor\u003e (\u003cn\u003e days ago)' as an AfterLine decoration matching git blame for it; moving the caret with 'j' and with '/pat' moves the annotation after the rest; an edited row shows 'Not Committed Yet'; toggling off removes it"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_blame_tests::ex_and_popover: ':Gitsigns toggle_current_line_blame' and ':Git blame' toggle it; ':Gitsigns blame_line' shows a popover whose text has the author, email, date and full message (EditorView::popover_text)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_blame_tests::open_commit: git.open_line_commit on a row from commit C opens a read-only DiffView of the file at C's parent against the file at C; on a root commit the left is empty; on an uncommitted row HEAD ↔ Working Tree"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_blame_tests::cheap: holding the caret still re-blames nothing; 10 caret moves within 150 ms update the annotation once; an untracked file or one outside a repo shows nothing; cargo test -p fe-workbench -p fe-gpui passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show the caret row's git blame after the line, a popover with the commit on hover or :Gitsigns blame_line, and open the commit's change to the file (Mac T555)

## Problem

The user (2026-10-12): "per-line blame for the current file (toggleable gutter annotation or inline end-of-line on the
caret line) with commit summary/author/date on hover; open the commit's diff." The vim-mode requirement (coordinator,
same day): toggled by an ex command (`:Git blame`-style), following the caret as vim motions move it. `Repo::blame`,
`text_at` and `parent` exist (the blame card).

Choice: the inline end-of-line form on the caret row (VS Code's `git.blame.editorDecoration`, Zed's inline blame), drawn
with the editor's existing `DecorationKind::AfterLine`; a gutter blame column would need a new gutter lane in
fe-display (not in this card).

## Change

In `crates/fe-workbench/src/git_blame.rs` (replacing its stubs), and a host popover call in `crates/fe-gpui/src/hover.rs`:

- `git.toggle_line_blame` (`:Gitsigns toggle_current_line_blame`, and `:Git blame`, which in fugitive opens a blame
  split; here it toggles, stated): off by default, per window. On, the active editor's caret row shows, after its
  text, VS Code's template `<summary>, <author> (<time ago>)` in `Muted`; an uncommitted row `Not Committed Yet`. It
  follows the caret however it moves (vim motions, search, clicks), in every mode, after 150 ms of rest so held `j`
  doesn't flicker; it hides while the row has a selection spanning rows.
- Blame comes from `Repo::blame(path, buffer text)` on the background executor, cached per buffer version and HEAD;
  edits re-blame 300 ms after the last one; a file outside a repository, untracked or over 5 MiB shows nothing.
- Hover: resting the pointer on the annotation, or `git.blame_line` (`:Gitsigns blame_line`, at the caret), shows a
  popover: `**<author>** <email>`, the absolute date and time ago, the full message, and the short id; added in
  hover.rs as `EditorView::show_popover(HoverContent)` so a host can show one at the caret, beside the hover
  provider's.
- `git.open_line_commit` (`:Gitsigns show_commit`): a DiffView of the caret row's commit for this file, left
  `<short>^` (`text_at(parent)`, empty for a root commit), right `<short>` (`text_at(commit)`), both read-only, caret
  on the blamed row; on an uncommitted row it opens HEAD ↔ Working Tree.

## Tests

New file `crates/fe-workbench/src/git_blame_tests.rs` (declared `#[cfg(test)]` from git_blame.rs).

Lead decision after review round 3: fix F9 and F10 here; they are what C3 needs. F9: decode Git's quoted pathname
format (octal escapes, quotes, backslashes) in porcelain `filename` and `previous` before joining them, so a commit's
change to `café.rs` or a renamed path opens the right diff on both sides. F10: resolve a row's origin with the same
newline normalization and buffer-to-HEAD mapping as `Repo::blame`, so a committed CRLF file (core.autocrlf=false)
opens its commit, not HEAD against the working tree. Test both (Unicode, quotes, backslashes, a rename; a CRLF
fixture). F2 and F4 (supersession races: the Save As round trip, HEAD validation while blame is pending, an obsolete
diff open stealing focus, the pane-focus round trip) are deferred to follow-up T135. Don't change that behaviour
further in this task.

## Implementation notes

- State lives in a `BlameState` global in `git_blame.rs` (as `git_diff`'s `DiffState` does), so `workbench.rs` is
  untouched: per window (by workbench entity), the annotation provider registered on the active editor's model, the
  caret last seen, the rest and blame tasks; per buffer, the last blame with its version and HEAD's id.
- Following the active editor without a hook in the workbench's render: line blame observes the workbench, its status
  bar (which `follow_active_editor` updates on every render that changes the active item) and the annotated editor.
- HEAD: the Changes panel notifying (a git refresh landing) reads HEAD's id in the background; only a moved HEAD
  blames again, so a still caret blames nothing.
- Untracked is "in neither HEAD nor the index" (`head_blob_size` / `index_blob_size`): `Repo::blame` itself calls such
  a file one uncommitted run. An added (staged) file shows `Not Committed Yet`.
- `:Git blame` and `:Gitsigns toggle_current_line_blame` answer nothing, as gitsigns' toggle doesn't.
- hover.rs: `EditorView::show_popover(HoverContent)` shows the host's text at the caret; for the pointer,
  `EditorView::set_after_line_popover(Some((label, content)))` shows `content` once the pointer rests on a drawn
  after-line decoration whose text is `label` (hit-tested against the frame's placed decorations), and hides it off
  the label. Neither asks the hover provider. Tests: `hover::host_popover_tests`.
- `git.open_line_commit` asked again for the same commit and file shows the tab already open. The commit, the row there
  and the file's paths at the commit and its parent come from `git blame --porcelain -L r,r --contents -` on the
  buffer (`blame_row`): git traces the row through renames and duplicate rows, which neither `Repo::blame` (no source
  rows or paths) nor a diff of two texts can.
- `now`/`ago` are copies of `git_branch`'s private helpers (that module is outside this card's scope); a later card
  can share them.
- `git_commands_tests::ex_routes` asserted `Not supported yet: Git: Toggle Line Blame` for `:G blame` and
  `:Gitsigns toggle_current_line_blame`; with the toggle implemented it answers nothing, so that test's expectation
  changes (scope widened to `crates/fe-workbench/src/git_commands_tests.rs` for it).

### Review round 1 (h96arf)

- F1: each marked editor gets its own `Annotation` provider. `EditorModel::split` copies registrations, so `look`
  removes the marked provider from the window's other editors sharing the buffer, and unmarking empties the provider
  for good, so any copy elsewhere (another window, a diff's source model) shows nothing. Test: `splits_show_one`.
- F2: a kept blame is valid for its buffer, path and version; marking an editor whose blame is kept reads HEAD again,
  and a kept "nothing to show" (untracked, no repository) is blamed again on every git refresh. `git.blame_line` and
  `git.open_line_commit` blame afresh. Test: `head_moves` (HEAD amended while line blame is off).
- F3: `commit_texts` reads the file at the commit and its parent by its paths there, from `Repo::file_log` (which
  follows renames) when the current path is missing. Test: `renamed_and_shifted`.
- F4: both commands land only when they're the last asked for and the editor is still active with the same text and
  caret row. Test: `superseded_asks`.
- F5: a HEAD check lands only if the same editor is still marked with an unchanged buffer and no blame pending, and
  marking another editor cancels it. F6: each blame carries a serial; a result older than the kept one (an older
  version, or an earlier blame of the same version) isn't kept. Neither has a timing test: the test executor can't
  reorder these background results deterministically.
- F7: `show` hides the annotation and its popover when the current blame has nothing for the row. Test: `head_moves`
  (untracked by a commit, the caret still).
- F8: see the row diff above. Test: `renamed_and_shifted` (duplicate rows shifted ten down).

### Review round 2 (znvk62)

- F2: a pending blame carries its file's path, and marking another editor cancels it, so a Save As during it blames
  the new path and a cached tab switched to reads HEAD without waiting behind another file's blame. No Save As test:
  the tests have no Save As entry point outside the workbench's save flow.
- F3, F8: `blame_row` (above). Tests: `renamed_over_another_file` (the side shown is the file the row was in at the
  traced commit), `duplicate_rows_traced` (the reviewer's start/dup/x/dup/end example, new and reused tab), and
  `renamed_and_shifted`.
- F4: an ask also records the active pane and its navigations and opens, and its editor's path; `stands` drops a
  result after any of them changed. Test: `asks_dropped_after_navigation`. For a row not committed yet the ask hands
  off to `git.diff_head` once it stands; that diff's own landing (`git_diff`'s `may_show`) guards its focus against
  later navigation, as for a diff asked for from the palette.

### Review round 3 (p0ytzf), per the lead's decision

- F9: `unquote` decodes git's quoted pathnames (C escapes and octal bytes, kept as bytes) in porcelain `filename` and
  `previous`; the stdout is parsed as bytes. Tests: `quoted_and_renamed_paths` (an accent, a quote and a backslash,
  changed then renamed: both sides, and the root commit's), `unquotes_git_paths`.
- F10: `read_origin` matches the buffer's row to HEAD's with `head_row`, the same Histogram row diff with `\r\n` read
  as `\n` that `Repo::blame` uses, then runs `git blame --porcelain -L r,r HEAD` on HEAD's blob rather than the buffer
  on stdin, so line endings no longer matter; a row the buffer changed is uncommitted. Test: `crlf_file`
  (`core.autocrlf=false`).
- F2 and F4 are deferred to T135, unchanged here.

### Review round 4 (q3e96y)

- F11: `head_row` diffs exactly `Repo::blame`'s rows (`blame_rows`: split at `\n`, the empty row after a final `\n`
  left out, one trailing `\r` per row dropped, at EOF too), joined by `\n` for `TextDiff`. A `\r` left within a row is
  replaced by a control byte that neither text contains, so rows compare as blame compares them (no aliasing) and no
  text grows past `TextDiff`'s 5 MiB limit; a too-large diff gives no row. Git's row in the commit's text is converted
  to the diff side's (`side_row`), counting `\r\n`, a lone `\r` and `\n` as the editor's normalization does.
  Tests: `lone_carriage_return`, `rows_matched_as_blame_matches_them` (EOF `\r`, the terminal empty row, a literal
  U+F8FF, 2 MiB of `\r`s under the limit).
- F2 and F4: disputed (`rgt review dispute`) as deferred by the lead to T135; unchanged.

### Review round 5 (8630tk)

- F2 and F4 withdrawn on dispute (deferred to T135).
- F11: made exact, above. F12: `side_row` counts a `\r` as a row break only when no `\n` follows it. Tests:
  `side_rows_split_as_editors_do`, `repeated_carriage_returns` (new and reused tab).

### Review round 6 (5vrvjs)

- F11: no absent byte is needed now. Rows without a `\r` within them are used as they are; each distinct row with one
  stands in as its `\r`s made `\u{1}`, lengthened with more `\u{1}`s until it equals no other row of either text and
  no other stand-in (`blame_rows`), so rows compare exactly as `Repo::blame` compares them. `head_row` refuses what
  `Repo::blame` refuses, judged on the texts as they are: over 5 MiB, or a NUL in the first 8000 bytes. Tests in
  `rows_matched_as_blame_matches_them`: every control byte present, a row reading as another's stand-in, a CRLF HEAD
  over 5 MiB whose LF buffer is under it, a binary text.

### Review round 7 (9mw7fb)

- F13 (and F11 for good): `head_row` no longer goes through `TextDiff`, whose tokenizer splits at a `\r`, so any
  lossless re-encoding of blame's rows for it can grow a text past its 5 MiB limit. It now runs exactly what
  `Repo::blame` runs: imara-diff's Histogram with `postprocess_lines` over the same byte rows (`Rows`/`rows`, as in
  `fe-git/src/blame.rs`), behind the same 5 MiB and binary checks on the texts as they are. The stand-in encoding of
  rounds 5 and 6 (`blame_rows`) is gone. This adds `imara-diff` (already a workspace dependency of fe-git, fe-editor
  and fe-buffer) to fe-workbench: scope widened to `crates/fe-workbench/Cargo.toml` and `Cargo.lock` (one line each).
  Test: `rows_matched_however_many_carriage_returns` (the review's reproduction).

