---
task_uid: "01m55a7005shkd3dfc70y7g7e3"
task: "T154"
rev: 3
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "git_commands.rs's registry and op(), and the History and Git Log panels' workbench-context registries, are gone, run_git_command's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists the 19 git_commands.rs commands and the History and Git Log panel workbench commands with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "an ex command (:Gwrite, :Gitsigns stage_hunk) still reaches run_git_command through fe-vim's host commands, as the existing git_commands_tests.rs proves unedited; cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the git commands and the History and Git Log panels' workbench commands to #[kiln::command] methods, with :G and :Gitsigns still routed through fe-vim (Mac T704)

## Problem

The git command registry (`crates/fe-workbench/src/git_commands.rs` ~281-330, from T550: `registry()`, `op()`, `GitCommand`, the `run_git_command` arm) dispatches 19 commands through `fe_command` (`git.stage_file`, `git.unstage_file`, `git.stage_hunk`, `git.unstage_hunk`, `git.revert_hunk`, `git.discard_file`, `git.reset_buffer`, `git.commit`, `git.commit_amend`, `git.create_branch`, `git.switch_branch`, `git.diff_head`, `git.diff_index`, `diff.compare_file`, `git.next_change`, `git.previous_change`, `git.blame_line`, `git.toggle_line_blame`, `git.open_line_commit`), and the History and Git Log panels add theirs (T590, T609, T610: `git_history.rs`, `git_log.rs`). The fugitive and gitsigns ex commands reach the same ops through fe-vim's host commands (`Workbench::follow_vim_keys`, `git_commands::Routed`).

## Change

Move every `git_commands.rs` command and the History and Git Log panels' workbench-context commands to methods in `workbench_commands.rs` with their old IDs, titles and contexts, calling `run_git_command` with the `GitCommand` each maps to. `GitCommand` and the vim routing stay: an ex command (`:Gwrite`, `:Gitsigns stage_hunk`, ...) still reaches `run_git_command` through fe-vim's host commands, and `git_routed` still records it. A panel's own-context commands (a list's select and open keys) stay with the panel, as T696 leaves the search panel's. Nothing in `crates/fe-vim` or `crates/fe-git` changes.

## How a command family moves (T693 sets this up; every later family card repeats it)

- Each command becomes a method marked `#[command(id = "<old id>")]` in the one
  `#[kiln::command(context = "workbench")] impl Workbench` block in `crates/fe-workbench/src/workbench_commands.rs`.
  A view gets one `ViewCommands` impl, so every family's Workbench methods go in that block; a method's body calls
  the family's existing function (`self.run_pane_op(..)`'s body, moved or called). Its doc line is the old title;
  a command whose old context wasn't `workbench` gives `context = ".."` on the method.
- Delete the family's `CommandTarget`, `Commands` global, `init`/`registry()`/`op()`, its arm in
  `Workbench::on_run_command` (`workbench.rs`), and its lines in `command_catalog()` and `command_list()`
  (`lib.rs`). Its `Op` enum goes too unless something other than command dispatch uses it (a rail click, a
  toolbar button): then it stays, only the registry goes.
- Append the family's IDs, titles and contexts to the table in `crates/fe-workbench/src/typed_command_ids_tests.rs`.

## Rules for every card of this chain

Phase 3 of the Kiln app layer (`docs/decisions/kiln-app-layer.md`, approved 2026-10-13). The chain runs serially,
in this order: T700 (kiln), T690, T691, T693, T694, T695, T697, T698, T699, T701, T702, T703, T692, T696, T704,
T705, T706.

- No command ID or key context is renamed (`app-framework.md` §4): every moved command keeps its ID with
  `id = ".."` and its context with `context = ".."`.
- No existing fastedit test is edited; behaviour is proven by the existing suites plus the new test file each
  card names.
- Nothing on the paint path changes: `EditorElement`, `TerminalElement` and `FileViewerElement` stay raw
  `impl Element`.
- The keymap and menus stay TOML tables (`crates/fe-keymap/keymaps/*.toml`, `menus/*.toml`), untouched.
- `crates/fastedit/src/main.rs` is not touched: `fe_workbench::init` registers what the chain adds (neon's T707
  owns `main.rs` and the keymap files).


## Placement (Mac lead, note zzr)

Argon (T702 find bar, T703 terminal panel) and neon (T696 sidebar panels) are moving their own subsystems' commands the same way at the same time, in the same files. To keep the Mac's stack cherry-picks clean, add the new methods as one contiguous group at a distinct place inside workbench_commands.rs's impl (not at the end of the impl), and keep lib.rs and typed_command_ids_tests.rs edits to this card's own entries. No dependency on T696: its place before this card in argon's chain was landing order only.

## Implementation notes

- The History and Git Log panels had no registry of their own: their workbench-context commands
  (`git.file_history`, `git.log`, `git.log_all`) were the last three rows of `git_commands.rs`'s `COMMANDS`, so they
  move with the other 19, 22 methods in all, grouped after `pane.close` in `workbench_commands.rs`. Each calls
  `run_git_command` with its `GitCommand`. `git_history.rs` and `git_log.rs` are unchanged: their list keys are
  raw key handling (`commit_rows`), not registry commands.
- `COMMANDS` goes with the registry; `GitCommand::title()`, which the `Not supported yet` answers name, is now a
  match with the same titles.
- `git_commands.rs`'s registry also held the Changes panel's commit box key, `git.commit_box.commit`, in the
  `git_panel` context. It stays a registry command: a key or the palette sends it as a `RunCommand` that
  `GitPanel::on_run_command` takes from the box, and a typed command would be dispatched as its own action, past
  that handler. Its registry moves to `lib.rs` (`commit_box_registry()`), in the same place in `command_catalog()`
  and `command_list()`, rather than into `git_panel.rs`, whose scope would bring in an unrelated earlier finding
  (T126 F10).
- Two methods are named apart from existing `Workbench` methods: `diff_compare_file` (`git_diff.rs` has
  `compare_file`) and the helper `run_typed_git` (`git_stage.rs` has `run_git`).
