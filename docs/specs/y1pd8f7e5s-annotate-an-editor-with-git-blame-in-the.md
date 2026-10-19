---
task_uid: "01m58zqe54hzab1qy1pd8f7e5s"
task: "T215"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test fe-workbench workbench::git_blame::git_blame_tests::column_tests::toggling_annotates_each_run passes: on the fixture's a.rs, git.toggle_blame_annotations gives the active editor exactly three entries, rows 0-1 and 3-13 '\u003cdate\u003e Ann' and row 2 '\u003cdate\u003e Bob', each date the YYYY-MM-DD of git blame --porcelain's author-time in +0200, Bob's heat 1.0 and Ann's 0.0; another tab on the same buffer has none; running it again removes them; on an untracked file it answers 'Not tracked by git' and sets none"
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-workbench workbench::git_blame::git_blame_tests::column_tests::edits_and_head_moves_reblame passes: after typing into row 5 and waiting EDIT_REST, row 5 is in no entry and Ann's entries cover rows 0-1, 3-4 and 6-13; after committing the edit as Cid in the fixture and a git refresh, row 5's entry is '\u003cdate\u003e Cid' with heat 1.0; closing the tab drops the annotations, and reopening a.rs shows none"
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-workbench workbench::git_blame::git_blame_tests::column_tests::hover_and_click_open_the_commit passes: hover(ix, 2) of Bob's entry is the text git.blame_line shows with the caret on row 2; a click at annotation_bounds(2) opens the same diff view git.open_line_commit opens for row 2 (its title and caret row), and a second click shows that one instead of opening another"
    required: true
    manual: false
  - id: "C4"
    text: "Test fe-workbench workbench::git_blame::git_blame_tests::column_tests::ex_forms passes: ':Git blame' and ':G blame' toggle the annotations, ':Gitsigns toggle_current_line_blame' toggles line blame; typed_command_ids_tests lists git.toggle_blame_annotations, 'Git: Toggle Blame Annotations', context workbench; the existing fe-workbench suites pass with only the ':Git blame' expectations in git_commands_tests.rs and git_blame_tests.rs changed"
    required: true
    manual: false
  - id: "C5"
    text: "Integration test fe-keymap blame_annotation_keys_toggle_the_column passes: cmd-alt-g b on macOS and ctrl-alt-g b off macOS (vim off, and vim insert) run git.toggle_blame_annotations in an editor, ctrl-alt-g b in vim normal mode doesn't, and the View menu has 'Toggle Blame Annotations' for it; git_keys still passes"
    required: true
    manual: false
  - id: "C6"
    text: "Test fe-workbench workbench::git_blame::git_blame_tests::column_tests::recent_commits_show_relative_age passes: with now() pinned, a commit 3 days old shows its relative age text (as the popover's age helper gives it) before the author, a commit exactly 30 days old and an older one show YYYY-MM-DD, rows not committed yet stay blank, and the hover popover for the recent commit includes its full ISO date"
    required: true
    manual: false
---

# Annotate an editor with git blame in the gutter column, as JetBrains does: each run's date and author tinted by age, the commit's popover on hover, its change on click, toggled by Git: Toggle Blame Annotations (Mac T820)

