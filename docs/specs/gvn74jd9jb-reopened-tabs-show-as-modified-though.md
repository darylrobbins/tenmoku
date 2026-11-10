---
task_uid: "01m711jzd74cgaj7gvn74jd9jb"
task: "T1665"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "New test crates/fe-workbench/tests/restore_clean_editors.rs restores a saved layout of several clean files (one saved under a path with '..') and every restored editor is clean, stays clean over several frames and a second restore, and shows the normalized path"
    required: true
    manual: false
  - id: "C2"
    text: "The cause is fixed, named in the finish notes, and the new test fails at the base"
    required: true
    manual: false
---

# Reopened tabs show as modified though the files are unchanged, and stay marked: a restored session's clean editors must restore clean

# Reopened tabs show as modified though nothing changed, and stay marked

## User report (2026-11-09)

"When I reopen the editor, I see all the files that were opened before, but it marks a number of them as modified
when they aren't. And it seems to keep marking them as modified." Not every restored file, "a number of them".
User's follow-up: the v0.1.3 release app, on ANOTHER machine (not this Mac, so its state is not polluted by tests;
see T1664 for that separate problem). Saving clears the mark, and after a save the file comes back clean on the next
reopen. So a file is marked modified on its first restore after something (a session, an update, time) and the mark
is one-shot per file: the restore itself, not the content, dirties it.

## What is known

- (This Mac's state only, for shape:) the user's last real project layout, `~/.local/state/tenmoku/layouts/777efd6e3f720d2d.json`, restores six
  editors in ~/code/bookshelf2 (main.go, admin.spec.ts, auth.go, tsconfig.json, facets.go, and
  `/Users/daryl/code/fastedit/../bookshelf2/docker-compose.yml`, a non-normalized path). None has a `backup`, so
  hot-exit backups are not what marks them dirty; their backup dirs hold only owners.lock.
- The files are clean on disk: LF endings, a final newline, no trailing whitespace, no .editorconfig in bookshelf2,
  and `git status` is clean.
- Suspects to check, in order: something applied to the buffer at restore time that counts as an edit (formatting,
  editorconfig/final-newline, encoding or line-ending normalization, an LSP edit, a restored caret/selection or fold
  write through an edit path; see crates/fe-workbench/src/hot_exit.rs and the layout restore in workbench.rs); a
  dirty flag compared against the wrong saved revision after restore; the same file opened twice through two
  spellings of its path (the `..` path above) so one copy is a different buffer.

## Fix

Reproduce with a test that saves a layout with several clean files (including one stored with a `..` path and one
an LSP or formatter touches), restores it, and asserts every restored editor is clean, stays clean across a few
frames and a second restore, and has the normalized path. Fix the cause; the reproduction test is the regression test.
