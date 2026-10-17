---
task_uid: "01m4xqgn1j22m2xyagzj0qct0p"
task: "T131"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_log_tests::lists: in a temp repo with a merged topic branch, an unmerged side branch and a tag, git.log and :Gclog show the Git Log panel titled 'Git Log: main' with HEAD's commits newest first in topo order, ref labels on their rows and each row's GraphRow equal to fe_git::graph's layout of the same list; git.log_all and ':Gclog --all' add the side branch under 'Git Log: All Branches'; ':Gclog foo' answers 'Not supported yet: …' while ':0Gclog' still opens T590's History; the palette lists 'Git: Log' and 'Git: Log (All Branches)'"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_log_tests::keys_and_pages: with vim on, j/k move, g g and shift-g go to the first and last loaded row and q hides the dock and focuses the editor; with vim off j and q do nothing and up/down work; in a 250-commit repo the panel first loads 200 rows and moving within 20 rows of the end loads the other 50, the graph continuing as one layout"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_log_tests::edges: outside a repository the command shows 'Not in a git repository'; an unborn HEAD shows 'No commits yet'; the log is read off the main thread (a 'Loading…' row until it arrives); T590's git_history_tests pass unchanged except that bare :Gclog now opens the log"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench passes; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; workbench.rs, vim_host.rs and the keymap files are unchanged; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show the current branch's or every branch's commits with a lane graph in a Git Log panel from git.log and :Gclog, vi-friendly and paged (Mac T609)

## Problem

The user, 2026-10-13, after asking for file history (T590): "a repo-wide log with graph is a later card". fastedit
can show one file's commits (T590's History panel, `:0Gclog`), but not the current branch's or every branch's
commits with a branch graph, as VS Code's Source Control Graph, GitLens and fugitive's `:Gclog` do. The standing
rule is that every feature is vi friendly: `j`/`k`/`gg`/`G`/Enter/`q` in the list, an ex command and a palette
command.

This card builds on:

- `Repo::log`, `Repo::commit_files` and `fe_git::graph::GraphLayout` (the fe-git commit-log card).
- T590's History panel (`crates/fe-workbench/src/git_history.rs`): its row text and list keys, and its
  `DockPanel::History` pattern in `crates/fe-workbench/src/dock.rs`.
- The git command registry (T550, `crates/fe-workbench/src/git_commands.rs`) and the DiffView tab opener in
  `crates/fe-workbench/src/git_diff.rs` (T551), which T590 uses to open a commit's change to a file.

## Change

### Shared list code: new `crates/fe-workbench/src/commit_rows.rs`

Move out of `git_history.rs` the commit row's text (`<short>  <summary>`, then `<author>, <time ago>` muted) and
the list's key handling (always `up`/`down`/`enter`; with vim on `j`/`k`, `g g`/`shift-g`, `o`, `q`), so both
panels use one copy. `git_history.rs` calls it and behaves exactly as before.

### Commands (in `git_commands.rs`'s registry)

- `git.log` "Git: Log": HEAD's history (`LogScope::Head`).
- `git.log_all` "Git: Log (All Branches)": `LogScope::All`.
- Ex: `:Gclog` with no range and no argument runs `git.log`; `:Gclog --all` runs `git.log_all`. `:0Gclog`,
  `:Gclog %` and `:Gclog -- %` stay T590's file history. Any other range or argument still answers
  `Not supported yet: :<the line>`. T590's test that bare `:Gclog` answers "Not supported yet" changes to expect
  the log (`git_history_tests.rs`).

### Panel: new `crates/fe-workbench/src/git_log.rs`, `GitLogPanel`

- `dock.rs`: `DockPanel::GitLog` (name "Git Log", id "git_log", icon `Icon::GitBranch`, show command `git.log`),
  `dock_item!(GitLogPanel, DockPanel::GitLog, false, true)`, on the rail once it has shown, focused when shown,
  created on first use and handed over with a `set_git_log` like T590's `set_history`. `workbench.rs` doesn't
  change.
- Title row: `Git Log: <branch>` (`Git Log: HEAD (detached)` when detached) or `Git Log: All Branches`.
- Each commit row: the graph cell, then the ref labels (small pills, the HEAD branch bold, tags with a `tag`
  prefix, remotes muted), then T590's row text from `commit_rows.rs`.
