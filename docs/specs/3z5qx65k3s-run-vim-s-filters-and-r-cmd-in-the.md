---
task_uid: "01m55rk14byh3rfk3z5qx65k3s"
task: "T167"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test workbench::vim_host_tests::vim_filter_tests::filter_runs_through_the_shell (new crates/fe-workbench/src/vim_filter_tests.rs, declared in crates/fe-workbench/src/vim_host_tests.rs) passes: in a real workbench in a trusted folder with vim.shell /bin/sh, :%!sort sorts the buffer and 2!!tr a-z A-Z upcases two rows, each one undo step that u restores; :r !printf 'x\\ny\\n' puts two rows below the line; and right after submitting :%!sleep 1; sort the buffer is unchanged and the window still handles input, sorted once the command ends."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test workbench::vim_host_tests::vim_filter_tests::filter_refusals_keep_the_buffer passes: in an untrusted folder :%!touch ran leaves a message and no file ran; a HostCommand::Filter carrying Actor::Host runs nothing; sh -c 'echo out; echo err \u003e\u00262; exit 3' leaves the rows unchanged, the message shell returned 3 and an error toast whose detail holds err; an edit made while the command runs keeps the buffer and leaves a message; a command still running at vim.filter_time_limit (shortened for the test) is killed and changes nothing; workbench.cancel_vim_filter kills a running one and changes nothing."
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test workbench::vim_host_tests::vim_filter_tests::file_names_are_shell_escaped passes: with the buffer's file named 'a b; rm -rf x.txt' in a trusted root that also holds a file x, :.!echo % leaves the row a b; rm -rf x.txt and x still exists; :.!echo # leaves E194; % in an untitled buffer leaves E499."
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test workbench::vim_host_tests::vim_filter_tests::shell_resolution passes: resolve_shell gives the vim.shell setting when set; else $SHELL with -c when it is absolute, /bin/sh when it is unset or relative; and for the Windows platform (as data, on every host) %COMSPEC% or cmd.exe with /C, where % and # are refused."
    required: true
    manual: false
---

# Run vim's ! filters and :r !cmd in the workbench: the user's own keys only, in a trusted folder, through vim.shell off the UI thread with a time limit and a cancel, file names shell-escaped (Mac T909)

