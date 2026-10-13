---
task_uid: "01m4sxmc4kncg6c1t2zd8recbg"
task: "T109"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test diff_view_tests::aligned: for left 'a b c d' and right 'a X Y Z d' (rows), the left view gets a 1-row filler after its modified rows so both views paint row 'd' at the same y (and for left 'a b d' vs right 'a X Y Z d', a 2-row filler), the left's modified rows carry the Removed/Modified tint and the right's the Added/Modified tint, and the left view is read-only"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test diff_view_tests::live_right_side: typing a new row in the right side's buffer (shared with a separate EditorView of the same buffer) updates the fillers and tints within one frame and shows in the other view"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test diff_view_tests::scroll_synced: scrolling the right view 40 screen rows sets the left view's top to the same screen row, and back, without a feedback loop (each set once)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test diff_view_tests::navigation: next change (through DiffView's own action or method) from the top moves the focused side's caret to the first hunk's first row and the other side follows; past the last hunk it wraps to the first with 'Wrapped to first change'; previous change likewise"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-gpui passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Compare two texts in a DiffView pane item: side by side, left read-only, rows aligned with fillers, changes tinted, scrolling synced, next and previous change (Mac T545)

## Problem

The user (2026-10-12) asked for a diff view: side by side, read-only left side, synced scrolling, next/previous hunk
navigation, used for git diffs and for a general "compare two files". Nothing in fe-gpui shows two texts against each
other. This card builds the view, independent of git; the workbench card opens it.

## Change

New file `crates/fe-gpui/src/diff_view.rs` (`pub mod diff_view` and `pub use diff_view::{DiffView, DiffSide}` from
`crates/fe-gpui/src/lib.rs`), a `kiln_shell::PaneItem` like `FileViewer` (`crates/fe-gpui/src/file_viewer/pane_item.rs`):

- `DiffView::new(file_name, left: DiffSide, right: DiffSide, cx)`, `DiffSide { title: SharedString, model: EditorModel }`; the
  host passes the left side's model over a fixed text and sets it read-only; the right side is usually a model over the
  file's live buffer (a new model on the open editor's buffer, so edits in either show in both). Either side may be
  read-only (a commit's change).
- Two `EditorView`s side by side, the left one `set_read_only(true)` (the view-seams card). Each gets the other's text
  as its diff base (`EditorModel::set_diff_base`), so their change bars, `row_changes` and vim's `]c`/`[c` (the vim
  diff-keys card) see the hunks; a fresh model's diff base isn't shared with the file's tabs (splits share it, new
  models don't), so tabs keep their HEAD-based change bars.
- Alignment: `TextDiff::new(left, right)` (the text-diff card); each side's `set_blocks` gets `Filler` blocks from
  `TextDiff::fillers(side)`; removed rows on the left get a `Removed` row-background tint and added rows on the right an
  `Added` one, modified rows `Modified` on both (a `DecorationProvider` the view registers on each model).
- Recomputed when either side's buffer version changes, at most once per frame, off the UI thread over 1 MiB; the
  previous blocks stay until the new diff lands.
- Synced scrolling: each view's `EditorViewEvent::Scrolled` sets the other's `set_scroll_top` to the same screen row
  (fillers make rows line up, so equal screen rows are equal positions).
- Navigation: `diff.next_change` / `diff.previous_change` commands in the view's key context (`DiffView`): move the
  focused side's caret to the start of the next/previous hunk (`TextDiff::next_hunk`/`prev_hunk`), wrapping at the ends
  with the message `Wrapped to first change`/`last change` (VS Code wraps); the other side scrolls with it. With vim on
  they're also vim's `]c`/`[c` (the vim diff-keys card), which don't wrap (vim's diff mode doesn't).
- Title: `<file name> (<left title> ↔ <right title>)` (`DiffView::new` takes the file name), e.g. `a.rs (Index ↔ Working
  Tree)`; the tab is dirty when the right side's buffer is.
- Header row above each side naming its title (`HEAD`, `Index`, `Working Tree`, or a path).

Not in this card: inline mode (next card), opening it from the workbench, staging from it.

## Tests

New file `crates/fe-gpui/src/diff_view_tests.rs` (declared `#[cfg(test)]` from diff_view.rs).

