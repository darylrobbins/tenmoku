---
task_uid: "01m5qphxfr3tmsqdwnpq82j3eh"
task: "T327"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests/ex_windows.rs: ex::parse gives HostCommand::Window(..) for :sp[lit] [file], :vs[plit] [file], :new, :vne[w], :on[ly][!], :clo[se][!] with nvim's shortest abbreviations (expected values stated from nvim 0.12.4), :[N]sp / :[N]vs counts accepted and ignored with a comment, and :sview/:sfind remain E492"
    required: true
    manual: false
  - id: "C2"
    text: "vim_windows_tests::split: :sp splits the active pane downward and :vsp to the right, as pane.split_down / pane.split_right do, the new pane showing the same file (not a copy of unsaved edits: the same buffer) and active; :sp other.rs opens other.rs in the new pane; a missing file gives the E484-style message and no split"
    required: true
    manual: false
  - id: "C3"
    text: "vim_windows_tests::new: :new splits down and :vnew right with an untitled empty tab, focused"
    required: true
    manual: false
  - id: "C4"
    text: "vim_windows_tests::only_close: :only closes every other pane without closing tabs' files that are dirty (a dirty tab moves into the surviving pane rather than being discarded; E445 style message if it cannot) and :close closes the active pane but not the last one (E444); neither quits the window or app"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim -p fe-workbench and cargo clippy -p fe-vim -p fe-workbench --all-targets -- -D warnings pass; the oracle suites are untouched (the oracle cannot see windows), the abbreviation facts live in the parse test"
    required: true
    manual: false
---

# Vim window ex commands: :sp, :vsp, :new, :vnew, :only and :close (and :clo, :on) run as the pane commands, with :sp {file} and :vsp {file} opening the file in the new pane (Mac T1139)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" The <C-w> keys split and close panes (T605), but the ex commands for windows, :sp :vsp :new :vnew :only :close, which vim users type and which plugin docs assume, are not recognised.

## Current state

- fastedit.toml:1604+ T605: ctrl-w s/v/c/o... bound to pane.split_down/right/close; the pane commands are in workbench_commands.rs:61-135 (`Workbench::split`, `close pane`) and pane_group.rs. `:only` has a ctrl-w o equivalent only if T605 bound it (verify; if it did, reuse its command).
- ex.rs parses no window command; `HostCommand` has no variant. The host side is vim_host.rs `run_vim_host_command`.
- This card follows the buffer-command card (T1136) because both edit ex.rs, ex/complete.rs and vim_host.rs; do not start before it merges.

## Change

- fe-vim: ex/windows.rs with `WindowCommand { Split{vertical, file}, New{vertical}, Only{force}, Close{force} }` and `HostCommand::Window(WindowCommand)`; completion names.
- fastedit: vim_windows.rs implementing them on top of the existing pane commands (no new pane model). `:close` of a pane with dirty tabs follows pane.close's rules; `:only` moves tabs rather than discards.
- Copy nvim: `:sp` splits above/below per 'splitbelow' (off: new window above in nvim; fastedit's pane.split_down puts the new pane below, keep that VS Code geometry and say so in a code comment), `:vsp` to the right of nvim's default left? nvim default is the new window at left; keep fastedit's right for consistency with <C-w>v (T605).

## Tests

New files fe-vim tests/ex_windows.rs and fe-workbench vim_windows_tests.rs (declared from vim_windows.rs), headless.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- T605 bound no `<C-w>o`, so `:only` is new code in vim_windows.rs; `:sp`, `:vs` and `:clo` reuse `Workbench::split` and `close_active_pane` (pane.split_down/right, pane.close).
- The scope gained crates/fe-vim/src/lib.rs (the `WindowCommand` re-export and crate docs) and fe-workbench's git_commit.rs and git_diff.rs, whose `HostCommand` matches are exhaustive.
- `:sp {file}`/`:vs {file}` split, then open the file in the new pane and drop the split's second view of the current file (it shares its buffer, so nothing is lost), so the new pane holds the file alone. `:sp` of the file already shown is plain `:sp`. `:new {file}`/`:vne {file}` are read as `:sp {file}`/`:vs {file}`.
- `:only` moves each other pane's dirty tab into the current pane unless that pane already shows its text (nvim's 'hidden' keeps such buffers); clean tabs close with their panes. The current pane keeps the tab it showed. With the moves, a pane is left dirty only if a move failed, which a move into a pane of the group never does; E445 covers that case, but no test can reach it.
- `:on N`, `:Non`, `:Nclo` (window numbers) are refused as unsupported; counts before `:sp`/`:vs`/`:new`/`:vne` are ignored. A `|` after a window command, and one run from `:g` or `:normal`, is refused, as for the buffer commands.
- The workbench tests use .txt files, not .rs: no language server starts.
