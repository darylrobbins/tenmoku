---
task_uid: "01m4a5apyjer5g73nvafb03yrt"
task: "T161"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::tests::normal_mode_keys_go_to_fe_vim: after set_vim_enabled(true), on a focused editor over a 10-row buffer, 5 j moves the primary caret 5 rows and an observe_invocations observer sees one vim.move with args {\"motion\":\"down\",\"extend\":false,\"count\":5}, source Keyboard, and a context whose editor frame has vim.mode normal; down moves one row; left at column 0 of row 1 leaves the caret there (editor.move_left would cross the line break); q, Q, K, enter, tab, backspace, shift-backspace, delete, alt-backspace and simulate_input(\"abc\") leave the text unchanged; cmd-a still selects all"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim::tests::pending_input_owns_the_keys: 2 then j with no frame drawn in between moves two rows; after 3 with no frame drawn, alt-z leaves soft wrap off and drops the count, so a following j moves one row; after d, the context the interceptor matches in has vim.mode operator_pending, the flag vim.pending and vim.pending_kind operator, and escape then clears all three; cmd-a while 3 is pending selects nothing and drops the count; escape with nothing pending changes neither text nor carets"
    required: true
    manual: false
  - id: "C3"
    text: "Setting and toggle: fe-config test vim_setting (tests/vim_setting.rs): Settings::default().vim.enabled is false, a settings.toml of [vim] enabled = true loads with no diagnostics and enabled true, and the template still loads to Settings::default(); settings.schema.json is regenerated (its golden test passes). fe-gpui test vim::tests::setting_and_toggle: with vim off, j types j and no frame of the context stack has vim.mode; after set_vim_enabled(true), j moves; dispatching vim.toggle turns vim off (vim_enabled false) and j types again; turning vim off with 3 pending and on again leaves nothing pending (j moves one row); vim.move dispatched with vim off changes nothing; the View menu has a \"Toggle Vim Mode\" item running vim.toggle"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test vim::tests::vim_layer_of_the_default_keymap: DEFAULT_MACOS loads with no diagnostics against fe_gpui::commands::command_catalog(); exactly 10 bindings have context editor \u0026\u0026 vim.mode == normal: left, right, up and down run vim.move with motion left, right, up and down, and enter, tab, backspace, shift-backspace, delete and alt-backspace run vim.key with no args; in the stack [workbench, editor with vim.mode normal] no default binding whose keys hold ctrl without cmd matches; the binding counts pinned in command_tests.rs and keymap_tests.rs are their base values plus 10, and those two files change only in those counts and their context lists"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test status_bar::vim_tests::shows_the_vim_mode: for an editor with vim off the bar has no vim_mode item; after set_vim_enabled(true) and a redraw it has a left item vim_mode with text NORMAL, still NORMAL after d (operator_pending); after set_vim_enabled(false) the item is gone; the right-hand built-in items are the same in all three"
    required: true
    manual: false
  - id: "C6"
    text: "fe-gpui test vim::tests::block_cursor_in_normal_mode: on \"a漢b\\n\\nxy\" with vim on, a caret at byte 1 paints as one rectangle spanning from the caret x at byte 1 to the caret x at byte 4; on the empty row 1 it is one space wide; with vim off it is 2px wide, as before; an unfocused editor paints no caret"
    required: true
    manual: false
---

# Turn on vim mode in the editor: a vim.enabled setting and toggle, normal-mode keys through fe-vim, no text input in normal mode, the mode in the status bar, a block cursor

# Turn on vim mode in the editor: the vim.enabled setting and a toggle, normal-mode keys routed to fe-vim, no text input in normal mode, the mode in the status bar, and a block cursor

Milestone M7 "Vim", GPUI wiring, card A of 2. `fe-vim` (T154) is headless: a pure reducer, `VimState`,
`vim.mode` on an editor frame, and `vim.move` in its own registry. This card makes it work in the app's
editor, in normal mode. Card B (after T156) wires insert mode.

Read first: `docs/decisions/vim-engine.md` §9 items 1, 2 and 7 and **§10 (all of it: the user's
decisions)**; `docs/decisions/command-model.md` (contexts, and the note that GPUI's identifiers can't
hold `.`, so the GPUI key is `vim_mode`); T154's spec (`docs/specs/` for its lane) and
`crates/fe-vim/src/lib.rs`; `crates/fe-gpui/src/keymap.rs`'s module docs.

## Clean room

As every M7 card (`CLEAN_ROOM.md`): don't open Vim's or Neovim's source (C, Lua or Vimscript runtime),
hjkl's crates or corpus (any `hjkl-*`), Zed's `vim` crate (GPL) or Helix (MPL). Zed's `editor` crate is
GPL too. Vim behaviour comes from running `nvim --clean` (0.12.4) and from `:help`; GPUI's own
Apache-2.0 crates are fine to read.

## Change

### The setting (`crates/fe-config`)

- `Settings` gains `vim: VimSettings` (`#[serde(default)]`), with `enabled: bool`, default `false`. In
  `settings.toml` that is `[vim]` / `enabled = true`. The type lives in `settings.rs`, reachable as
  `fe_config::settings::VimSettings`, so `fe-config/src/lib.rs` doesn't change.
- Regenerate `settings.schema.json` (its golden test, `UPDATE_GOLDEN=1`).
- `settings-template.toml` gains a commented `[vim]` section with `# enabled = false` and a one-line
  description, after `[editor]`; the template must still load to `Settings::default()`.

### On and off (`crates/fe-gpui/src/vim.rs`, new)

- One app-wide switch, a GPUI global: `pub fn set_vim_enabled(cx: &mut App, enabled: bool)` and
  `pub fn vim_enabled(cx: &App) -> bool`, re-exported from `fe-gpui`'s `lib.rs`. Setting it refreshes
  every window.
- Each `EditorView` holds `vim: Option<VimState>`. It creates the state when vim is on and it has none,
  and drops it when vim is off (a pending command goes with it). Turning vim on doesn't move any caret:
  the first vim motion puts carets in normal-mode positions, as `fe-vim` already does.
- `crates/fastedit/src/main.rs` applies `settings.vim.enabled` at startup, and on a settings reload
  only when `vim.enabled` changed between the previous and new settings, so an unrelated edit to
  `settings.toml` doesn't undo the toggle below.
- **Toggle Vim Mode.** `vim.toggle`, title "Toggle Vim Mode", in fe-gpui's app registry (no context;
  `command_catalog()` knows it). It flips the switch for this session only; it doesn't write
  `settings.toml`. There is no command palette yet, so the View menu gets a "Toggle Vim Mode" item after
  "Soft Wrap" (`crates/fe-keymap/menus/default-macos.toml`). No default key.

