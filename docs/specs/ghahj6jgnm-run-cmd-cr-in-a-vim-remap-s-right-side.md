---
task_uid: "01m58zqf7j7fwd10ghahj6jgnm"
task: "T220"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "remap_cmd_runs_ex_lines (top level, crates/fe-vim/tests/remap_cmd.rs): \u003cCmd\u003es/a/b/\u003cCR\u003e substitutes without entering cmdline mode or adding to the : history, a visual selection is kept around it, the keys after it run, and an unclosed \u003cCmd\u003e is a RemapIssue"
    required: true
    manual: false
  - id: "C2"
    text: "remap_cmd_leaves_fastedit_commands: \u003cCmd\u003efe:editor.save\u003cCR\u003e leaves a RemapCommand for editor.save that take_remap_command() returns once, and an invalid ID is a RemapIssue"
    required: true
    manual: false
  - id: "C3"
    text: "vim::remap_cmd_tests::fe_command_dispatches: in the GPUI editor the fe: command runs with Source::Keyboard in the stamped context, an unknown command shows E492: Not an editor command: fe:…, and \u003cCmd\u003ew\u003cCR\u003e leaves HostCommand::Write for the workbench"
    required: true
    manual: false
  - id: "C4"
    text: "args.rs is untouched (vim.ex_line's args struct is in remap.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "vim::remap_cmd_tests::fe_in_normal_stops_with_a_message: a :normal whose keys hold \u003cCmd\u003efe:editor.save\u003cCR\u003e in the middle runs the keys before it, shows \"fe: commands don't run inside :normal\", does not run the command, and runs nothing after it (also over a range); and vim::remap_cmd_tests::workbench_registry_commands_read_as_unknown: a map with fe:project_panel.focus shows E492: Not an editor command: fe:project_panel.focus"
    required: true
    manual: false
---

# Run <Cmd>…<CR> in a vim remap's right side without leaving the mode: an ex line, or fe:<command id> dispatched as a key binding is (Mac T876)

## Problem

The user answered yes to question 5 of `docs/decisions/vim-remaps.md` (decided 2026-10-17):
a remap's right side may run a fastedit command, `<Cmd>fe:<command id><CR>`, in a card after
the core engine. This is the record's card 7 (§6). The remap set card rejects `<Cmd>` in a
`to` as "not supported yet". nvim's `<Cmd>…<CR>` runs an ex line without changing mode and
without the command line: no history, no prefill, a visual selection kept.

