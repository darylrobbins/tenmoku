---
task_uid: "01m4t6g243nqnagqhmrtdbtfyx"
task: "T102"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-buffer test proposed_insertion_order::accepting_a_later_insertion_first_keeps_text_order: base \"a\\nb\\nc\\n\", proposal \"a\\nX\\nb\\nY\\nc\\n\", user deletes \"b\\n\"; after accepting hunk 1 the pending hunk 0 range is 2..2 and hunk 1 is 2..4, and accepting hunk 0 then gives \"a\\nX\\nY\\nc\\n\" with ranges 2..4 and 4..6"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test proposed_insertion_order::accept_all_after_a_later_insertion_keeps_text_order: in the same setup after accepting hunk 1, dry_run(..).text() and accept_all both give \"a\\nX\\nY\\nc\\n\" with no stale hunks"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test proposed_insertion_order::accepting_in_text_order_keeps_text_order: accepting hunk 0 then hunk 1 gives \"a\\nX\\nY\\nc\\n\" (passes at base and after)"
    required: true
    manual: false
  - id: "C4"
    text: "accept_as takes its range from locate(), so accept, accept_all, hunks and dry_run agree; the existing proposed_* tests pass unchanged; cargo test -p fe-buffer and cargo clippy -p fe-buffer --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Land two proposed insertions whose points a user edit merged in text order, whichever hunk is accepted first (Mac T636)

## Problem

A proposed change's pure-insertion hunk is anchored by one `anchor_after(p)` (`anchors()`,
`crates/fe-buffer/src/proposed.rs:518`). When a user edit merges two insertion points into one offset
and the later hunk is accepted first, its text lands at the shared point and the earlier hunk's anchor
moves to after it. The earlier hunk then lands after the later one, and the outcome depends on accept
order: `accept`, `accept_all`, `hunks` and `dry_run` disagree with what they give when nothing has
been accepted yet. `accept_as` (proposed.rs:302) resolves `hunk.anchors(..)` itself rather than
through `locate()` (proposed.rs:439), which `hunks`, `accept_all_as` and `dry_run` use.

Checked 2026-10-13 at e6988aba: base `"a\nb\nc\n"`, `ProposedChange::from_text(.., "a\nX\nb\nY\nc\n")`
gives hunk 0 (insert `"X\n"` at 2) and hunk 1 (insert `"Y\n"` at 4); the user deletes `"b\n"` (2..4),
so both points are 2.

- Accepting hunk 0 then hunk 1 gives `"a\nX\nY\nc\n"`, as it should.
- Accepting hunk 1 first: pending hunk 0's `HunkView.range` is `4..4`, not `2..2`; then accepting
  hunk 0, `accept_all` or `dry_run(..).text()` all give `"a\nY\nX\nc\n"`, not `"a\nX\nY\nc\n"`.

## Change

In `crates/fe-buffer/src/proposed.rs`:

- In `locate()`, after `resolve_many`, adjust each pending insertion hunk i (empty `old_text`, not
  landed in this snapshot): walk the later hunks j > i from last to first, and when hunk j has landed
  in this snapshot with a non-empty range ending exactly at i's offset, move i's offset to the start of
  j's range.
- Make `accept_as` take its range from `self.locate(snapshot)[id.index]` instead of resolving the
  anchors itself, so `accept`, `accept_all`, `hunks` and `dry_run` agree.
- Anchors and the public API are unchanged; `proposed/dry_run.rs` already calls `locate` and needs no
  change.

## Tests

New file `crates/fe-buffer/tests/proposed_insertion_order.rs` with the setup above. The pinned
regression tests are `accepting_a_later_insertion_first_keeps_text_order` (also asserts the pending
hunk's range is `2..2` and the landed one's `2..4`) and
`accept_all_after_a_later_insertion_keeps_text_order` (dry run and `accept_all` both give
`"a\nX\nY\nc\n"` with nothing stale); both fail at the base. Add the control
`accepting_in_text_order_keeps_text_order`, which passes at the base and must keep passing.
