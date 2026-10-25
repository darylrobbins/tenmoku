---
task_uid: "01m5qphyzqytmnja3rshbabhy6"
task: "T225"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::which_key_tests::popup_waits_for_the_delay (crates/fe-gpui/src/vim/which_key_tests.rs): after z no popup; with the test clock advanced past vim.which_key_delay_ms the popup lists the z keys with titles; typing t closes it"
    required: true
    manual: false
  - id: "C2"
    text: "vim::which_key_tests::fast_sequences_never_show_it: a prefix completed before the delay never shows the popup, even after the delay passes; Esc, blur and a mode change close an open one"
    required: true
    manual: false
  - id: "C3"
    text: "vim::which_key_tests::lists_user_remaps: with nnoremap \u003cleader\u003eff and \u003cleader\u003ew the popup after \u003cleader\u003e lists f (+group) and w with its command title, and repaints with f's list after f"
    required: true
    manual: false
  - id: "C4"
    text: "vim::which_key_tests::settings_switch_it: vim.which_key = false shows nothing; delay 0 shows at once; a settings reload applies live to open editors"
    required: true
    manual: false
  - id: "C5"
    text: "Integration test vim_which_key_setting (top level, crates/fe-config/tests/vim_which_key_setting.rs): defaults true and 200, out-of-range delay is a located schema error, the schema golden and template tests pass"
    required: true
    manual: false
---

# Show a which-key popup for a pending vim prefix after a delay (vim.which_key, vim.which_key_delay_ms), never when the next key comes first (Mac T1137)

## Problem

The user asked on 2026-10-24 for a which-key popup that appears only when the next key has not arrived within a delay: "We shouldn't show the which key popup if they type the next key fast enough." T1134 gives the model (`VimState::which_key`). This card shows it: a popup listing the next keys and their titles after a delay, with settings to tune or turn it off.

## Current state

- Held remap prefixes already have a timer: `EditorView::wait_for_remap` (`crates/fe-gpui/src/vim.rs:1783`) arms a `CHORD_TIMEOUT` (1 s, `kiln/crates/kiln-platform/src/keys.rs:47`) task and a blur hook, in a single `Vim.remap_wait` slot.
- Popups are child layers of `EditorView::render` (`crates/fe-gpui/src/view.rs:1773`, `completion_menu::menu_layer`).
- Vim settings reach the editors through setters (`set_vim_jump_on_s`, `crates/fe-gpui/src/vim.rs:488`), called from `crates/fastedit/src/main.rs:417` and `:152`, from `VimSettings` (`crates/fe-config/src/settings.rs:229`).
- Trap (see gpui-focus-key-ordering): keys the keymap matcher holds (`] d`, `g b`) never reach vim; this card covers prefixes vim or the remap engine holds (`z`, `Z`, `<leader>`, user maps). Chords come in a later card.

## Change

- Settings `vim.which_key` (bool, default true) and `vim.which_key_delay_ms` (default 200, which-key.nvim's; range 0 to 5000; 0 shows at once), in `VimSettings` with doc comments, `settings.schema.json` regenerated, a commented example in `settings-template.toml`, a fe-gpui setter `set_vim_which_key(cx, on, delay)` and the two calls in `main.rs`.
- New `crates/fe-gpui/src/vim/which_key.rs`: after any vim key that leaves `VimState::which_key()` `Some`, start a delay timer (one per editor, replacing the last); when it fires with the same prefix still pending, set the view's popup. Every later key, `<Esc>`, blur, a mode change or the prefix resolving removes the popup and cancels the timer, so a sequence finished before the delay never shows it. The popup is a bottom-anchored panel under the editor's text (which-key.nvim's default look): the prefix, then key and title in columns; a `group` entry shows `+N`; `Command` labels show the command's title from the host catalog (`crates/fe-gpui/src/vim.rs:757`), `Keys` labels the right side.
- The popup never takes focus or keys. It repaints with the typed prefix (`<leader>` then `f`).
- `vim.rs` and `view.rs` get only the hook lines (a field on the view, the layer, calls after a key and on blur).

## Tests

New `crates/fe-gpui/src/vim/which_key_tests.rs` (TestAppContext, no screenshots), declared next to `remap_tests` in `vim.rs`: popup absent right after `z`, present after the delay with the timer advanced, gone after `t` completes `zt`; a prefix completed before the delay never shows it; `<Esc>` and blur close it; the leader list with nested `<leader>f`; delay 0; `which_key = false` shows nothing. New `crates/fe-config/tests/vim_which_key_setting.rs`: defaults, range, schema golden.

## Implementation notes

- The popup is `EditorView::which_key` (`crates/fe-gpui/src/vim/which_key.rs`): armed by `which_key_after_key` after every keystroke `feed_vim` takes, shown by the timer only if the same prefix still waits in the same mode, and closed by the next key, a blur (whose redraw is deferred past the frame the blur listener runs in), `set_vim_which_key(false)`, or `settle_which_key` (after `run_vim`, after a held remap resolves, and each render), which drops it once `VimState::which_key` or the mode no longer matches what it shows or waits on.
- Prefixes that come with no key (a held map timing out into `z`, the keys after a map's `<Cmd>fe:…<CR>`, a command's replay) wait the same delay through `which_key_later`, deferred to the active window like `wait_for_remap_later` (which calls it at the end of every replay). It skips the prefix the popup last waited on since the last key, so a mode change that keeps `Z` pending (`vim.visual` dispatched) closes its popup for good.
- Once shown, a key that leaves another prefix waiting repaints it at once (`<leader>` then `f`), as which-key.nvim does.
- The panel takes at most half the editor's height; its keys scroll inside it (`the_panel_fits_a_short_editor`).
- Titles: built-ins from fe-vim's table; `Command` labels look the ID up by metadata in vim's, the proposals', the diff view's and the editor's registries, the host's registry commands, the typed commands and the app's, falling back to the ID; `Group(n)` shows `+n`; an entry that is a map and a group shows its title with ` +`. The host's `CommandCatalog` only checks IDs, so `fe_gpui::set_host_commands` takes the workbench's registry commands with their titles: one line in `crates/fe-workbench/src/lib.rs` (outside the card's listed scope) hands it `command_list()`.
- Like a held remap's wait, it arms only while the editor takes keys (focused in the active window).
- Limit: a remap prefix still resolves at `CHORD_TIMEOUT` (1 s) whether or not the popup shows, so a leader popup stays up for about 800 ms with the default delay, and never shows with a delay over 1 s. which-key.nvim keeps waiting while its popup is open; holding the remap wait open while the popup shows would be a follow-up.
- Checked in the real window under Xvfb: `z` shows the two-column list along the editor's bottom, `\` with `\ff` and `\w` mapped lists `f → +1` and `w → Undo`, and `f` repaints with `f → gg`.