fe-vim has no command that runs an ex line without the line. `cmdline::submit` closes an
open line, adds it to history and calls `ex::run`. A fastedit command is dispatched from the
editor as the keymap dispatches one, a `RunCommand` with `Source::Keyboard` through the
focused element (`crates/fe-gpui/src/keymap.rs`; kiln's `keys`). `commands.rs` is held by
the lent T836, hence the dependency.

## Change

- **fe-vim `remap.rs`.** A `to` may hold `<Cmd>{text}<CR>` pieces among its keys. `{text}`
  is an ex line, or `fe:{command id}` with a valid `fe_command` ID. An unclosed `<Cmd>`, or a
  `<Cmd>` in a `from`, is an issue. Replace the "not supported yet" issue.
- **Running a `<Cmd>` piece.** When the expansion reaches the piece, `feed` commits:
  - for an ex line, a new `vim.ex_line { text }`, registered in `commands.rs`, with its
    args struct in `remap.rs` (`args.rs` is held by lent cards). It runs
    `ex::run` without opening, closing or recording a command line. Mode, the visual
    selection and the history are unchanged, but an ex command that itself changes mode
    still does.
  - for `fe:`, nothing in fe-vim: the state leaves a `RemapCommand(Invocation)` for the host
    to take with `VimState::take_remap_command()`.

  Either way the rest of the expansion goes on after it, in order.
- **fe-gpui `vim.rs`.** `replay_vim` and `feed_vim` take a `RemapCommand` after each key and
  dispatch it with `Source::Keyboard` in the editor's stamped context, as a keymap binding
  is dispatched. `<Cmd>fe:editor.save<CR>` saves, and an unknown ID shows the vim message
  `E492: Not an editor command: fe:…`.
- A `vim.ex_line` that leaves a host command (`<Cmd>w<CR>`) has it run by the workbench, as
  the editor-runs-remaps card does for a typed line.

Tests go in the new top-level `crates/fe-vim/tests/remap_cmd.rs` (headless) and the new
`crates/fe-gpui/src/vim/remap_cmd_tests.rs`, declared in `vim.rs`.

## What this card does NOT do (lead decision, 2026-10-19; these are defined behaviours, not defects)

Review kept asking for two capabilities the card does not promise. They are follow-ups (T226, T227), and what happens in their place is specified and tested here:

- **`fe:` inside `:normal`.** A `:normal` whose keys contain a `<Cmd>fe:<command id><CR>` piece STOPS at that piece: nothing after it (keys or range iterations) runs, and the vim message `fe: commands don't run inside :normal` is shown. It never half-runs and never runs the command. (A resumable `:normal` runner is T226.)
- **Workbench registry commands.** `fe:` runs the commands the editor itself knows: fe-gpui's editor and vim catalogs plus typed and app commands. A command that only the workbench's panel registry has (`project_panel.*`, search panels) reads as unknown and shows `E492: Not an editor command: fe:…` like any unknown id. (Handing the host's full catalog to fe-gpui, and stopping a map on an editor command's runtime failure, is T227.)

Review findings that only ask for those two capabilities are out of scope for this card.

## Implementation notes

- **Scope.** Widened by respec to `crates/fe-vim/tests/remap_set.rs` (its
  bad-entry list had `<Cmd>w<CR>`, now valid; an unclosed `<Cmd>w` takes its
  place), `crates/fe-vim/src/lib.rs` (re-exports `RemapCommand` and
  `ExLineArgs`), `crates/fe-vim/src/headless.rs` and
  `crates/fe-vim/src/ex/normal.rs` (route a `<Cmd>` key), and
  `crates/fe-gpui/src/view/host.rs` (read-only judges `vim.ex_line` by its
  line), which brought T224's F1 along: `scroll_to_buffer_line` now keeps
  the fraction within the last buffer row, tested in
  `view/host/scroll_line_tests.rs`.
- **How a `<Cmd>` travels.** `RemapSet` keeps an entry's `<Cmd>` pieces
  beside its keys, by how many keys come before each, so `Lookup`'s `to`
  is still the keys alone. When the map runs, each piece is queued among the
  keys as a placeholder key (`CMD_KEY`, `<C-@>`) whose tag carries the
  piece; the host feeds it back through `VimState::feed` like any other
  key, which runs it. A host routes keys by mode, so
  `VimState::remap_command_next()` says the next key is a `<Cmd>`, to go to
  `feed` in insert and `cmdline` mode too: the GPUI editor, `HeadlessVim`
  and `:normal`'s runner do so. It skips a `:s` confirmation and jump labels
  waiting, and running ends them.
- **Pending input.** A `<Cmd>` drops what was pending before it (count,
  register, operator), as a completed command does. nvim instead applies a
  pending operator to the cursor movement the command makes
  (`d<Cmd>…<CR>` as a custom motion); fe-vim doesn't.
- **Parsing.** The text is read as keys (`<lt>`, `<Bar>`, `<leader>`) up to
  the first `<CR>`; a key that isn't a character before it is an issue, as
  is a `<Cmd>` with no `<CR>`. `fe:` takes a `CommandId`; an invalid one is
  an issue, while a valid ID no command has is the host's E492.
- **What `vim.ex_line` keeps.** No `:` history and no `:` register (`@:`),
  as nvim's `<Cmd>`; a pattern the line sets still goes into the search
  history, as nvim's `:s` adds it whichever way it runs.
- **fe: in the editor.** The state queues `RemapCommand`s. When the
  editor's replay reaches one it holds, and once the editor's update is
  over the command is checked (E492 for an unknown ID, a command needing
  args, a context predicate that doesn't hold, read-only's E21) and
  dispatched as the keymap dispatches a binding, both in the context a key
  would be matched in: the stack the replay's keys came in, with the
  editor's frame stamped live (vim, the completion menu, the suggestion).
  That origin is the keystroke's window and stack, or for keys a wait held
  and a timeout or blur resolved, those of the keystroke that started the
  wait, carried on to every later piece, to keys that a later piece's
  suffix holds again and resolve at once, and to an editor whose wait
  another's replaces; a replay that starts some other way (a `@` the keymap
  dispatched) takes where the editor's last keystroke came in, noted as the
  keymap matches it. With the editor still focused
  there the command goes through `dispatch_invocation` from the focused
  element, so `editor.save` saves as `cmd-s` does and the workbench's and
  the app's commands run too; with focus gone elsewhere the editor runs its
  own commands itself, in its own window, and others go to that window's
  focused element. The rest of the map then goes on from a deferral queued
  after the dispatch, so it follows even a dispatch the window defers
  itself, and after any keys the command replayed. A synchronous dispatch
  from inside the editor's update would re-enter it. A command that fails
  its check or dispatch, leaves a host command (`vim.diff_get`) or switches
  vim ends the map. Whether it left a host command is noted as it runs,
  since the workbench takes the command as the editor notifies, before the
  replay resumes. While a replay holds for a command, the editor owns every keystroke
  (as with a vim command pending) and holds it, a chord's breaking key or
  its replayed keys included, then dispatches them again in order, as
  typed, once the map is over: so none comes between the map's keys or
  changes the context the command runs in.
- **What the card leaves to T226 and T227 (lead, rev 6).** A `:normal`
  whose keys leave a `<Cmd>fe:…<CR>` command stops right there: the
  runner drops the command and every key and line after it, and the
  `:normal` fails with `fe: commands don't run inside :normal`, which also
  stops a map that ran it, and a `:normal` it ran in (over its range too). Commands left before the `:normal` and not yet
  taken stay for the host. An `fe:` ID only a workbench panel's registry
  has (`project_panel.*`) reads as unknown: E492. An editor command's
  runtime error (as against bad args, context or read-only) is only
  logged, as a key's is.