{
  "mode": "inline",
  "body": "## Problem\n\nThe user, 2026-10-17 (Mac main 92c57c57): \"I don't like the git blame view. Can we do something closer to\njetbrains with a column getting added before the line.\" Today's blame, from T555 (helium's T127), is in\ncrates/fe-workbench/src/git_blame.rs: `git.toggle_line_blame` shows the caret row's\n`<summary>, <author> (<ago>)` after the line, and resting on it shows the commit's popover\n(`popover`, :726). `git.open_line_commit` opens the change the caret row's commit made to the file in a\nread-only diff view (`open_line_commit` :876 → `open_commit` :1128). JetBrains' \"Annotate with Git Blame\"\nshows every row's commit in a gutter column left of the line numbers instead. It tints by age, and\nhovering shows the commit and clicking opens it.\n\n`fe_git::Repo::blame` (crates/fe-git/src/blame.rs:74) already blames the whole buffer text. It returns\n`BlameRun { rows, commit: Option<Arc<CommitInfo>> }` per run of rows, with id, short id, author, email,\nauthor time and offset, summary and full message, and `None` for rows edited or added since HEAD.\ngit_blame.rs keeps each buffer's last blame (`BlameState::blamed`), but only the window line blame\n(`LineBlame`) drives blames today: `start_blame` :495, `landed` :544, `check_head` :578.\n\nThe previous card adds the gutter annotation column API (`fe_gpui::GutterAnnotations`,\n`EditorView::set_gutter_annotations`, `annotation_bounds`). This card puts blame in it.\n\n## Waits for (not Mac tasks, so not --depends-on)\n\n- helium's T135, which holds git_blame.rs and git_diff.rs (it cancels superseded blame work) and waits on\n  the user's findings_override. Start this card only once T135 has landed on Mac main, and build on its\n  blame scheduling.\n- helium's run of T704 (git commands to `#[kiln::command]`), which holds git_commands.rs, workbench.rs,\n  workbench_commands.rs and typed_command_ids_tests.rs. Start only once it has landed on Mac main.\n- neon's T707, which collapses the keymap and menu tables. Add the binding and the menu item to whichever\n  table Mac main has when this card starts.\n\n## Change\n\n1. **Command** `git.toggle_blame_annotations`, titled \"Git: Toggle Blame Annotations\". Register it the way\n   T704 lands: a `#[command(id = \"git.toggle_blame_annotations\")]` method, doc line its title, in the\n   `#[kiln::command(context = \"workbench\")] impl Workbench` block in\n   crates/fe-workbench/src/workbench_commands.rs. Its body runs a new `GitCommand::ToggleBlameAnnotations`\n   (crates/fe-workbench/src/git_commands.rs, `Feature::Blame`) through `run_git_command`. Append its ID,\n   title and context to the table in crates/fe-workbench/src/typed_command_ids_tests.rs.\n   - Ex forms (`EX_FORMS`): `:Git blame` and `:G blame` now toggle annotations, as fugitive's `:Git blame`\n     opens a blame column. `:Gitsigns toggle_current_line_blame` stays on line blame. Update the\n     `G blame` expectations in crates/fe-workbench/src/git_commands_tests.rs (~:199, :206). Switch\n     `ex_and_popover` in crates/fe-workbench/src/git_blame_tests.rs (~:371, :374) to\n     `Gitsigns toggle_current_line_blame`.\n\n2. **Toggle** (crates/fe-workbench/src/git_blame.rs routes it; the column's code goes in a new\n   crates/fe-workbench/src/git_blame_column.rs, declared from git_blame.rs):\n   - It turns annotations on or off for the active editor only, per editor as JetBrains does. Another tab\n     or a split on the same buffer is not annotated.\n   - On a file with no blame it answers with the existing messages (`NO_FILE`, `NO_REPO`, `UNTRACKED`,\n     `TOO_LARGE`) through `answer_git_op` and shows nothing.\n   - The annotations are dropped when the editor is released (its tab closes) or its path changes.\n\n3. **Entries**: one `GutterAnnotation` per `BlameRun` with a commit, anchored over its rows at the blamed\n   version. Runs with no commit (not committed yet) get no entry: blank, as JetBrains shows them.\n   - Text: `<when> <author>`. USER decision (2026-10-17): `<when>` is RELATIVE for a commit whose author\n     date is less than 30 days before now, using the existing blame age text (e.g. `5 min ago`,\n     `3 days ago`, the same helper the popover's age uses), and the ISO date `YYYY-MM-DD` (the author date in\n     the commit's own offset, `date()`'s first 10 characters) from 30 days on. The hover popover always\n     shows the full ISO date. The author is cut to 20 characters, the 20th replaced by `…`. Relative text\n     ages while the column is open: recompute the entries' text when the column next redraws after a\n     minute boundary, or simply on each reblame; say which in the code.\n   - Heat: rank the distinct commits in the file by author time, newest first. With n commits, rank r gets\n     `1 - r / (n - 1)`, and one commit gets 1.0.\n   - `hover(ix, _)` is the existing `popover(Some(commit), now())`: author, email, date and age, message,\n     short id.\n   - `on_click(ix, row, ..)` opens that commit's change to this file with the caret on `row`. This is\n     exactly what `git.open_line_commit` does for a caret on `row`; factor `open_line_commit` to take the\n     row. It shows the diff view already open, as that command does.\n\n4. **Following the file**: the annotated buffer is blamed again as line blame's buffer is. That means\n   `EDIT_REST` after an edit, and on a git refresh or when the editor becomes active if HEAD moved. Both\n   share `BlameState::blamed`, so line blame and annotations on one buffer blame it once. Each landed blame\n   replaces the editor's annotations with entries for its version. Rows edited since stay blank (the\n   previous card's rule) until the next blame lands, which marks them not committed (blank). It works with\n   line blame on or off.\n\n5. **Key**: Zed's `git::Blame` chord.\n   - macOS: `cmd-alt-g b`, context `editor`.\n   - Off macOS: `ctrl-alt-g b`, context\n     `editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)`. This is the same guard as\n     `ctrl-alt-y`, since the editor reads ctrl-alt chords as vim's ctrl keys.\n   - Neither `alt-g` chord is bound today. Put it in the git section of crates/fe-keymap/keymaps/fastedit.toml.\n\n6. **Menu**: View menu, \"Toggle Blame Annotations\" beside \"Toggle Line Blame\"\n   (crates/fe-keymap/menus/fastedit.toml ~:296). fastedit has no gutter context menu, so there's no entry\n   there.\n\nTests:\n- crates/fe-workbench/src/git_blame_column_tests.rs, declared in the existing\n  crates/fe-workbench/src/git_blame_tests.rs by `#[path = \"git_blame_column_tests.rs\"] mod column_tests;`.\n  It uses the parent's `fixture` (Ann's root commit 10 days ago, Bob's change of row 2 3 days ago), `window`,\n  `dispatch`, `ex`, `settle` and `wait` helpers, and reads `EditorView::gutter_annotations`.\n- crates/fe-keymap/tests/blame_annotation_keys.rs, a new integration test, built as\n  crates/fe-keymap/tests/git_keys.rs builds its keymaps.",
  "ref": "",
  "ref_blob": ""
}


