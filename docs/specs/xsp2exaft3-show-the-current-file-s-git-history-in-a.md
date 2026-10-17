---
task_uid: "01m4xqgms2mrge0fxsp2exaft3"
task: "T130"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_history_tests::lists: in a temp repo where a.rs was renamed to b.rs, git.file_history from b.rs's editor shows the History panel titled 'History: b.rs' with one row per commit newest first ('\u003cshort\u003e  \u003csummary\u003e', '\u003cauthor\u003e, \u003ctime ago\u003e', the rename row with 'renamed from a.rs'); ':0Gclog', ':Gclog %' and ':Gclog -- %' do the same; ':Gclog' and ':3Gclog' answer 'Not supported yet: …'; the palette lists 'Git: File History'"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_history_tests::opens_change: Enter on a modifying commit opens a DiffView, both sides read-only, left the file at its parent and right at the commit, titled 'b.rs (\u003cshort\u003e^ ↔ \u003cshort\u003e)', focused; on the rename commit the left text is a.rs's at the parent; on the root commit the left is empty; opening it again focuses the same tab"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_history_tests::keys: with vim on and the panel focused, j/k move, g g and shift-g go to the first and last row, o opens, and q hides the dock and focuses the editor; with vim off j and q do nothing, and up/down/enter move and open"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_history_tests::edges: outside a repository the command shows 'Not in a git repository'; an untracked file shows 'No commits for \u003cname\u003e'; the list is read off the main thread (the panel shows a loading state until it arrives)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench passes; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; workbench.rs, vim_host.rs and the keymap files are unchanged; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show the current file's git history in a History panel from git.file_history and :0Gclog, opening a commit's change to the file in a DiffView (Mac T590)

## Problem

The user, 2026-10-13: "File history" should show the current file's commits in a panel, like fugitive's
`:0Gclog`. Opening one shows that commit's change to the file in the DiffView. The standing rule is that every
feature is vi friendly: `j`/`k`/Enter/`q` in the list, an ex command and a palette command. A repo-wide log with
a graph is a later card.

This card builds on:

- `Repo::file_log` (the fe-git file-log card): commits newest first, following renames, each with `path` and
  `from`.
- `HostCommand::User` with `range: Some(0..0)` for `:0…` (the fe-vim zero-range card).
- The git command registry (T550): palette entries and `:G…` ex routing in `crates/fe-workbench/src/git_commands.rs`.
- The DiffView (T545), opened as a tab by `crates/fe-workbench/src/git_diff.rs` (T551).

## Change

New module `crates/fe-workbench/src/git_history.rs` with a `HistoryPanel`.

**Command.** Add `git.file_history`, "Git: File History", to T550's registry table in `git_commands.rs` and
declare the module next to T550's git modules. Its ex forms:

- `:0Gclog` (range `Some(0..0)`), `:Gclog %` and `:Gclog -- %` run it.
- `:Gclog` with no range or arguments, or with any other range or argument, answers
  `Not supported yet: :<the line>`.

**What it does.** For the active editor's file, it shows the History panel on the right dock and lists
`Repo::file_log(path, 1000)`. The list is read on the background executor.

- Outside a repository: the message `Not in a git repository`, as in T551.
- No commits: the panel's empty text, `No commits for <name>`.
- 1000 commits returned: a last row, `Showing the newest 1000 commits`.
- The list is a snapshot and isn't refreshed live. Running the command again re-reads it, for the active file
  at that time.

**Panel.** `crates/fe-workbench/src/dock.rs` gets `DockPanel::History` (name "History", id "history", icon
`Icon::FileDiff`, show command `git.file_history`) and `dock_item!(HistoryPanel, DockPanel::History, false,
true)`. Like Problems, it goes on the rail once it has shown and is focused when shown. The panel is created on
first use and handed to the dock with a `set_history` like `set_changes`, so `workbench.rs` doesn't change.

- Title row: `History: <file name>`.
- Each row: `<short>  <summary>`, then `<author>, <time ago>` in the muted colour. A rename row adds
  `renamed from <old path relative to the root>`.

**Keys,** handled in the panel's own `on_key_down`, as `git_panel.rs` does (no keymap files):

- Always: `up`/`down` move and `enter` opens.
- With vim on (the key context gets the `vim` flag): `j`/`k` move, `g g`/`shift-g` go to the first and last
  row, `o` opens, and `q` hides the dock and focuses the active editor.
- With vim off, those letters do nothing.
- A click opens.

**Opening a commit.** Opening a row opens, through `git_diff.rs`'s tab opener, a DiffView with both sides
read-only:

- Left, titled `<short>^`: `text_at(parent, from.unwrap_or(path))`, empty for a root commit or one that added
  the file.
- Right, titled `<short>`: `text_at(id, path)`, empty for one that deleted it.
- Tab title `<name> (<short>^ ↔ <short>)`. Opening the same commit again focuses that tab. Focus goes to the
  diff.

## Tests

New file `crates/fe-workbench/src/git_history_tests.rs`, declared `#[cfg(test)]` from git_history.rs. It uses a
gpui `TestAppContext` and temp repos built with the `git` CLI, developer config shut out, as T551's tests do.

## Implementation notes

- `git_diff.rs` is outside the scope and its opener (`new_diff`, `place_diff`, `show_diff`) is private to it, so
  `git_history.rs` opens the commit's diff view itself with the same steps: `DiffView::new` over two read-only
  sides, `own_editor` for both sides (so `:q` and the other host commands go to `git_diff` as for any diff view),
  pushed to the pane active when asked and shown if it's still the last open asked for. Its open views are kept
  per commit and file, so opening one again shows its tab. `git_branch.rs`'s `ago` is private too, so
  `git_history.rs` has its own copy.
- `git_history` is declared from `git_commands.rs` (`workbench.rs` doesn't change), and `lib.rs` re-exports
  `HistoryPanel` for `dock.rs`.
- `:Gclog` takes a range and any arguments in `git_commands::ex_command`; `git_history::run` picks the file's
  forms (`:0Gclog`, `:Gclog %`, `:Gclog -- %`) and answers `Not supported yet: :<line>` for the rest, the line
  rebuilt from the range as rows from 1.
- The dock's title row says `History: <file name>`: `dock_item!` takes an optional title method.
- The History rail button (`DockOp::ShowHistory`) shows the history last listed; `git.file_history` reads it again.
- Outside a repository the panel shows `Not in a git repository` too, after its loading state, as the repository
  is found on the background executor along with the list.
- `q` hides the dock once the panel's key handler is over (`Window::defer`): hiding it looks at which panel has
  focus, which reads the panel being updated.
- Review F1, F2: a commit's open is numbered as it's asked for, an open tab shown again included, and its pane's
  navigations and opens are taken then (`Landing`, as `git_diff`'s); a late read is shown only if it's still the
  last asked for, its pane is still active and nothing was navigated to or opened there, else it waits in its tab.
  `git_history_tests::opens_late` covers an open tab shown again, another tab shown and a split made while a read
  is pending. (Showing a tab counts as a navigation, so either check alone holds the first case.)