Lead note (rev 2, scope decision): review round 1's F1, F2 and F7 need edits outside this card's scope: an fe-editor/src/lib.rs constructor giving a split its own diff base, the diff.next_change / previous_change registration in fe-gpui/src/commands.rs (busy with helium's lent work), and EditorView::shares_document in fe-gpui/src/pane_item.rs. They move to a **follow-up card** that the Mac lead drafts; don't make them here. C1 is corrected to the arithmetic ('a b c d' vs 'a X Y Z d' gives a 1-row filler on the left). C2 no longer requires the separate view's diff base to stay untouched, because that's F1, now in the follow-up. C4 may drive navigation through DiffView's own actions or methods, without the global command catalog (F2 is in the follow-up).

## Implementation notes

- `DiffSide` has a third field, `read_only` (`DiffSide::new(title, model)`, `.read_only()`), as the spec says either
  side may be read-only but gives the host no way to say so. The left side is read-only whatever it says.
- Diff bases: only a read-only side takes the other side's text as its `set_diff_base`. The spec assumed a "new model
  on the open editor's buffer" has its own diff base, but the only way to get a second model over a buffer is
  `EditorModel::split`, which shares the base with the file's tabs (and fe-editor is outside this card's scope). So an
  editable side, which is over a live buffer, keeps its base (the file's tabs keep their HEAD change bars, C2), and a
  read-only side, a host-made model over fixed text, gets the other's text. The tints and fillers come from the view's
  own `TextDiff`, so they don't depend on either base. A fe-editor `EditorModel::view_of`-style constructor (shared
  buffer, own base) would let the editable side take one too.
- C1's example: left `a b c d` against right `a X Y Z d` is one hunk of two rows against three, so the left gets a
  1-row filler (fillers equalize the sides' heights: 4 + 2 can't equal 5). The test checks that, and the 2-row filler
  the criterion describes with left `a b d` (one modified row against three).
- Recompute: the view diffs on render when either buffer's version moved, so at most once a frame; it observes both
  sides so their edits re-render it. Over `ASYNC_BYTES` (1 MiB) a side, the diff (and the texts' `String`s) is made on
  the background executor and the previous fillers and tints stay until it lands. Over 1 MiB no side gets a diff base:
  the model would run `diff_rows` against it on the main thread when a frame asks for change bars.
- Alignment is by buffer rows, so the view keeps both sides unwrapped and unfolded (a wrap or fold on one side is
  undone on the next render).
- Navigation remembers the hunk it went to and where it put each caret; while the focused caret is still there the
  next move counts from that hunk, so a hunk whose start on a side is past its last row (a trailing addition or
  removal) doesn't trap it.
- Navigation shows its message (`Wrapped to first change`, `Wrapped to last change`, `No changes`) in the right
  header and `DiffView::message()`. The other side's caret goes to the hunk's start on its side, without a reveal;
  its scroll follows the focused side's through the scroll sync. The navigation test drives the commands through a
  user keymap binding in the `DiffView` context. They aren't in the default keymap, the command catalog or the command
  list (fe-keymap and `commands.rs` are outside this card's scope).
- Review round 1's F1 (an editable side's own diff base), F2 (the commands in the catalog and command list) and F7
  (`EditorView::shares_document` recognizing a `DiffView`) need files outside this card's scope and go to the Mac lead's
  follow-up card (rev 2). Navigation is reachable through the view's `RunCommand` handler and its `next_change` /
  `previous_change` methods.
- Review round 2: a side over `MAX_DIFF_BYTES` (5 MiB) shows `Too large to compare` in the header, and navigation says
  so instead of `No changes` (F8). Edits made through a model the view doesn't show (the file's editor in another
  window) reach it within `POLL` (250 ms): it checks both buffers' versions then, as well as on render (F9). An edit
  it diffs redraws every window, so the file's editor elsewhere shows edits typed in the diff view. Moving the other
  side's caret first ends its vim visual mode or pending command, which would otherwise put its old selection back
  (F10). F1, F2, F3 (an editable split's own HEAD-based gutter diff, which its file tab already runs) and F7 stay with
  the follow-up card, as the rev 2 lead note says.
- Review round 3: a new diff clears navigation's message, and the header's `Too large to compare` wins over it (F8).
  The other side's caret moves without its own reveal, `autoscroll` off after vim's visual exit, so only the focused
  side scrolls and the other follows (F11). The poll and render also note the right buffer's dirtiness, so a save
  elsewhere, which leaves the text version alone, redraws the tab (F12).
