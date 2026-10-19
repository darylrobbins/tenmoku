---
task_uid: "01m58zqesmmk9eeeysnf36zh56"
task: "T218"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "vim::remap_tests::expansion_runs_on_its_key: with nnoremap H ^, typing H moves the caret before the next keystroke, and 3H and dH (onoremap) work through the GPUI key path"
    required: true
    manual: false
  - id: "C2"
    text: "vim::remap_tests::prefix_owns_next_key: with a remap gb and a keymap binding on a key, after g the next keystroke goes to vim, not the keymap; after advancing the clock CHORD_TIMEOUT * 2 the prefix resolves as remap_timeout() does, and a click cancels it"
    required: true
    manual: false
  - id: "C3"
    text: "vim::remap_tests::mapped_write_leaves_host_command: nnoremap \u003cleader\u003ew :w\u003cCR\u003e leaves HostCommand::Write for the workbench (take_vim_host_command), while a macro replay's :w is still dropped"
    required: true
    manual: false
  - id: "C4"
    text: "vim::remap_tests::open_line_keeps_text: nnoremap \u003cleader\u003es :%s/ leaves the editor in cmdline mode with command_line_prefill() == \"%s/\""
    required: true
    manual: false
  - id: "C5"
    text: "view.rs is untouched (the timer lives on the Vim global), and the existing vim::macro_tests pass unchanged"
    required: true
    manual: false
  - id: "C6"
    text: "The two limits in the spec's Known limits section hold and are documented in the module docs of crates/fe-gpui/src/vim.rs: a map ends at its first host command with a status message (a vim::remap_tests test pins it), and shift, double and triple click handling is untouched"
    required: true
    manual: false
---

# Run vim remaps in the editor: expansions run in the same keystroke, a held prefix owns the next key and times out after CHORD_TIMEOUT, and a mapped :w runs (Mac T874)

{
  "mode": "inline",
  "body": "## Problem\n\nThe feed card runs remaps inside `VimState::feed`. It queues an expansion's keys through\n`take_replay` and holds a prefix with `remap_waiting()`. The settings card hands each editor\nits `RemapSet`. Typing in the app still doesn't run a remap. This is the record's card 3\n(the expansion runner, `docs/decisions/vim-remaps.md` §5, decided 2026-10-17). It covers\nthe timeout too, which the user chose to be `CHORD_TIMEOUT`. On main, in\n`crates/fe-gpui/src/vim.rs`:\n\n- `EditorView::feed_vim` calls `run_vim` (which ends in `replay_vim`) only on\n  `Step::Commit`. A key that resolves a remap returns `Step::Wait`, so its expansion would\n  sit queued until the next commit.\n- `live_stack` treats the editor as owning the keystroke only while\n  `state.pending().kind().is_some()`. A held remap prefix leaves `kind()` `None`, so the\n  keymap would take the next key.\n- `replay_key` types a replay's command-line keys into a line it doesn't show, and drops\n  the host command the line leaves: \"a macro replay doesn't run …\". For\n  `nnoremap <leader>w :w<CR>` that host command is the point. The workbench already runs\n  whatever host command an editor's keys leave: `Workbench::follow_vim_keys`\n  (`crates/fe-workbench/src/vim_host.rs`) observes the active editor and takes it.\n- At the end of a replay, a command line still open is dropped, with \"a macro replay ended\n  with the command line open\". `nnoremap <leader>s :%s/` must instead leave the line open\n  with `%s/` typed. The status bar's line (`fe-workbench/src/vim_line.rs`) opens with\n  `VimState::command_line_prefill()`.\n- Nothing calls `VimState::remap_timeout()`.\n\n## Change\n\nIn `crates/fe-gpui/src/vim.rs`, plus one setter in `crates/fe-vim/src/state.rs`.\n\n- `feed_vim` runs `replay_vim` after every fed key, not only after a commit, so an\n  expansion runs in the same keystroke.\n- `live_stack` reports pending while `remap_waiting()` is true. A remap prefix then owns\n  the next keystroke, as a pending vim command does (§4 rule 2).\n- **Timeout.** After a key leaves `remap_waiting()` true, start a `CHORD_TIMEOUT` timer.\n  Keep it on the `Vim` global with the editor's `WeakEntity`: one editor waits at a time,\n  and `view.rs` stays untouched (T802 holds it). Any key fed to that editor drops the timer.\n  When it fires, it calls `remap_timeout()`, then `replay_vim`, then `notify`. A click,\n  `cancel_vim` or a focus change cancels the prefix, which resolves as a timeout would.\n- **Host commands.** In `replay_key`, a host command left by a line that an *expansion*\n  submitted stays on the state for the workbench. A macro replay's is still dropped and\n  logged. Tell the two apart with the key tags the feed card added: expose\n  `VimState::expanding() -> bool` from `state.rs` if needed.\n- **Open line.** When an expansion ends with the command line open, `replay_vim` doesn't\n  drop its text. It sets it as the line's prefill through a new\n  `VimState::set_command_line_prefill(text)` in `state.rs`, so the status bar's line opens\n  showing it. `<Left>`/`<Right>` inside such a line aren't followed (known limit: the caret\n  starts at the line's end).\n\nTests go in the new `crates/fe-gpui/src/vim/remap_tests.rs`, declared in `vim.rs` next to\n`macro_tests`, on a GPUI `TestAppContext` with vim on and remaps set through\n`set_vim_remaps`. Advance the clock with `cx.executor().advance_clock(CHORD_TIMEOUT * 2)`,\nas `vim/tests.rs` does.",
  "ref": "",
  "ref_blob": ""
}