Filed on argon from Mac T909 (card ~/coordination/argon-cards/zzq/T909.json rev 2; note zzq, Vim parity after the user's decisions). Mac task uid 01m55mf0pfcas6kmdwn6p2gcww. Base: argon trunk 7edfe4f (= mac/main c7b6a04 synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T166 (Mac T908), T165 (Mac T912). Held in the inbox: waits until Mac T864, Mac T865 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T909)".

## Problem

T908 turns `!{motion}`, `:{range}!{cmd}` and `:r !{cmd}` into `HostCommand::Filter { rows, command, actor }`
and `HostCommand::ReadCommand { below, command, actor }`, which no host runs yet. The user approved `!` in
the "Vim parity" track on 2026-10-17 and, the same day, set its guardrails:

1. Trusted folders only (fastedit's folder trust), with the user's shell from a `vim.shell` setting
   (default: `$SHELL` on Unix, falling back to `/bin/sh`; Windows: `%COMSPEC%`).
2. Only from the user's own keystrokes: one from a plugin, an AI agent or workspace-provided config is refused.
3. `%` and `#` expand to shell-escaped file names, unlike vim.
4. Off the UI thread, with a timeout (a setting) and a cancel; the whole filter one undo step; a non-zero exit
   shows stderr and leaves the text unchanged.

On a failing command nvim 0.12.6 replaces the rows with the command's stdout and stderr together and says
`shell returned N` (probed: `:%!sh -c "echo out; echo err >&2; exit 3"` on `a b c` leaves `out|err`,
`v:shell_error` 3; `:%!nonexistentcmd` leaves the shell's `command not found` line). Here we deliberately
differ: the rows keep their text. On success nvim also takes stderr into the rows ('shellredir' `2>&1`);
here only stdout goes in.

Trust lives in kiln-shell, read through `Workbench::root_trusted` (`crates/fe-workbench/src/trust.rs`).
The settings card before this one adds `vim.shell` and `vim.filter_time_limit` and keeps a project file
from setting `vim.shell` or `vim.remap`; the workbench reads them with `config::settings::<Settings>(cx)` when
a filter starts. Toasts take a detail and an action (`crates/fe-workbench/src/toast.rs`).

## Change

- **Shell.** A pure `resolve_shell(setting, platform, env)` in a new `crates/fe-workbench/src/vim_filter.rs`
  (declared from `crates/fe-workbench/src/vim_host.rs` by `#[path = "vim_filter.rs"] mod vim_filter;`): the
  setting if set; else on Unix `$SHELL` if it is an absolute path, else `/bin/sh`, run as `<shell> -c
  <command>`; on Windows `%COMSPEC%`, else `cmd.exe`, as `<shell> /C <command>`. `PATH` is
  `fe_lsp::LoginPath::system()`'s when it has one, so tools from the login shell's `PATH` are found when
  fastedit starts from the Dock.
- **Refusals, before anything runs**, each a vim message and nothing spawned: the actor isn't
  `Actor::User` (`Shell commands run only from your own keys`); the window's root isn't trusted, or the
  buffer's file isn't under it (`Shell commands run only in a trusted folder`); a filter already running in
  that editor.
- **Expansion.** `%` becomes the buffer's path relative to the root (absolute when outside it); an untitled
  buffer fails nvim's E499. `#` fails E194 as nvim does with no alternate file: fastedit keeps none yet; a
  later `<C-^>` card fills it through the same escaping. On Unix each name is single-quoted (`'` as `'\''`);
  on Windows `%` and `#` are refused (cmd.exe's quoting isn't safe to build), with a message.
- **Running** on the background executor, in the root (nvim's working directory, where `%` is relative to),
  with the rows' text on stdin written from a thread of its own (a command that never reads stdin can't
  block), stdout and stderr read apart, each capped at 64 MiB (fe-vim's put limit; past it the run is
  killed and refused). The process gets a process group of its own. Past `vim.filter_time_limit` it is
  killed (`SIGTERM`, then `SIGKILL` after 1 s) and nothing changes. While it runs past 500 ms an info toast
  `Filtering through <command>…` carries a Cancel action, the new typed command `workbench.cancel_vim_filter` ("Vim:
  Cancel Filter", in `crates/fe-workbench/src/workbench_commands.rs`), which kills it the same way; closing
  the editor's tab cancels too.
- **Answer.** Exit 0: the rows take stdout through `VimState::apply_filter` (T908), one undo step, or
  `:r !` puts it through `VimState::apply_read_command`; any stderr shows in a warning toast's detail. Non-zero
  exit: nothing changes, the vim message is `shell returned N`, and an error toast `<command> exited with N`
  shows stderr as its detail. If the buffer changed while the command ran, nothing changes and a message
  says so.

Tests in a new `crates/fe-workbench/src/vim_filter_tests.rs`, declared in the existing
`crates/fe-workbench/src/vim_host_tests.rs` by `#[path = "vim_filter_tests.rs"] mod vim_filter_tests;`, in a
real workbench with `vim.shell` set to `/bin/sh`.

Order: vim_host.rs and vim_host_tests.rs are changed by T833, T864 and T865; workbench_commands.rs by T820,
T848 and T852 (unrelated; the scheduler keeps them apart). This card depends on T908, the filter settings
card, T864 (after T833) and T865.

## Implementation notes

- **fe-gpui and toast.rs.** The workbench had no way to reach an editor's `VimState` mutably, so
  `EditorView::update_vim` (`crates/fe-gpui/src/vim.rs`) runs a closure on the state and the model and
  then repaints. `ToastLayer::push` (`crates/fe-workbench/src/toast.rs`) is now the workbench's, handing
  back the toast's id, so a command's toasts show in its own window and its Cancel toast is dismissed when
  it ends. The scope was widened by respec for both.
- **Thread, not the background executor.** The command runs on a `std::thread` of its own, which waits on
  the process with real time for `vim.filter_time_limit`, the 64 MiB caps (checked again once the readers
  end, before the leader is reaped, and stopped as any run past the cap) and the cancel flag, and hands its outcome back through a channel the workbench's task awaits. On
  gpui's background executor a test's deterministic scheduler would run the blocking wait inline; a thread
  keeps the window taking input while it runs.
- **Stopping.** `SIGTERM` to the group, then after 1 s `SIGKILL` to whatever is left of it, always, from a
  cleanup thread; the leader is probed with `waitid(WNOWAIT)` and reaped only after the `SIGKILL`, so the
  group's id can't have passed to another process. A descendant that ignores `SIGTERM` and redirects its
  streams dies too.
- **Final line break.** fe-vim counts the empty row after a buffer's final line break as a line (its `%`
  of `c\na\nb\n` is rows 0..4). nvim has no such line, so the host takes a range on it as on the row
  before and puts `:$r !` output above it: `:%!sort` sorts the file's three lines and keeps its line break,
  and `:$!` filters the last line.
- **`%`'s quoting** is one quoted word for the shell's family, POSIX (`'` as `'\''`) or fish (`\` and `'`
  escaped), and it goes only where that is one literal word without parsing any shell's grammar: a command
  with `%` may hold none of `'`, `"`, `` ` ``, `$`, `\`, `(`, `)`, `{`, `}`, `#` or `<<` anywhere (so
  nothing can open quotes, a substitution, an expansion, a zsh glob qualifier, a group, a comment or a
  here-document around it), and `%` may not follow `~` or a word-starting `=` (zsh's `=cmd`). Nor may the
  file's name hold a line break or another control character: review showed a name with line breaks
  ending a `#` comment (typed as `\#`) and running its next line. A scanner for the shells' syntax was tried first and review
  found a `case` pattern inside `$(…)` that fooled it, so this is the conservative rule: `$(basename %)`
  is refused, with a message, and a command without `%` runs as typed in any shell. Another shell (`tcsh`,
  `nu`, …) is refused a `%`.
  The quoting makes the name one literal word; a command that runs its arguments as code (`eval %`,
  `sh -c %`, bash arithmetic such as `[[ 1 -eq % ]]`) runs it as it would any argument the user typed.
- **A command that ends** is reaped then, as nvim's is: what it left running in the background with its
  streams elsewhere goes on. Only a stopped command's group is killed.
- **Trust** compares real paths: a file whose path, symlinks followed, isn't under the root's real path is
  refused, whatever its path says. A path that doesn't resolve counts only when nothing at all is there (not
  even a dangling symlink) and its folder resolves: a new file's name in a real folder. A dangling link, or
  a file in a folder not made yet, is refused.
- **Running state** is a gpui global keyed by editor (`workbench.rs` is outside the scope), made with two
  hooks: `on_window_closed` cancels the commands of a closing window at once, whoever still holds its
  workbench; `on_app_quit` (fastedit quits as its last window closes) stops every process. The processes
  are listed apart from the UI's runs (`Lives`): from the moment a command is typed (before its thread has
  started the shell) until its group's leader is reaped, so a cancelled command whose result was already
  shown is still listed while its group waits for `SIGKILL`. The quit marks the list quitting under its
  lock, which the shell's spawn also takes, so nothing starts after; sends every listed group `SIGTERM`;
  waits for the runs' own stops to kill and reap them, at most the 1 s grace and 200 ms; and `SIGKILL`s
  what's left before quitting goes on. A watch every 100 ms on the UI thread cancels a command once its editor's
  tab has closed or the editor has gone, and shows the Cancel toast after 500 ms; the global entry goes
  when the command ends even with no window left to show it.
- **Mode at the end.** Output that lands while vim isn't in normal mode (insert, replace, visual, a pending
  operator, the command line) is dropped with a message, as an edit during the run is: applied there it
  would join the insert's undo step.
- **Cancel.** The toast's Cancel is a button bound to its own command (a toast question), not the typed
  command, so it can't cancel another one that started since. `workbench.cancel_vim_filter` ("Vim: Cancel
  Filter") cancels the focused editor's command, or with none there, every one in the window.
- Also refused before running: a read-only editor (`E21`), and a file name that isn't UTF-8 when `%`
  names it.