- **Graph cell.** One `canvas` per row, `ROW_HEIGHT` tall and 12 px per lane: each `GraphLine` of the row's
  `GraphRow` (top half and bottom half) drawn as a 1.5 px line, straight or diagonal, and the commit's dot (a 7 px
  circle; a merge commit's is hollow) at its column. Lane colour = `GRAPH_COLORS[color % 5]`, constants in
  git_log.rs taken from VS Code's `scmGraph.foreground1…5` defaults (`#FFB000`, `#DC267F`, `#994F00`,
  `#40B0A6`, `#B66DFF`).
- **Paging.** Reads run on the background executor. The first read is 200 commits; when the selection or the
  scroll comes within 20 rows of the end, the next 200 are read (`skip` = rows loaded) and laid out with the same
  `GraphLayout`, so the graph continues. A last row `Loading…` shows while a page is read; none once a page
  returns fewer than 200. The list is a snapshot: running the command again re-reads it.
- **Opening** a commit (Enter, `o`, a click) is the next card's (it expands the commit to its files and opens a
  file's change); in this card Enter and `o` only select.
- **Keys** (the panel's own `on_key_down` via `commit_rows.rs`, no keymap files): `up`/`down`/`enter` always;
  with vim on `j`/`k` move, `g g`/`shift-g` go to the first and last loaded row
  (`shift-g` doesn't load further pages), `o` = Enter, `q` hides the dock and focuses the active editor; with vim
  off the letters do nothing.
- Outside a repository: `Not in a git repository`, as T551. An unborn HEAD: the empty text `No commits yet`.

## Tests

New file `crates/fe-workbench/src/git_log_tests.rs`, declared `#[cfg(test)]` from git_log.rs, with a gpui
`TestAppContext` and temp repos built with the `git` CLI, developer config shut out, as T551's and T590's tests
do. Tests read the panel's rows and their `GraphRow`s (a `#[cfg(test)]` accessor), not pixels.

## Decided by the lead overnight

- **A dock panel, not a tab.** Pane items are editors today (`pane.rs`'s `Tab`), and VS Code's Source Control
  Graph is a sidebar view too; the panel sits in the sidebar dock like T590's History and can be widened (T602).
- **Current branch by default, all branches by a second command** (`git.log_all`, `:Gclog --all`), like
  `git log` vs `git log --all`; no toggle inside the panel in this card.
- **Split** along the risk line: listing, graph and paging here; expanding a commit and opening diffs in the
  next card, which depends on this one.
- **Lane colours** are VS Code's five graph defaults as constants; theme tokens for them are a later card.
- **Shared code** with T590 goes in `commit_rows.rs`; T590 must land first (`--depends-on T590`).

## Implementation notes

- The repository is the active file's, else the workbench root's; with neither the command answers
  `Not in a git repository` without making the panel.
- `Loading…` is the list's last row whenever a page is being read, the first page included; a page that
  fails to read shows its error there instead, and no further page is read.
- `shift-g` selects the last row loaded and reads no page, neither from the selection nor from the rows it
  scrolls into view: paging is held until the selection moves (a key or a click), the wheel turns over the
  list (even at its end, where the offset is clamped and nothing redraws by itself) or other rows are drawn.
  Review round 1, F2; round 2, F4.
- Pages are read from fixed commit ids, not the live refs (review round 1, F1): the first read resolves the
  scope's tips (`Repo::log_tips`: HEAD, and for all branches every branch, remote-tracking branch and tag)
  and every page is `Repo::log_from` those ids, so a commit, reset, branch switch or fetch between pages
  can't shift, duplicate or mix the list; `log_from` doesn't look at HEAD, so switching to an unborn branch
  between pages doesn't end it either (round 2, F1: the unborn-HEAD check is only in `log` and `log_tips`).
  Ref labels are read with each page. This widened the scope to
  `crates/fe-git/src/commit_log.rs` (the two methods; `Repo::log` now shares their reader) and a new
  `crates/fe-git/tests/commit_log_tips.rs`.
- The scroll trigger runs from the list's row rendering: the page read is started once the frame is over,
  as notifying during a draw doesn't redraw.
- A merge's dot is a ring with a transparent centre, and the lines meeting it stop at its edge, so the row's
  background shows inside whether the row is plain, selected or hovered (review round 1, F3).