## Known limits (lead decision, 2026-10-19; these are NOT part of this card)

Four finishes of review kept asking for work this card's scope and criteria forbid or do not cover. The lead decided the following are out of scope here and handled by separate cards:

- **Host commands inside a longer map.** A map whose right side contains a host command (`:w<CR>dd`, `do:w<CR>`) runs the keys before it, hands the host command to the workbench exactly as C3 says, and ENDS the map there with a status message; the keys after the host command are not replayed. Suspending the replay and resuming it after the workbench's asynchronous result needs a completion protocol from fe-workbench (`vim_host.rs`, owned editors in `git_commands.rs`, Save As in `workbench.rs`) back to the editor; that is a separate card. Document the limit in the module docs of `crates/fe-gpui/src/vim.rs`.
- **Clicks and a held prefix.** A plain click cancels or resolves a held prefix as C2 says. Shift, double and triple click paths, and click offsets computed in `view.rs` before the prefix resolves, are `view.rs` work (held by another task) and a separate card; document it in the same module docs.

Review findings that only ask for these two are out of scope for this card.

## Implementation notes

- The wait (`RemapWait` on the `Vim` global) holds the timer task and a
  `cx.on_blur` subscription for the editor's focus handle, made in `vim.rs`
  from the window `feed_vim` has, so the focus change needs nothing in
  `view.rs`. When the wait ends with keys still held (a timeout's trailing
  keys start another map), it is armed again.
- `VimState::expanding()` is whether the last key read (through `feed` or
  `record_key`) was a map's; `replay_key` reads it right after the
  command-line key's `record_key`, before the line runs.
- `cancel_vim` resolves the held keys as a timeout, then feeds `<Esc>` if
  anything is still pending or held, so a click leaves nothing pending.
- `set_vim_remaps` replays at once the keys a new set releases unmapped,
  rather than leaving them for the next key.
- In tests, `g` first goes to the default keymap's `g d` chords: it reaches
  vim after `CHORD_TIMEOUT`, and only then does the remap's wait start.
- Review round 1: `reduce` reads an arrow with nothing pending in normal
  mode as `h l k j`, so a replay feeds arrows through `feed` and a map's
  arrows are remapped; every replay arms the wait once its update is over
  (`wait_for_remap_later`), so a `@` the keymap dispatched waits too; a new
  wait resolves another editor's; and only the editor focused in the active
  window holds the wait.
- Host commands (review rounds 1-3): the host runs a file or window command
  only after the keystroke's update, asynchronously for writes, so a map's
  ex line that leaves one **ends the map** there (`replay_key` stops the
  replay, `handed`). The host runs that one command exactly as a typed
  line's. Keys after it are dropped, with the message "a map's keys after a
  file or window command aren't run" (known limit: `:w<CR>:q<CR>` writes
  only; `:wq` and `:x` do both). Running the rest after the command would
  mean suspending the replay across the workbench's async writes, owned
  editors' writes and every command's success, a card of its own. A
  macro's host command is still dropped, also when a map plays the macro.
