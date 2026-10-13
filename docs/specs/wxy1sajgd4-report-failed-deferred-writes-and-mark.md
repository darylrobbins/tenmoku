---
task_uid: "01m4spvr256951q9wxy1sajgd4"
task: "T105"
rev: 4
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Finishing a stream whose deferred write conflicts, or flushes into an undone transaction, returns a StepError (Conflict or Undone) instead of Finished(None), and the stream is dropped"
    required: true
    manual: false
  - id: "C2"
    text: "A finish that only flushes a deferred CR marks only the landed range (not earlier text), both over a longer selection and on an empty one, and its transaction is undone and redone with its mark"
    required: true
    manual: false
  - id: "C3"
    text: "Stream a line, accept a proposal replacing it, undo the accepted hunk: the stream's touched rows are restored; redo removes them again; undoing a covering whole-buffer deletion restores them too; redo gives each tail record its own span"
    required: true
    manual: false
  - id: "C4"
    text: "A first chunk equal to the selected text marks the stream's region (whether a transaction is recorded is left to the fe-buffer follow-up); all tests in crates/fe-editor/tests/scripted_ai.rs still pass"
    required: true
    manual: false
---

# Report failed deferred writes and mark every landed stream range per transaction in fe-editor's scripted AI (T501 review F1-F4) (Mac T617)

## Problem

Four findings from the codex review of T501 (approved 2026-10-12, never filed), all in
`crates/fe-editor/src/scripted_ai.rs`, the fake AI that T502's `ai_demo` will play:

- **F1** (:512) `finish` reports success when a deferred write fails: `EditStream::finish` swallows its
  `Conflict` (push a lone CR over a selection, edit inside the target, finish) and a deferred flush into an
  undone transaction hides `Undone`; the driver returns `Finished(None)`.
- **F2** (:515) completion marking assumes the buffer grew: a stream of only a deferred CR replacing a
  longer selection changes the text but marks nothing; on an empty selection it marks, but the transaction
  first created in `finish` is never added to `marked`, so undo leaves a collapsed mark and redo can't
  restore it.
- **F3** (:698) touched decorations have no owning transaction: `mark` removes every contained decoration
  and `catch_up` removes by span. Stream a line, accept a proposal replacing it, undo the accepted hunk:
  the streamed text returns but its touched rows are gone.
- **F4** (:499) a push is marked only if it made a transaction: a first chunk equal to the selected text
  ('old' over 'old') establishes the region with no transaction, and nothing is marked.

## Change

- Flush deferred output through an error-returning path; `Conflict` and `Undone` become `StepError`s and
  the stream is dropped.
- After `finish`, mark the actually landed range whatever the net growth, and add any transaction created
  during `finish` to `marked`.
- Keep each touched decoration with the transaction that made it; `mark` and `catch_up` update or remove
  only that transaction's records, so undoing one transaction restores the marks of the stream underneath.
- Mark the stream's region after every successful push, transaction or not.
- Tests go in the new `crates/fe-editor/tests/scripted_ai_followups.rs`, in the style of
  `tests/scripted_ai.rs`.

Lead note (rev 2, scope decision): review F1 (an equal-text first chunk must create no transaction) can't be met in scope, because fe-buffer's `Buffer::edit` records a transaction for it. It moves to a **follow-up card** touching fe-buffer, which the Mac lead drafts, and C4 no longer asks for it. Fix F4, F5 and F6 in scope with one design change: record stream-mark coverage by the covering **batch and edit identity**, not by position and length. Then (F4) a deferred-CR Finish marks only the newly appended range, (F5) undoing a covering whole-buffer deletion restores the stream's marks, and (F6) redo restores each tail record's own span, not the transaction's whole edited span. Add regressions for the F4, F5 and F6 cases the review describes.

## Implementation notes

- F4 as written ('old' over 'old' makes no transaction) doesn't hold: `Buffer::edit` records a transaction
  for an equal-text replacement, so that case marks at the base too. The path that really has a region and no
  transaction is a held-back `\r` (a push of only `\r`); `a_first_chunk_equal_to_the_target_marks_the_region`
  covers that (red at the base) and the equal-text push. Making an equal-text edit transaction-free would be an
  `fe-buffer` change, outside this task's scope.
- Marks are `Mark` records (transaction, range, active, cover) whose ranges `catch_up` moves through each
  event's edits. An edit by another transaction that replaces a mark's text covers it, remembering where in
  the replaced text it was; undoing that edit restores it there, and redoing covers it again. This includes
  the open stream's mark.
- Coverage is recorded by the covering edit's identity: a `Cover` holds the transaction, the index of the
  covering batch among that transaction's applied batches (counted from the events), and the edit's index in the
  batch. Only the undo of that batch restores the mark, at the same-index edit, so same-position batches,
  whole-buffer deletions and nested covers don't confuse it. A transaction with several marks records each one's
  span from its first mark when undone, and a redo puts each back there, not on the transaction's whole span.
  Text pushed while the first mark is covered, or after it is restored, goes to the tail marks, never over them.
  A deferred-CR `finish` over a covered stream marks only the CR it appended.
