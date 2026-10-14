---
task_uid: "01m4xqgjkxfkc7r4jjwe0s1pgt"
task: "T122"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_commands_tests::palette_lists: the palette lists every command id in the card with its title, and running one with no feature behind it shows 'Not supported yet: \u003ctitle\u003e'"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test git_commands_tests::ex_routes: in a vim editor, ':Gdiffsplit', ':Gdiffsplit HEAD', ':Gwrite', ':Gread', ':Git commit --amend', ':G blame', ':Git switch -c x', \":'\u003c,'\u003eGitsigns stage_hunk\" and ':Gitsigns toggle_current_line_blame' each reach the matching GitOp (with args and range); ':Foo' answers 'E492: Not an editor command: Foo'; ':Git frobnicate' answers 'Not supported: :Git frobnicate'"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_commands_tests::owned_editors: a host command (':w', 'do') typed in an editor registered with own_editor(DiffView) reaches git_diff::host_command, one registered as CommitBox reaches git_commit::host_command; an unowned non-tab editor's is ignored; 'do' in a tab editor answers 'E99: Current buffer is not in diff mode'"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test git_commands_tests::active_editor_in_diff_view: with a DiffView active and its left side focused, Workbench::active_editor is the left EditorView, and the status bar shows its vim mode"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Route the git and diff commands: one git command registry in the palette, fugitive and gitsigns ex commands, and host commands from diff-view and commit-box editors, each to a stub per feature (Mac T550)

## Problem

The git and diff features the user asked for (2026-10-12) need commands in the palette and, per the vim-mode
requirement (coordinator, same day), ex commands from vim normal mode. Each feature's wiring card would otherwise
edit the same hot files: `crates/fe-workbench/src/workbench.rs` (`on_run_command` ~:1586), `crates/fe-workbench/src/lib.rs`
(`init`, `command_list`), and `crates/fe-workbench/src/vim_host.rs` (which drops host commands from editors that aren't a
tab's: `tab_of` is `None`). This card adds every command once, routed to a stub per feature, so the feature cards
only fill in their own module.

## Change

- New `crates/fe-workbench/src/git_commands.rs`: a `Registry` of the commands below in the workbench's
  `KEY_CONTEXT`, added to the palette (`command_list`), `git_commands::op(run, cx)`, and `Workbench::run_git_op`
  forwarding each to its module's stub. One `if let` in `on_run_command`, one `init` line in lib.rs.
- Stub modules, each `pub(crate) fn run(…)` answering `Not supported yet: <title>` (as vim_host's `RELOAD`) and
  `pub(crate) fn host_command(…)`: new files `crates/fe-workbench/src/git_diff.rs`, `git_stage.rs`, `git_commit.rs`,
  `git_blame.rs`, `git_branch.rs`.
- Commands (id: palette title: ex forms). Ex convention, stated in the module docs: fugitive's `:Git`/`:G`
  subcommands and `:G…` commands for files and the repository, gitsigns' `:Gitsigns <action>` for changes and line
  blame (ranges allowed), Vim's own `:diff…` commands.
  - diff (git_diff.rs): `git.diff_index` "Git: Compare with Index" `:Gdiffsplit`, `:Gvdiffsplit`; `git.diff_head`
    "Git: Compare with HEAD" `:Gdiffsplit HEAD`; `diff.compare_file` "Compare Active File With…" `:diffsplit {file}`;
    `git.next_change` / `git.previous_change` "Go to Next/Previous Change" `:Gitsigns next_hunk`/`prev_hunk`.
  - stage (git_stage.rs): `git.stage_file` "Git: Stage File" `:Gwrite`, `:Gitsigns stage_buffer`; `git.unstage_file`
    "Git: Unstage File" `:Gitsigns reset_buffer_index`; `git.discard_file` "Git: Discard File Changes";
    `git.reset_buffer` "Git: Revert Editor to Index" `:Gread`, `:Gitsigns reset_buffer`; `git.stage_hunk` "Git: Stage
    Change" `:Gitsigns stage_hunk`; `git.unstage_hunk` "Git: Undo Stage Change" `:Gitsigns undo_stage_hunk`;
    `git.revert_hunk` "Git: Revert Change" `:Gitsigns reset_hunk`.
  - commit (git_commit.rs): `git.commit` "Git: Commit Staged" `:Git commit`; `git.commit_amend` "Git: Commit Staged
    (Amend)" `:Git commit --amend`.
  - blame (git_blame.rs): `git.toggle_line_blame` "Git: Toggle Line Blame" `:Gitsigns toggle_current_line_blame`,
    `:Git blame`; `git.blame_line` "Git: Show Line Blame" `:Gitsigns blame_line`; `git.open_line_commit` "Git: Open
    Line's Commit Change" `:Gitsigns show_commit`.
  - branch (git_branch.rs): `git.switch_branch` "Git: Switch Branch…" `:Git switch`, `:Git checkout` (with a name,
    switch to it); `git.create_branch` "Git: Create Branch…" `:Git switch -c {name}`, `:Git checkout -b {name}`.
- `crates/fe-workbench/src/vim_host.rs`: `HostCommand::User` looks the name and args up in the table above and runs
  the command for that editor with its range; an unknown one answers `E492: Not an editor command: <line>`, an
  unknown `:Git` subcommand `Not supported: :Git <args>`. `DiffSplit` goes to `git_diff::run`. A host command from an
  editor that isn't a tab's goes to `git_commands::owned_host_command`, which hands it to the owner registered with
  `Workbench::own_editor(editor, Owner::{DiffView(WeakEntity<DiffView>), CommitBox})` (`git_diff::host_command` or
  `git_commit::host_command`); unowned, ignored as today.
- `crates/fe-workbench/src/workbench.rs`: `active_editor` returns the focused side's `EditorView` when the active item
  is a `DiffView` (the DiffView card), so the vim line, status bar items and editor commands target it.

## Tests

New file `crates/fe-workbench/src/git_commands_tests.rs` (declared `#[cfg(test)]` from git_commands.rs).

## Implementation notes

- The modules are declared from `workbench.rs` (`#[path]`, as `vim_host` is), so the feature cards reach the
  workbench's fields; lib.rs only adds `git_commands::init` and the registry to `command_list` and `command_catalog`.
- Answers: a command typed on the vim line answers there; one run from the palette or a key answers in a toast (the
  vim message only shows with vim on).
- Ex forms beyond the card's list: `:Gvdiffsplit HEAD`, and `:Git checkout {name}` / `:Git switch {name}` carry the
  name in `GitOp::args`. Only `:Gitsigns` forms take a range (others answer `E481: No range allowed`); a `!` on a
  known name, or arguments no form takes (`:Gdiffsplit main`, `:Git switch -c`), answer `Not supported: :<line>`.
- `do` / `dp` in normal mode leave a host command in the editor's vim state that nothing took before (only the line's
  submit did), so `'do' in a tab editor answers E99` needed `Workbench::follow_vim_keys`: it observes the active
  editor and runs the host command its keys leave.
- `Pane` (outside the scope) only holds editors in its tabs and draws `tabs[active]`, so a DiffView can't be shown in a
  pane yet: the tests push one into the pane's `PaneState` directly. `active_editor` finds it there and returns the
  side whose focus the workbench noted last as it rendered (focus leaving for the vim line keeps the side). Since it
  isn't drawn, the owned-editor tests hand `run_vim_host_command` the commands the line and keys would. The DiffView
  card makes `Pane` show it (and `Pane::active_editor`'s read-state check cover it).
