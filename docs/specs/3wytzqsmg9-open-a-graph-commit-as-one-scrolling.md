---
task_uid: "01m69a22nzxpnxk63wytzqsmg9"
task: "T267"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test commit_diff_view_tests::opens: 'Open Changes' on a commit row (and the commit card's file count) opens one tab '\u003cshort\u003e - \u003csummary\u003e (N files)' with a section per file; opening it again focuses that tab"
    required: true
    manual: false
  - id: "C2"
    text: "commit_diff_view_tests::header: each header shows the file icon (icon-set seam), name, dimmed dir, the status letter and +/- counts at the right; a click on the chevron or header collapses it"
    required: true
    manual: false
  - id: "C3"
    text: "commit_diff_view_tests::body: each section renders side by side with the word tints from T1218 and the hidden-lines folding from T1224; both sides of a section scroll together and the whole view scrolls vertically"
    required: true
    manual: false
  - id: "C4"
    text: "commit_diff_view_tests::navigation: the toolbar next/previous change moves across hunks and sections in order, expanding a collapsed section it lands in; clicking a file in the Graph scrolls the open view to its section"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench passes; clippy clean; if the one-view approach needs a kiln-ui change, stop and report instead of widening scope"
    required: true
    manual: false
---

# Open a Graph commit as one scrolling diff with a collapsible section per file, each header showing icon, name, dimmed directory, status letter and counts, with previous/next change across files (Mac T1228)

## Problem
The user: "Add one scrolling view as card". View half of T1227. Reuses DiffView sides per section (fe-gpui diff_view.rs) if it can embed, else a lighter read-only renderer; decide in the first hour and note the choice. Filed after T1226 so git_log.rs/git_commands.rs edits stay serial.

## Implementation notes

- **Renderer choice: a lighter read-only renderer, not embedded DiffViews.** A `DiffView` side is a
  full `EditorView` with its own vertical scroll and height; stacking one per section inside a single
  scrolling list would need fe-gpui/kiln-ui changes (content-sized editors, scroll hand-off), which C5
  rules out. `CommitDiffView` (`commit_diff_view.rs`) draws the model's rows itself in one
  `uniform_list`: a header per section, then each hunk's removed and added rows paired side by side
  (the shorter side hatched with `diff_filler`), the row tints and changed-word tints of T1218
  (`diff_*_row` / `diff_*_word`, a partnerless changed row tinted whole as `DiffView` does), and the
  model's `N hidden lines` folds (T1224), which a click unfolds. No syntax highlighting, no editing,
  no per-fold breadcrumb or partial expand: follow-ups if wanted.
- Changed words are computed with the diff on the background executor: `Diffed::words` in
  `commit_diff.rs` (from `InlineDiff::changed_spans`).
- Horizontal scroll is per section (both sides together), clamped so the section's widest row's end
  shows: every row of both sides is shaped with the text system in the font it's drawn in, once per
  section and font (family and size), against that section's own gutter; a stored offset past the
  limit (window resized, font changed) is clamped and stored back when drawn. Vertical scroll is the list's, and the rows shown are
  passed to `CommitDiff::set_viewport`.
- Review round 1: a navigation or Graph-reveal target is kept by path and hunk and re-scrolled on
  every relayout (sections landing above it) until the wheel scrolls the list or a section is
  collapsed/expanded; a reveal before the file listing lands waits for it. Any of those user actions
  (the wheel, a collapse or expand, Collapse All) also cancels a landing waiting for a section's
  texts (review round 2). Tabs match on commit id and the
  repository's working tree, so another worktree's same commit is its own tab.
- Navigation: toolbar previous/next (no keybinding added; the view's key context is
  `CommitDiffView`, so f7 isn't bound there). Stops are every hunk of every read section plus one per
  section not read yet; landing on an unread one expands and reads it, then goes to its first (last,
  going back) hunk, or on past it if it has none. Wraps at both ends.
- Graph: 'Open Changes' is now on the commit row's menu too (first item), and the card's
  files-changed count is a link. With the commit's view open, a click or Enter on one of its file
  rows shows that tab scrolled to the file's section instead of opening the single-file diff; the
  file menu's Open Changes still opens the single-file diff.
- Files outside the listed scope: `commit_diff.rs` (the words above, `test_gate`s on the listing and
  section reads so tests can order them, a `workdir` accessor, `display_path` made `pub(crate)`, doc link), `git_log_menu.rs` (the commit
  item) and `git_log_menu_tests.rs` (`commit_menu`'s expected item list gains Open Changes).
- The tests' text system (gpui's `NoopTextSystem`) draws every character one em wide, two outside
  the basic plane, whatever the font: the wide-row test uses emoji, and the font switch is checked by
  what the width cache is keyed on, not by two fonts' advances.
