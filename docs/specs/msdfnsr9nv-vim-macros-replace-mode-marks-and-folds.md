---
task_uid: "01m4rqeghr06zx5mmsdfnsr9nv"
task: "T47"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::macro_tests::record_and_replay: with vim on, q a A simulate_input(\";\") escape j q leaves register a holding \"A;\\x1bj\"; 3 @ a then appends ; to the next three rows in three undo steps; a macro with i, typed text, enter and escape replays the same text and line breaks; a macro holding /foo enter replays the search without showing the command line; marked IME text that is then committed records only the committed text; replay dispatches no GPUI keystroke (a keystroke observer sees none during @a); a recursive macro stops at the buffer's end"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::replace_tests::replace_mode_typing: with vim on, R then simulate_input(\"xy\") overwrites two graphemes, backspace restores the last one, escape returns to normal mode and u undoes the whole replace in one step; in replace and operator-pending mode the caret paints as an underline (2px tall, one grapheme wide), in normal mode as a block and in insert mode as a bar; ctrl-a on \"x 9\" gives \"x 10\"; za on an indented block hides its rows on the next frame and za again shows them; 'a after ma and G moves the caret back and scrolls it into view (that test runs on the macOS keymap)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::vim_recording::shows_replace_and_recording: with vim on, after R the vim_mode item reads REPLACE; after q a it shows recording @a beside the mode, and after q again it is gone"
    required: true
    manual: false
  - id: "C4"
    text: "The default keymap (crates/fe-keymap/keymaps/fastedit.toml, regenerated into default-macos.toml and default-linux.toml) binds escape and ctrl-[ to vim.normal_mode in replace mode as in insert mode, and off macOS binds ctrl-a and ctrl-x to vim.key with platform = \"!macos\" in contexts editor \u0026\u0026 vim.mode == normal and editor \u0026\u0026 vim.visual, shaped as the ctrl-v/ctrl-q bindings (four bindings, none in default-macos.toml); nothing else changes in the vim layer; the counts pinned in command_tests.rs and keymap_tests.rs (143 and 61 on main, counted on default-macos.toml) change only by the replace-mode bindings; fe-keymap test one_keymap passes; fe-workbench test tests::default_keymaps::vim_keeps_its_ctrl_keys_on_linux checks ctrl-a and ctrl-x resolve to vim.key in normal and visual mode and not in insert mode (editor.select_all and editor.cut there); fe-workbench test tests::outline_panel::the_default_keymap_binds_outline_show passes unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "Scope: nothing under crates/fe-vim changes; crates/fe-workbench/src/lib.rs doesn't change; T161's vim::tests, T162's vim::insert_tests and T175's, 06a-06c's, 07b's and 10b's tests pass unchanged"
    required: true
    manual: false
  - id: "C6"
    text: "Checked in the running app with vim on: record a macro with qa that types text in insert mode and moves down, play it with 5@a, and see recording @a in the status bar while recording; R overwrites with an underline cursor and backspace restores; za folds and unfolds the block under the cursor"
    required: true
    manual: false
---

# Vim macros, replace mode, marks and folds in the editor: recording every key including typed text, replaying through the same path, overwrite typing with an underline cursor, and a recording indicator (Mac T231)

Filed on helium from Mac T231 (~/coordination/helium-cards/T231.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1, the proposal/AI chain, run strictly one after another for crates/fe-gpui/src/view.rs): counts the card pins (keymap rows in default-macos.toml, keymap_tests.rs and similar) were taken on Mac main; on helium use the counts on your base and change them by exactly the deltas the card states. Screenshot goldens are Metal-only and do not run on helium: a golden criterion is met here by the Linux suite passing, and the Mac regenerates or checks goldens after the sync. Argon's Mac T427 (multi-cursor keys, alt-click) also changes view.rs and vim.rs but is not on helium: do not port it or code around it.

# Vim macros, replace mode, marks and folds in the editor: recording every key including typed text, replaying through the same path, overwrite typing with an underline cursor, and a recording indicator

Milestone M7 "Vim", second batch (resized), card 11: the GPUI wiring of 08a–08b (marks, macros) and
09a–09c (replace mode, `ctrl-a`/`ctrl-x`, fold keys); unchanged in substance from the old card 11
(about 430 non-test lines, size m). Read their specs, T161's and T162's, 06a's (the command line) and
`docs/decisions/vim-engine.md` §9 items 6 and 7 and §10.

**Clean room** as every M7 card: no Vim/Neovim source, no hjkl, no Zed, no Helix.

## Change

### Macro recording (`crates/fe-gpui/src/vim.rs`, `crates/fe-gpui/src/view.rs`)

- While `vim.recording` is set on the focused editor, every key that reaches it is recorded with
  `VimState::record_key`: keys fed to the reducer (normal, visual, operator-pending), and in insert
  and replace mode the text the input handler commits (each char), `enter`, `backspace` and
  `escape`/`ctrl-[`. Keys the keymap runs as other commands in insert mode (`alt-backspace`, arrows,
  `secondary-z`) are recorded in vim notation where one exists (`<Left>`), and otherwise end the
  recording's fidelity: they are skipped and logged once per recording (documented limit).
- IME: only committed text is recorded, never marked (composing) text.
- Keys typed into 06a's command line are recorded too: the line reports its submitted text and
  `enter`/`escape` to the editor's state.

### Replay

- After `vim.play`, the editor takes `VimState::take_replay()` and feeds the keys in order,
  synchronously, through the same paths as typing: the reducer in normal/visual modes, the model's
  text input path (`EditorModel::insert` through the session, `line_break`, `backspace`,
  `replace_text`) in insert and replace modes, and the command line's submit for command-line text
  (without showing the line). No GPUI keystroke is dispatched (T161's lesson: nested dispatch can
  leave GPUI's propagation flag stopped).
- Replay stops at the first key fe-vim reports as failed, as 08b defines.
- **T352 lands the basic replay loop first** (normal, visual and insert keys fed through
  `vim.rs`, stopping at a failed key; command-line keys stop the replay with a warning). This task
  builds on it: command-line replay (submit without showing the line), recorded typed text with
  line breaks and IME commits, counted and recursive replay, rather than writing the loop again.

### Replace mode

- In `replace` mode the input handler is registered and typed text goes to
  `VimState::replace_text` instead of `EditorModel::insert`; `backspace` to `replace_backspace`;
  `enter` to `line_break`; `escape` and `ctrl-[` to `vim.normal_mode` (T162's bindings in `crates/fe-keymap/keymaps/fastedit.toml`
  gain the `replace` mode: context `editor && (vim.mode == insert || vim.mode == replace)`, or two
  more bindings; regenerate `default-macos.toml` and `default-linux.toml`; the pinned counts change
  accordingly).
- §9.7's **underline cursor** in `replace` and `operator_pending` modes: a bar under the grapheme,
  2px tall.

### Status bar (`crates/fe-workbench/src/status_bar.rs`)

- `REPLACE` for `replace` mode, and `recording @a` beside the mode while recording.

### Marks, increments and folds

- Nothing to add if 08a and 09b–09c work through the reducer: check with tests that `ma`/`'a` jumps scroll
  the target into view, `ctrl-a` and `ctrl-x` reach the reducer in normal and visual mode, and
  `za`/`zc`/`zo` change what the editor paints (the gutter's fold chevrons included) on the next frame.
- **`ctrl-a`/`ctrl-x` off macOS.** On macOS no `ctrl-` binding applies in the editor, so both reach
  the reducer unmatched. On Linux `ctrl-a` is `editor.select_all` and `ctrl-x` is `editor.cut`
  (context `editor`), which would catch them first. In `fastedit.toml`'s vim layer add `vim.key`
  bindings for `ctrl-a` and `ctrl-x` with `platform = "!macos"`, one in `editor && vim.mode == normal`
  and one in `editor && vim.visual`, shaped as the existing `ctrl-v`/`ctrl-q` ones and beside them:
  four bindings, all in `default-linux.toml`, none in `default-macos.toml`. In insert mode and with
  vim off they stay select-all and cut. The counts pinned in `crates/fe-gpui/src/command_tests.rs`
  and `crates/fe-gpui/src/keymap_tests.rs` (143 and 61 on main) count `default-macos.toml`, so these
  four add nothing to them: the pins move only by the replace-mode bindings above.

## Tests (new files)

- `crates/fe-gpui/src/vim/macro_tests.rs` and `crates/fe-gpui/src/vim/replace_tests.rs`, declared
  from `vim.rs`.
- `crates/fe-workbench/src/tests/vim_recording.rs`, declared in
  `crates/fe-workbench/src/tests/mod.rs` (the lead's rule: not a `#[path]` test file under `src/`).
- In the existing `crates/fe-workbench/src/tests/default_keymaps.rs`,
  `vim_keeps_its_ctrl_keys_on_linux`'s key list gains `ctrl-a` and `ctrl-x`.

## Size

About 430 non-test lines (recording ~150, replay ~100, replace input and underline ~120, status bar
~30, checks ~30).

## Out of scope

- Recording keys that run non-vim editor commands in insert mode faithfully (only their vim
  notation equivalents); `Q`; `:normal`; recording invocations for AI replay (§9.6's other half).
- Any fe-vim change (block if 08a–09c's API lacks something).

## Decided (lead defaults; the user may revisit)

- **Replay feeds keys into fe-vim and the model directly**, not as synthetic GPUI keystrokes, so
  user keymap bindings typed during recording are replayed as the keys vim saw, not re-resolved.
  The other choice (replaying through the keymap) honours later keymap changes but needs nested
  key dispatch.

## Implementation notes

- **Recording.** The reducer records what it reads; the editor records what it takes itself
  through `VimState::record_key`, in `run_command`: `editor.insert`'s text (typed keys and
  input-method commits alike, so marked text never reaches it), `editor.newline` as `<CR>`,
  `editor.backspace` as `<BS>`, the arrows' `editor.move_*` as `<Left>` …, and `vim.normal_mode`
  from insert or replace mode as `<Esc>`. In normal and visual mode the arrows (`vim.move` from the
  keymap, which the reducer doesn't read) are recorded as `<Left>` … too, and replayed as those
  `vim.move`s. Any other editor command in insert or replace mode is left out, with one warning
  per recording.
- **Command line.** The status bar's line calls `EditorView::record_command_line(text, key)` just
  before it submits (`<CR>`) or cancels (`<Esc>`, or `<BS>` on an empty line). The text is
  recorded after the line's prefill (`'<,'>`, `.,.+1`), any prefill it took out as `<BS>`s first,
  since a replay starts the line with the prefill too. A replay edits a line held in the replay
  loop, never shown, and `<CR>`, `<C-j>` or `<Esc>` submits it (nvim's `c_<Esc>` in a macro);
  `<BS>` on an empty line and `<C-c>` cancel it. A host command a replayed `:w` leaves is dropped
  with a warning. A replay that ends with the line open leaves it open, its replayed text dropped.
- **Replay** sets `vim_replaying`, so the typing paths it goes through (`type_text`,
  `run_command`) don't record or count its keys a second time, and a `@` run inside it doesn't
  start a nested loop.
- **Replace mode.** `accepts_text` includes replace mode (input handler registered, keys not
  vim's). `run_command` sends `editor.insert` to `VimState::replace_text` and `editor.backspace`
  to `replace_backspace` there, and rewrites `vim.normal_mode` to `vim.replace_normal_mode`, which
  fe-vim requires (`vim.normal_mode` is not in context in replace mode). The completion menu isn't
  asked while typing in replace mode.
- **Keymap.** Two new bindings (`escape`, `ctrl-[` in `editor && vim.mode == replace`), so the
  pins in `command_tests.rs` and `keymap_tests.rs` go from 143 to 145; the four `ctrl-a`/`ctrl-x`
  bindings are `!macos` only.
- **Scrolling fix.** Since T416 every vim motion asks for the viewport (for `H M L`), so
  `dispatched` pinned the top row after every motion and `G` or `'a` never scrolled the caret into
  view. It now pins only while the primary caret's row is still in the view the command saw.
