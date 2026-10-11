---
task_uid: "01m4nenk3aqghbsb74jf7fzkpd"
task: "T352"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::tests::macro_replay::recorded_macro_replays (new file crates/fe-gpui/src/vim/tests/macro_replay.rs): in a headless window with vim on, on \"abc\\nabc\\nabc\" from (0,0), the keystrokes q a x j q then @ a leave \"bc\\nbc\\nabc\" with the caret at (2,0), then @ @ leaves \"bc\\nbc\\nbc\". At the base it fails: the text stays \"bc\\nabc\\nabc\""
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::tests::macro_replay::replay_types_in_insert_mode: with register a holding Axy (yanked with \"ay$ or set through the app's vim registers), on \"Axy\\nfoo\" with the caret on row 1, @ a leaves \"Axy\\nfooxy\" in insert mode. At the base it fails"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test vim::tests::macro_replay::failing_key_stops_replay: with register a holding jx and the caret on the last row of \"abc\\njx\", @ a leaves the text unchanged (j fails, so x never runs), and a following x deletes one character as typed"
    required: true
    manual: false
  - id: "C4"
    text: "Replay feeds keys directly, synchronously, inside the @ keystroke: no GPUI keystroke is dispatched. A key that reaches the command line stops the replay with one log::warn!"
    required: true
    manual: false
  - id: "C5"
    text: "Scope: the only changed files are crates/fe-gpui/src/vim.rs, one added mod line in crates/fe-gpui/src/vim/tests.rs, and the new crates/fe-gpui/src/vim/tests/macro_replay.rs, apart from this task's own spec under docs/specs/ and its decision record. Nothing under crates/fe-vim, crates/fe-workbench, view.rs or commands.rs changes. The existing vim::tests, insert_tests, visual_tests and search_tests pass unchanged"
    required: true
    manual: false
---

# Replay vim macros in the editor: feed the keys @ leaves through the GUI vim host

# Replay vim macros in the editor: feed the keys `@` leaves through the GUI vim host

## Problem

Reported by kiln (the Linux framework agent) running the real app on mac/main at 80bba19a (2026-10-11):
"in the real app, `q` records a macro but `@a`/`@@` replay nothing."

T225 (fe-vim macros, finalized) made `@{r}` / `@@` commit `vim.play`, which doesn't feed keys itself: it
leaves them in `VimState::take_replay()` (`crates/fe-vim/src/state.rs:318`) for the host to feed. The only
caller is fe-vim's headless harness, `HeadlessVim::feed` (`crates/fe-vim/src/headless.rs:80-100`). The GUI
host, `crates/fe-gpui/src/vim.rs`, never calls it: `EditorView::feed_vim` runs the `vim.play` invocation
through `run_vim` and returns, so the replay keys sit in the state until the next `@` replaces them.
Macros have only ever been tested headless.

Recording already works for keys the reducer reads: `VimState::feed` records them while `q` is on.
Recording insert-mode text and command-line keys, the status bar's `recording @a`, replace mode and the
rest stay with T231; this card is only the replay half that makes `@` do anything.

## Change (`crates/fe-gpui/src/vim.rs`)

- After `run_vim` runs a committed invocation (in `feed_vim`, and anywhere else the host runs one from a
  key), the editor takes `VimState::take_replay()` and, if there are keys, feeds them synchronously, in
  order, before returning from the keystroke. No GPUI keystroke is dispatched (T161's lesson: nested
  dispatch can leave GPUI's propagation flag stopped).
- Each key goes where a typed key would, by the mode at that moment:
  - normal, visual and operator-pending: `VimState::feed`, then for `Commit` the invocation stamped with
    the editor's live context (as `feed_vim` stamps it) and run through `run_vim`;
  - insert: `VimState::record_key` first (as T225's contract says), then a `Char` through `type_text`
    (the keyboard's `editor.insert` path, so it joins the insert session), `Enter` and `Ctrl('j')`
    through `line_break`, `Backspace` as the editor's backspace in the session, and `Esc` as
    `vim.normal_mode` through `run_vim`; any other key fails;
  - the command line (`Mode::CmdLine`): the line lives in fe-workbench (`vim_line.rs`, T231's scope), so
    the replay stops there, with one `log::warn!` naming the limit.
- After each key, take any new replay keys (a `@` inside the macro) and feed them first, as
  `HeadlessVim::feed` does with its queue.
- A key that fails stops the replay: `Step::Forward`, `VimState::failed()`, a dispatch that returns an
  error (have `run_vim` report it, e.g. return whether it succeeded), or an insert key with no path. Drop
  the remaining replay keys and call `VimState::stop_replay()`. fe-vim's 1,000,000-key cap stays fe-vim's.
- `cx.notify()` once after the replay, so one frame shows the result.

Don't touch `crates/fe-vim`, `view.rs`, `commands.rs` or fe-workbench: `type_text`, `line_break`,
`run_vim` and `dispatched` are already reachable from `vim.rs`.

## Tests (new file)

`crates/fe-gpui/src/vim/tests/macro_replay.rs`, declared with one `mod macro_replay;` line in the
existing `crates/fe-gpui/src/vim/tests.rs`, not in `vim.rs`: Regatta's base overlay applies only
test-only files (`**/tests.rs`, `**/tests/**/*.rs`), so a module declared from `vim.rs` wouldn't compile
at the base and the regression check would go unevaluated. The tests use only keystrokes
(`simulate_keystrokes`) and API that exists at the base, in a headless window with vim on, as
`vim/insert_tests.rs`'s `vim_editor` does.