## Lent to helium (note y11)

helium's T135 (finalized) and T154 (= Mac T704, finalized) are already in your main, so the two "Waits for" bullets about them are satisfied; build on T135's blame scheduling. Mac T816 (gutter annotation column) is landed in Mac main (2113e84d and earlier) and reaches you through the usual merge; if fe_gpui::GutterAnnotations is missing in your tree when you start, hold and tell the Mac lead. T209/T210 touch git_blame.rs too: rgt orders them. neon's T707 (keymap/menu table collapse): add the binding and menu item to whichever table your main has. fe-workbench UI card: the Mac lead regenerates the screenshot goldens at landing.

## Implementation notes

- The column's code is `git_blame_column.rs` (`mod column` of git_blame.rs). Annotated editors live in
  `BlameState::columns`, keyed by editor; each follows its editor (edits, path), the window (tab closed,
  becoming active) and the Changes panel (git refresh) with its own subscriptions.
- Blames are shared through `BlameState::blamed`: `landed` shows each blame kept in the buffer's annotated
  editors, and neither line blame nor a column starts a blame of a buffer version the other is already
  blaming (`line_blaming`, `column::blaming`). `line_blame_and_annotations_blame_once` checks it.
- Relative ages are made afresh as each blame lands and as the editor becomes active again (not per minute).
- A toggle whose first blame finds none answers through `answer_git_op` and leaves the editor
  unannotated; a later blame that finds none (the file untracked by a commit) shows an empty column.
- `open_line_commit` takes an optional row; a click's ask keeps the caret where it is, so `stands` checks the
  caret row only for a caret's ask.
- Tests pin the blame clock with a test-only `pin_now`, so the fixture's 10- and 3-day-old commits show dates.
- `git_keys.rs` was added to the scope (revision 2): its `no_vim_keys` counts every `git.*` binding and
  presses each as one keystroke, so it now leaves the new chord to `blame_annotation_keys.rs`. Menus show no
  shortcut for a two-key chord, so the View item has none.
- fe-gpui's `command_tests.rs`, `keymap_tests.rs` and `tests.rs` were added to the scope (revision 3): they
  count the macOS bindings (272 → 273) and list each editor binding's test (`cmd-alt-g b` is
  `blame_annotation_keys`').
