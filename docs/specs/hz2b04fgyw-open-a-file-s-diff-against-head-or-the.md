---
task_uid: "01m4xqgjwm7a7150hz2b04fgyw"
task: "T123"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_diff_tests::opens_index_and_head: in a temp repo with a staged and an unstaged edit to a.rs, git.diff_index opens a DiffView whose left text is the index's and right is the open buffer, titled 'a.rs (Index ↔ Working Tree)'; git.diff_head's left is HEAD's; asking twice focuses the same tab; ':Gdiffsplit' and ':Gdiffsplit HEAD' do the same"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_diff_tests::panel_and_bar: Enter on a Changes panel row opens HEAD ↔ Working Tree for it and alt-enter opens the file; a ChangeBarClicked { rows: 9..12 } opens HEAD ↔ Working Tree with the right caret on row 9"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_diff_tests::compare_file: diff.compare_file with b.rs chosen opens b.rs (read-only, left) against the active file; ':diffsplit b.rs' does the same"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_diff_tests::vim_diff_commands: in a HEAD diff, 'do' on a right-side hunk reverts it to HEAD's rows and 'u' restores it; 'do' and ':w' on the left answer E21; 'dp' on the left side's hunk reverts the right side's; 'dp' from the right answers E793; ':q' closes the diff tab"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test git_diff_tests::refresh_and_navigation: after 'git add' of the file in the repo, the open Index diff's left side shows the new index text after the refresh; git.next_change in a tab editor goes to the next changed row and wraps with 'Wrapped to first change'; cargo test -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Open a file's diff against HEAD or the index from the Changes panel, the change bar and the palette, compare two files, and run vim's diff commands in it (Mac T551)

## Problem

The user (2026-10-12): "open a file's working-tree diff against HEAD (and against the index) from the Changes panel /
gutter / palette … Plus a general 'compare two files' entry using the same view." The DiffView (fe-gpui) and the
`git.diff_*` command stubs (`crates/fe-workbench/src/git_diff.rs`, the git-commands card) exist; nothing opens one.

## Change

In `crates/fe-workbench/src/git_diff.rs` (replacing its stubs), with small hooks in
`crates/fe-workbench/src/git_panel.rs` and `crates/fe-workbench/src/git.rs`:

- `git.diff_index` (`:Gdiffsplit`): the active file's DiffView, left `Index` (`Repo::index_text`, empty for an
  untracked file), right `Working Tree` (a new `EditorModel` on the open editor's buffer, so edits show in both and
  save as the file). `git.diff_head` (`:Gdiffsplit HEAD`): left `HEAD` (`head_text`). Opens as a tab in the active
  pane; asking again for the same file and base focuses that tab. Outside a repository: message `Not in a git
  repository`. `:Gdiffsplit <other rev>`: `Not supported yet: :Gdiffsplit <rev>`.
- Changes panel: Enter or a click on a file opens its diff (VS Code's Source Control): HEAD ↔ Working Tree (the
  panel lists staged and unstaged together until the panel-groups card); a deleted file shows HEAD ↔ empty, both
  read-only. `alt-enter` and the row's "Open File" button open the file as Enter did before.
- Change bar: `EditorViewEvent::ChangeBarClicked { rows }` (the change-bar click card) opens HEAD ↔ Working Tree for
  that file, side by side, with the right side's caret on the change's first row, centred. (VS Code shows an inline
  peek instead; this opens the diff, stated.)
- `diff.compare_file` (`:diffsplit {file}`, path resolved as `:e` resolves it): palette entry opens the quick-open
  file picker titled "Compare with…"; the chosen file is the read-only left side (title: its project-relative path),
  the active file the right.