### The mode in the key context (`view.rs`, `vim.rs`, `keymap.rs`)

- With vim on, the editor's GPUI key context is `editor vim_mode=<VimState::mode_name()>`, plus the
  flag `vim_pending` and `vim_pending_kind=<kind>` while a command is pending. `keymap::context_stack`
  maps GPUI keys starting `vim_` to `vim.` (`vim_mode` → `vim.mode`), so predicates read as fe-vim
  writes them. With vim off none of these are set (§10).
- **Live state, not the last frame.** A drawn key context lags: `3` then `alt-z` typed within one frame
  would be matched with no `vim.pending`. So the interceptor re-stamps the focused editor's frame from
  that editor's live `VimState` (`VimState::stamp`) before matching. Suggested mechanism: a global map
  from each vim editor's `FocusId` to its `WeakEntity<EditorView>`, kept by the view.
- fe-gpui must not `match` exhaustively on `fe_vim::Mode`: T156 adds `Insert`. Use `mode_name()`.

### Key routing (§10 steps 1–3, §9.1)

A keystroke on a focused vim editor in `normal` or `operator_pending` mode:

1. **Nothing pending:** the keymap matches first, as today. `cmd-` shortcuts work. A match runs.
2. **Unmatched, or nothing pending and matched to `vim.key` (below):** it goes to the editor, which
   converts it to a `VimKey` and calls `VimState::feed`. Suggested place: an `on_key_down` listener on
   the editor's element, which the interceptor's propagation already reaches.
   - `Wait` and `Cancel` swallow it (stop propagation) and notify, so the context redraws.
   - `Commit(invocation)` dispatches it through `fe_vim::registry()` (built once, thread-local, like
     `EDITOR_COMMANDS`), with its context set to the stack the key came in, the editor frame stamped
     from the state after the reducer cleared it. Effects and errors are handled as `run_command`
     handles them, and `observe_invocations` observers see it. Swallowed.
   - `Forward`: nothing runs. It propagates, and, with no input handler, types nothing.
3. **Something pending** (`vim.pending` on the live frame): the keymap isn't consulted; every key goes
   to step 2 (§9.1: "While `vim.pending` is true, the handler owns every key"). A keystroke with no
   `VimKey` (anything with `cmd`, function keys) cancels the pending command and is swallowed.

**Keystroke to `VimKey`:** no `cmd`; `ctrl-[` is `Esc`; `ctrl-<letter>` is `Ctrl(letter)`; `escape`,
`enter`, `backspace`, `tab` and the four arrows are their named keys; otherwise the keystroke's typed
character (`key_char`, case kept, so `shift-4` is `$`) is `Char`. Anything else has none.

**Running `vim.*` from a binding or a menu.** `EditorView::run_command` sends IDs the vim registry knows
to it, when vim is on, with the editor frame stamped from the live state; with vim off they fail as
unknown commands do today. `vim.key` is the exception: it is never dispatched (step 2 above).

### The vim layer of the default keymap (`crates/fe-keymap/keymaps/default-macos.toml`)

Appended after every other binding, so in the same frame it wins over plain `editor` bindings (later
entry wins, `matcher.rs`). Context `editor && vim.mode == normal` for each:

