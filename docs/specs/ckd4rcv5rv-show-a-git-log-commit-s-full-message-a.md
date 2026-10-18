---
task_uid: "01m569n1f398rhkyckd4rcv5rv"
task: "T196"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test fe-workbench workbench::git_commands::git_log::git_log_tests::message_tests::hovering_a_commit_row_shows_its_whole_message passes: hovering a commit row past the tooltip delay shows a popover whose text is the summary, the body with its blank lines, '\u003cauthor\u003e \u003cemail\u003e', '\u003cdate\u003e (\u003cago\u003e)' with the date as git.blame_line's popover formats it, and the full 40-hex id; not before the delay"
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-workbench workbench::git_commands::git_log::git_log_tests::message_tests::expanding_a_commit_lists_its_body_above_its_files passes: Enter on a commit whose message is 'subject\\n\\nline one\\nline two\\n\\npara\\n' lists 'line one', 'line two', '', 'para' under it, then its files; a one-line message lists only its files; Enter again hides both; the selection moves over body rows and Enter or o on one does nothing"
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-workbench workbench::git_commands::git_log::git_log_tests::message_tests::a_commit_rows_muted_text_is_its_age_alone passes: a Git Log commit row's muted text is the age ('just now') without the author, while the History panel's rows keep '\u003cauthor\u003e, \u003cago\u003e'"
    required: true
    manual: false
  - id: "C4"
    text: "File and body rows too long for the panel end in an ellipsis instead of clipping mid-character, as the summary does"
    required: true
    manual: false
  - id: "C5"
    text: "The existing git_log_tests (updated only for the muted text and the mod line) and git_log_open_tests pass; git_blame.rs changes only date's visibility; no file outside the scope changes, and no command registration changes"
    required: true
    manual: false
---

# Show a Git Log commit's full message: a hover popover with its subject, body, author, date and full id, and its body above its files when expanded (Mac T809)

## Problem

The user, from the running app (2026-10-17, main 92c57c57), about the Git Log panel: "there's no real way
to see the full comment." Their screenshot shows the rows cut to "80bba19 Follow-ups from…", while the muted
"Daryl Robbins, 6 days ago" takes about 40% of each row.

- A commit row (crates/fe-workbench/src/git_log.rs `GitLogPanel::render_rows`, about :777-799) shows
  `commit_rows::texts`: `<short>  <summary>`, ellipsized, then `<author>, <ago>` in a `flex_none` div,
  so the subject loses space before the muted text does.
- Enter, `o` or a click expands a commit (`toggle`, :461) to its files only. The message's body is never
  shown anywhere in the panel, though `LogCommit.commit` (`fe_git::CommitInfo`) already carries the whole
  `message`, `email`, `time`, `offset_minutes` and the full `id`.
- File rows clip mid-character at the panel's edge
  ("A docs/specs/c6h5v8g6f5-follow-ups-from-review-of-t3(") because they have no `text_ellipsis`.

VS Code's Source Control graph and GitLens show the full message, author, date and id in a hover over a
commit.

## Change

All in crates/fe-workbench/src/git_log.rs unless named.

1. **Hover popover.** Each commit row gets a GPUI tooltip (`.tooltip(...)` on its stateful row element, at
   GPUI's usual hover delay) built from a small view in git_log.rs. Top to bottom, it shows:
   - the summary, bold;
   - the body (the message after its first line, leading blank lines dropped, trailing whitespace trimmed),
     wrapped, with its blank lines kept;
   - `<author> <email>`;
   - `<date> (<ago>)`, the date as `git.blame_line`'s popover gives it (crates/fe-workbench/src/git_blame.rs
     `fn date`, made `pub(super)` and called from here; nothing else in git_blame.rs changes);
   - the full 40-hex id, muted.

   It's at most about 480 px wide and wraps, never ellipsizes. Under `cfg(test)` the panel records the last
   popover's text (lines joined with `\n`), as `Pane::last_tooltip` does (crates/fe-workbench/src/pane.rs:1500).
2. **Body when expanded.** Expanding a commit lists its body's lines first, one muted row each, indented
   as its files are (blank lines kept as empty rows), then its files (or the `Loading…` / error row).
   A one-line message lists files only. This is a new `Line::Body(commit, row)`. The selection moves over
   body rows as over file rows, and Enter, `o` or a click on one does nothing. Collapsing hides both.
   Body rows are there as soon as the commit expands: they come from the message already in hand, with no read.
   Give `Line::id` a range for them that can't collide with file or note rows.
3. **Wider subject.** A commit row's muted text is the age alone (`6 days ago`); the author moves to the
   popover. The summary keeps `flex_1` and the age stays `flex_none`. The History panel's rows
   (`commit_rows::texts`, git_history.rs) are unchanged: give the Git Log its own text for the muted
   column, and leave `commit_rows::texts` alone.
4. **Ellipsis.** File rows and body rows too long for the panel end in an ellipsis
   (`min_w_0`, `overflow_hidden`, `text_ellipsis` on their text), as the summary does.

Update crates/fe-workbench/src/git_log_tests.rs where it pins the muted text (`"t, just now"` at :320 →
`"just now"`), and nothing else there but the mod line below.

New tests go in crates/fe-workbench/src/git_log_message_tests.rs, declared at the end of
crates/fe-workbench/src/git_log_tests.rs by `#[path = "git_log_message_tests.rs"] mod message_tests;`.
They build their repositories as git_log_open_tests.rs does (one commit whose message is
`"subject\n\nline one\nline two\n\npara\n"`, one with a one-line message), open the panel with `git.log`,
and hover with `simulate_mouse_move` plus `advance_clock` past the delay, as
crates/fe-workbench/src/tests/activity_rail.rs `tooltips_show_name_and_shortcut` does. A `cfg(test)`
helper may expose a commit row's drawn bounds.

Not in this card: the History panel's rows and a popover there; the Changes panel.

Files T704 (helium) also touches: git_log.rs (rows, render and a new tooltip view; no command code).
Nothing here touches git_commands.rs, workbench.rs, workbench_commands.rs, lib.rs or
typed_command_ids_tests.rs.

## Implementation notes

- GPUI lays a tooltip out at its min-content size (`AvailableSpace::min_size()` in
  `Window::prepaint_tooltip`), so a popover with only `max_w` paints its text wrapped over
  rows measured as one line each: they overlap. `CommitPopover` sets its width from its widest
  line as shaped (bold for the summary), up to 480 px, so the text wraps to the height laid out.
  Checked in the running app under Xvfb.
- The age alone comes from `commit_rows::texts` with `<author>, ` stripped, since
  `commit_rows::ago` and `git_blame::ago` are private and outside the scope.
- Body rows' element ids set the top bit, so no file, note or commit row shares one.
- Review b68ka6 (F1): a message taller than the window. The popover is a `hoverable_tooltip`, no
  taller than the window less 16 px; its summary and body scroll under the wheel (`overflow_y_scroll`)
  while the author, date and id stay in view below, and it occludes the list beneath. Covered by
  `message_tests::a_message_taller_than_the_window_scrolls_in_its_popover` (300 body lines: every part
  drawn within the window, the mouse moved into it keeps it open, the wheel scrolls it) and checked in
  the running app.
