---
task_uid: "01m4xqgm7w5cabt5wc3925rvh3"
task: "T128"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_branch_tests::status_item: in a repo on main the status bar has a left item 'main', 'main*' after an edit, the short id when detached, none outside a repo; after a switch from a terminal and a refresh it shows the new branch; clicking it opens the branch picker"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_branch_tests::picker: the picker lists '+ Create new branch…', then feat and main (local, newest first, main marked current), then origin/feat; ctrl-n/ctrl-p move the selection; Enter on feat switches HEAD to feat; on origin/feat (with no local feat) creates a tracking feat"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_branch_tests::create: git.create_branch with 'topic' creates and switches; 'bad..name' shows 'Not a valid branch name' and creates nothing; ':Git switch -c topic2' and ':Git checkout -b topic3' create directly; ':Git switch main' switches; ':Git switch nope' answers 'Not found: nope'"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_branch_tests::would_overwrite: with a local edit the target branch would overwrite, switching prompts 'Your local changes would be overwritten by checkout.' listing the path; Cancel changes nothing; Stash \u0026 Checkout leaves one stash and switches; Force Checkout switches and drops the edit"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Switch and create branches from a picker and the status bar's branch item, asking before a switch would overwrite local changes (Mac T556)

## Problem

The user (2026-10-12): "list local/remote branches in a picker, checkout, create branch; show current branch in the
status bar; refuse/ask on dirty conflicts." The vim-mode requirement (coordinator, same day): picker navigable with
`ctrl-n`/`ctrl-p` and an ex entry. `Repo::branches`, `current_branch` (git-reads card) and `switch`,
`create_branch`, `stash_and_switch` (branch card) exist.

## Change

In `crates/fe-workbench/src/git_branch.rs` (replacing its stubs) and `crates/fe-workbench/src/git.rs` (the refresh
reads `current_branch` with the status):

- Status bar: a host item on the left (the status bar's host items, `crates/fe-workbench/src/status_bar.rs` module
  docs), git-branch icon and the branch name with `*` when the working tree has changes (VS Code), the short id when
  detached; hidden outside a repository. Clicking it runs `git.switch_branch`.
- `git.switch_branch` (`:Git switch`, `:Git checkout` with no name): the shared picker (kiln-shell's, which already
  takes `ctrl-n`/`ctrl-p`, up and down) titled "Select a ref to checkout": first "+ Create new branch…", then local
  branches (current one marked, newest commit first), then remote ones, each with its last commit's summary and time
  ago; typing filters. Enter switches (`Repo::switch`, background).
- `git.create_branch` (`:Git switch -c {name}`, `:Git checkout -b {name}`, or the picker's first row): asks for the name
  in the picker's input ("Branch name"), an invalid name shows `Not a valid branch name` there; Enter creates and
  switches.
- `:Git switch {name}` / `:Git checkout {name}` switch without the picker; an unknown name answers `Not found:
  <name>`.
- `SwitchError::WouldOverwrite`: prompt Warning "Your local changes would be overwritten by checkout." detail listing
  up to 10 paths (then `and N more`), buttons ["Stash & Checkout", "Force Checkout", "Cancel"] (VS Code's, without
  Migrate Changes); Stash & Checkout runs `stash_and_switch`, Force `switch(name, true)`. Other errors: an error toast (T517)
  with git's output.
- After a switch: a git refresh (change bars rebase on the new HEAD); open files changed on disk reload through T433.

## Tests

New file `crates/fe-workbench/src/git_branch_tests.rs` (declared `#[cfg(test)]` from git_branch.rs), temp repos with a
local bare `origin`.

## Implementation notes

- The pickers are the workbench's `Palette::Branch` (`workbench.rs`, revision 4 of the scope), so other palettes
  dismiss them before showing and focus returns to the editor.
- The create row hands over to a second picker (placeholder "Branch name") once the branch picker is gone, as
  kiln-shell's input has a fixed placeholder. Its one row creates the branch while the name passes a local
  `git check-ref-format --branch` check; `Repo::create_branch` asks git itself all the same.
- Outside a repository the branch commands answer `Not in a git repository`.
- `git_commands_tests::ex_routes` asserted `Not supported yet` for `:Git switch`, `:Git checkout main`,
  `:Git switch -c x` and `:Git checkout -b x`, which this card replaces; it now expects `Not in a git repository`
  for those (its root isn't a repository). Revision 3 added that file to the scope.
- Every step holds to the root it started in (`Origin`: the root and the `GitState` entity, which `set_root`
  replaces): a new root dismisses the pickers (they follow `RootChanged`), and a listing, a pick, a switch landing
  or an answer to the overwrite question from before it does nothing.
- A listing shows only if it's the last one started (`GitState::start_branch_listing`) and no modal has shown since
  it started.
- Picked rows switch by ref kind: `fe_git::BranchRef` (`Local`, or `Remote { remote, branch }` with the configured
  remote's whole name) and `Repo::switch_to` / `stash_and_switch_to` (`crates/fe-git/src/branch.rs`, tests in
  `crates/fe-git/tests/switch_ref.rs`), so a local `origin/feat`, one made after the listing, or a remote named
  `team/origin` can't be taken for another ref. Names typed on the vim line still go through `Repo::switch`, as git
  resolves them.
- On Linux the default keymaps bind `ctrl-n`/`ctrl-p` in pickers only on macOS (`fastedit.toml`); the tests run
  with the macOS bindings, as the quick-open tests do. Binding them on Linux is a keymap change.