- `left`, `right`, `up`, `down` → `vim.move` with `{"motion": "left"}`, `right`, `up`, `down` (Vim's
  `<Left>` is `h`, `<Right>` `l`, `<Up>` `k`, `<Down>` `j`; `:help h` etc.).
- `enter`, `tab`, `backspace`, `shift-backspace`, `delete`, `alt-backspace` → `vim.key`.

`vim.key` (no args) means "this keystroke belongs to vim here": the interceptor doesn't dispatch it but
hands the keystroke to step 2. It is registered (in fe-gpui's app registry, so the catalog knows it)
with a handler that does nothing, for a direct dispatch. It takes no args on purpose: it can't map one
key to another (remaps aren't designed yet, §10).

Ctrl keys in normal mode (§10: "vim wins"): the default keymap has no `ctrl-` binding in an `editor`
context today, so every `ctrl-` key reaches the reducer. A test pins that, so a later default `ctrl-`
binding has to be shadowed in the vim layer. Home, End, page keys, option-arrows, shift-selection and
`alt-z` keep their macOS bindings in normal mode for now.

The fe-gpui tests that pin the default keymap (`command_tests.rs`, `keymap_tests.rs`: the binding count,
the old table and the context lists) are updated for exactly these 10 bindings, and nothing else in them
changes.

### No text in normal mode (`element.rs`, `view.rs`)

`EditorElement::paint` calls `window.handle_input` only when the view accepts text: vim off, or
`mode_name()` is `insert` (none until T156). In normal and operator-pending mode no input handler is
registered, so neither typing nor an IME inserts text (§10).

### Showing the mode

- **Status bar** (`crates/fe-workbench/src/status_bar.rs`): a built-in item `StatusItem::VIM_MODE`
  (`"vim_mode"`), on the left at priority 0, shown only when the editor has vim on. Text: `NORMAL` for
  `normal` and `operator_pending`, `INSERT` for `insert`, otherwise `mode_name()` in upper case. fe-gpui
  exposes it as `EditorView::vim_mode(&self) -> Option<&'static str>` (the `mode_name()`), so
  fe-workbench doesn't depend on fe-vim. A mode change notifies the view, which redraws the bar.
- **Block cursor** (§9.7): in `normal` and `operator_pending`, each caret paints as a block over the
  grapheme after it (to the next grapheme boundary on that display row), in the cursor colour at partial
  alpha so the glyph stays readable; at a line end or on an empty line, one space wide. Unfocused or vim
  off: the 2px bar as today. Add a test-only accessor on the view for the caret rectangles the last frame
  painted.

### `fe-gpui/Cargo.toml`

Add `fe-vim.workspace = true` (root `Cargo.toml` already has it in `[workspace.dependencies]`).

### Order with T155

T155 (running) adds `registers: &'a mut Registers` to `VimTarget`. Build `VimTarget` in exactly one
function in `vim.rs`. If T155 has landed when you start or rebase, keep one `fe_vim::Registers` per app
as a GPUI global and pass it there. If this card lands first, T155's rebase needs that one line.

## Tests (new files)

- `crates/fe-gpui/src/vim/tests.rs` (declared from `vim.rs`, as `element.rs` declares `element/tests.rs`),
  `#[gpui::test]`s in a headless window, keys through `simulate_keystrokes`/`simulate_input` as
  `keymap_tests.rs` does.
- `crates/fe-workbench/src/status_bar_vim_tests.rs`, declared from `status_bar.rs` with `#[path]`, as
  `project_panel.rs` declares `project_panel_hover_tests.rs` (so `src/tests/mod.rs` doesn't change).
- `crates/fe-config/tests/vim_setting.rs`, an integration test.

## Out of scope

- Insert mode: `Esc`/`ctrl-[` bindings, typed text joining an insert session, splitting the session,
  the bar cursor there (card B, after T156). Operators, registers, text objects (T155) beyond passing
  `Registers` through as above.
- Visual modes, macros, marks, search, ex, showcmd (the pending keys in the bar), the underline cursor
  for operator-pending (§9.7), and vim behaviour for mouse clicks.
- Key-to-key remaps (`nnoremap`): not designed yet (§10).
- Writing `settings.toml` from the toggle; a command palette.
- Any change in `crates/fe-vim`, `crates/fe-keymap/src`, `crates/fe-editor` or `crates/fe-command`.

## Decided (lead defaults; the user may revisit)

- **One app-wide switch,** not per editor or per window: `vim.enabled` is a user setting.
- **The toggle doesn't persist:** it flips the switch for the session. Persisting needs a settings
  writer that keeps the user's comments, a later card.
- **`vim.key` instead of unbinding:** fe-keymap's `command = ""` only removes a binding with the same
  context, so it can't turn `enter` off in `editor && vim.mode == normal` alone. `vim.key` takes no
  args, so it isn't a remap.
- **Keys with `cmd` cancel a pending command** and do nothing else; §9.1's "explicit overrides" while
  pending are a later card.