- `git.next_change`/`git.previous_change` in a tab editor: caret to the next/previous `row_changes()` change,
  wrapping with `Wrapped to first change`/`last change` (VS Code's Go to Next Change); in a DiffView,
  `diff.next_change`/`previous_change`.
- Fresh left sides: after a git refresh that changed the file's status, or moved HEAD, each open DiffView over that
  file reloads its git side in the background (`git.rs`'s refresh calls `git_diff::refresh_views`).
- Vim, through `git_diff::host_command` for editors in a DiffView: `do`/`:diffget` on the right side reverts the
  hunks in those rows to the left text (`EditorModel::revert_changes`, one undo step); on a read-only side `E21:
  Cannot make changes, 'modifiable' is off`. `dp`/`:diffput` from the read-only left side reverts the right side's
  hunks there the same way (putting left's text into right); from the right side into `HEAD` or another read-only
  left: `E793: No other buffer in diff mode is modifiable` (into `Index` is the hunk-staging card's). `:w` on the
  right side saves the file; on the left `E21`. `:q` closes the diff tab (`E37` rules as for a tab when the right
  side is dirty and the file has no other editor). `]c`/`[c` (the vim diff-keys card) on both sides.

## Tests

New file `crates/fe-workbench/src/git_diff_tests.rs` (declared `#[cfg(test)]` from git_diff.rs), gpui
`TestAppContext` with temp git repos built by the `git` CLI (developer config shut out).

## Implementation notes

- Scope widened (revision 2) to the tests that asserted the stubs' answers (`git_commands_tests.rs`,
  `git_panel_tests.rs`: Enter and a click on a Changes row now open its diff), `quick_open.rs` (a `set_pick` hook:
  the "Compare with…" picker is a fresh quick open whose pick goes to the compare instead of opening the file; it
  indexes the project afresh each time it's shown) and `vim_host.rs` (its `:e` path resolution, save, `:qa`/`:wa`
  and messages made `pub(super)` so a diff view's sides run them).
- The hooks are `Workbench` methods defined in `git_diff.rs`: the Changes panel calls `open_change_diff` through
  its git state's workbench; `GitState::track` subscribes to each tab editor's `ChangeBarClicked` and calls
  `Workbench::change_bar_clicked`, which finds the window whose root is the workbench; a refresh that finds the
  repository calls `refresh_git_diffs`.
- An `Index` side is read again after every refresh, not only one that changed the file's status: `Repo::status`
  combines staged and unstaged changes, so `git add` changes no status. A `HEAD` side follows the spec.
- A file no tab has open (a Changes row) is read from disk for the right side, a buffer of its own that saves as
  the file. Review round 1 (F1, F2, F7) made that working side one of the file's open editors
  (`Workbench::file_editors`): `:wa`/`:qa` count it, disk sync reloads it (scope revision 3 adds `disk_sync.rs`), a
  later diff of the file reuses it (found by real path) and a tab opened on the file shares it (`pane.rs`'s peer
  model looks at diff views' working sides).
- Review round 1, the rest: a file whose folder was deleted finds its repository from the nearest existing folder
  (F3); a diff asked for again, or found at a refresh, with its file deleted or restored since is rebuilt in its tab
  unless its working side is dirty, and every refresh looks whether each git diff's file is there (F4); git-side
  reads are numbered per view and only the last lands, and an open landing after a refresh began reads its git side
  again (F5); `git.next_change` in a tab remembers the change it went to (F6); files of `loader::LARGE_FILE_BYTES` or
  more (`loader::is_large`, scope adds `loader.rs`) aren't compared, and a git text that large isn't shown (F8,
  though git reads it whole first: fe-git has no blob-size query); a diff opens in the pane it was asked from and is
  shown only if it's the last asked for and nothing was navigated to there meanwhile (F9); a change-bar diff is
  shown side by side (F10).
- Centring on a change-bar click uses the clicked editor's visible row count, as the new view hasn't been drawn.
- `:Gdiffsplit <rev>` still answers `Not supported: :Gdiffsplit <rev>` from `git_commands`' ex forms, which are
  outside the scope.
- `:diffput` from the right side of an `Index` diff answers `Not supported yet: putting a change into the index`
  until the hunk-staging card.
- Review round 2: a diff keeps a plain "source" model over its working buffer, which nothing shows and which
  another diff of the file or a tab opened on it splits (`Workbench::diff_source`, found by the file's real path),
  so a tab shares the buffer but not the diff's comparison state or tints (F2, F11). Ordinary tabs still match only
  by their own spelling: a symlink's tab is deliberately its own buffer (`tab_git_status_tests`). `:e` from a diff
  side keeps an open file's spelling, as `:e` does. Every refresh now reads every git diff's text and looks at its
  file, as one numbered read: a rebuild carries its read's number and lands nowhere if a later read started or its
  tab closed, and one whose working side became dirty keeps it but takes the git text (F5, F14). Whether the file is
  there is looked at as a diff opens, not taken from the combined status, which reports a staged deletion of a file
  still on disk as deleted (F13). A diff also isn't shown over a file opened in its pane meanwhile (F9). Git text is
  read as a file's contents, line endings normalized (F12), and refused before it's read when its blob is as large
  as a file too large to edit, through new `Repo::head_blob_size`/`index_blob_size` in fe-git, which read the
  object header (F8, scope revision 4). A diff's working side's change bar opens the file against HEAD (F15).
- Review round 3: `dp` applies exactly the hunks picked, rediffed from the two texts, each right side's rows
  replaced by the left's in one edit, one undo step (F16). A diff asked for from an editor keeps that editor's buffer
  whether or not its file is on disk, a new file not yet saved or a dirty one deleted (F17); a Changes row still
  looks at disk. `git.next_change` reuses the change it went to only while the changes are the same (F18). A diff's
  source model catches up as the view moves on, so it holds no buffer events or history back, and goes as the view
  closes (F19). A change-bar click on a diff found out of date is revealed, side by side, in the diff that replaces
  it (F20).
- Not changed (round 3's prior F2): a tab opened through a symlink and Open File on its target make two buffers
  whether or not a diff was ever open. Pane matches tabs by their own spelling on purpose (a symlink's tab is its own
  editor, `tab_git_status_tests`), and that's the pane's identity model, not this card's; disputed.
- Review round 4: `dp` splices the picked hunks' rows, split at newlines so a last row without one and a lone final
  newline are rows as the diff counts them, and applies the result as one minimal edit (F21). An editor asking for a
  base whose diff already compares with nothing (its file deleted) has it rebuilt around the editor's buffer (F17). A
  diff's entry goes as the view is released, through an app-level release hook, so a closing window, which releases
  the workbench first, drops it too (F19). Of overlapping requests for the same diff, the last asked for places the
  caret, whichever lands first (F22).
