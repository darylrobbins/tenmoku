---
task_uid: "01m4sj27ngda3wr4h7z99rnvhb"
task: "T605"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-keymap tests/vim_window_keys.rs: on macOS and on Linux every sequence in the card's table resolves to its command in editor \u0026\u0026 vim.mode == normal, and none resolves in vim insert mode or with vim off; one_keymap passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench vim_window_keys_tests.rs: with vim on in an editor, ctrl-w v splits right, ctrl-w h focuses the left pane, ctrl-w L moves the active tab right, ctrl-w \u003e widens the active pane, ctrl-w = evens the panes and ctrl-w c closes the pane; vim's mode and pending keys are unchanged by each (vim's message is cleared by any keystroke, bound or not, by design since T219: fe-gpui vim::live_stack; that is not changed here)"
    required: true
    manual: false
  - id: "C3"
    text: "With the Linux keymap, ctrl-w h in vim normal mode doesn't run workbench.close_tab, and with vim off ctrl-w alone still resolves to workbench.close_tab"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-keymap -p fe-workbench -p fe-gpui and cargo clippy -p fe-keymap -p fe-workbench -p fe-gpui --all-targets -- -D warnings pass; the fe-gpui test edits (command_tests.rs, keymap_tests.rs, vim/tests.rs) only update binding counts and exempt the window keys, through one shared predicate (keymap_tests::is_window_key: a pane.* command on a ctrl-w chord in context editor \u0026\u0026 vim.mode == normal) with an assertion that exactly the card's window-key bindings match it"
    required: true
    manual: false
---

# Bind vim's <C-w> window keys in normal mode to the pane commands: focus, next, split, close, move tab, resize and even out

## Problem

The user (2026-10-13): "We should also allow moving and resizing panes." They work in vim mode.

fastedit has no vim window keys: `<C-w>` reaches fe-vim unmatched (fe-vim has no window commands; its
`HostCommand` covers only `:w :q :e :wa :qa`, user and diff commands), so a vim user can't split, move between,
move a tab to, resize or even out panes from the keyboard without leaving vim's keys for the
`pane.*` bindings (ctrl-cmd-arrows and the like). The pane commands for all of it exist once the pane-resize
card lands (`pane.grow_width` … `pane.equalize`, `pane.focus_next|previous`), beside T79's
`pane.split_*`, `pane.focus_*`, `pane.move_tab_*` and `pane.close`.

## Change

Bind vim's `<C-w>` keys to the pane commands in the keymaps, in context `editor && vim.mode == normal`
(after the existing vim layer in `crates/fe-keymap/keymaps/fastedit.toml`, and in `default-macos.toml` and
`default-linux.toml`, which fe-keymap's `one_keymap` test checks it against):

| keys | command |
|---|---|
| `ctrl-w h`, `ctrl-w ctrl-h`, `ctrl-w left` | `pane.focus_left` (and `j`/`k`/`l`, down/up/right, likewise) |
| `ctrl-w w`, `ctrl-w ctrl-w` / `ctrl-w W` | `pane.focus_next` / `pane.focus_previous` |
| `ctrl-w s`, `ctrl-w S`, `ctrl-w ctrl-s` / `ctrl-w v`, `ctrl-w ctrl-v` | `pane.split_down` / `pane.split_right` |
| `ctrl-w c`, `ctrl-w q`, `ctrl-w ctrl-q` | `pane.close` |
| `ctrl-w H` / `J` / `K` / `L` | `pane.move_tab_left` / `down` / `up` / `right` |
| `ctrl-w >` / `<` | `pane.grow_width` / `pane.shrink_width` |
| `ctrl-w +` / `-` | `pane.grow_height` / `pane.shrink_height` |
| `ctrl-w =` | `pane.equalize` |

Shifted letters and punctuation are named as the file's header says (by what they type). Off macOS, where
`ctrl-w` alone is `workbench.close_tab` in the `workbench` context, these bindings keep `ctrl-w <key>` from
closing the tab in vim normal mode; with vim off, `ctrl-w` still closes it. Counts (`3<C-w>>`) are out of
scope.

New tests: `crates/fe-keymap/tests/vim_window_keys.rs` (each binding resolves on both platforms), and
`crates/fe-workbench/src/vim_window_keys_tests.rs`, declared from `pane_group.rs` with `#[path]`, driving
keystrokes into a vim-mode editor in a workbench with the default keymap loaded, as
`src/tests/default_keymaps.rs` does.

Keymap files: after T462 (lent to helium), which edits the same three keymap files; T523 and T557 (inbox) edit
them too.
