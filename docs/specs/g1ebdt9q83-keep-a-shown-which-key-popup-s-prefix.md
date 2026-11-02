---
task_uid: "01m6c8b8gcz1ft0wg1ebdt9q83"
task: "T1281"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Once a matcher chord's which-key popup shows (g, ], ctrl-w), the chord stays held and its popup, with the keymap's keys, stays up past CHORD_TIMEOUT until the next key: a_shown_chord_popup_outlasts_the_chord_timeout in crates/fe-gpui/tests/which_key_hold.rs"
    required: true
    manual: false
  - id: "C2"
    text: "Once a vim map prefix's popup shows (\u003cleader\u003e of \u003cleader\u003eff), the held keys don't resolve at CHORD_TIMEOUT and the keys typed after it complete the map: a_shown_map_popup_outlasts_the_map_timeout"
    required: true
    manual: false
  - id: "C3"
    text: "With no popup shown (vim.which_key off, or a delay longer than CHORD_TIMEOUT), a chord and a held map still end at CHORD_TIMEOUT as before: vim::which_key_chord_tests::chord_end_closes_the_popup, vim::which_key_tests::lists_user_remaps and vim::which_key_tests::prefixes_left_by_a_replay_show_it, updated, pass"
    required: true
    manual: false
  - id: "C4"
    text: "\u003cEsc\u003e, a blur and a keymap reload still close a held popup and end its prefix (chord_end_closes_the_popup)"
    required: true
    manual: false
---

# Keep a shown which-key popup's prefix waiting past CHORD_TIMEOUT, so g, ], ctrl-w and leader popups no longer hide (and g's reappear) after 1 s

## Problem

The user (2026-11-01): "The vim what key popup shows and then hides within a second for the g one it
comes back again. The hiding logic isn't quite working properly for fast pressing."

Every prefix that is held on a timer loses its which-key popup at `CHORD_TIMEOUT` (1 s), even while the
popup is up and the user is reading it. which-key.nvim, which this popup copies, waits for the next key
however long that takes once it shows.

- **Keymap matcher chords** (`g` of `g b`/`g d`/`g r r`, `]`/`[` of `] d`, `ctrl-w`): `g` in normal mode
  is held by kiln's matcher (`crates/fe-keymap/keymaps/fastedit.toml:2453-2511`). `HeldChord::start`
  (`kiln/crates/kiln-platform/src/keys.rs:243-246`) arms a `CHORD_TIMEOUT` timer
  (`keys.rs:50`). The popup shows at 200 ms (`which_key_due`,
  `crates/fe-gpui/src/vim/which_key.rs:248-253`). At 1 s `on_timeout` -> `end_chord`
  (`keys.rs:487-519`) flushes the matcher and replays `g` to vim (`VimKeys::replay`,
  `crates/fe-gpui/src/keymap.rs:44-50`, then `feed_vim` -> `which_key_after_key`). That closes the chord
  popup (`which_key.rs:195-200`). Vim has its own `g` prefix, so it re-arms with the 200 ms delay and the
  popup comes back at ~1.2 s, now without the keymap's `b`, `d`, `r`. The same happens for `]` and `[`.
  `ctrl-w` doesn't come back because vim takes no key after it. Today's test
  `vim::which_key_chord_tests::chord_end_closes_the_popup` ("Timed out", which_key_chord_tests.rs:294-311)
  asserts this flicker as intended.
- **Vim map prefixes** (`<leader>` of `<leader>ff`): `EditorView::wait_for_remap`
  (`crates/fe-gpui/src/vim.rs:2097-2128`) arms a `CHORD_TIMEOUT` timer, and `remap_timed_out` resolves the
  held keys. `resolve_remap_from` -> `settle_which_key` (`vim.rs:2185-2202`) closes the popup at 1 s
  (asserted by `vim::which_key_tests::lists_user_remaps`, which_key_tests.rs:340-342).

Repro: in normal mode press `g` and wait. The popup shows at 0.2 s, disappears at 1.0 s and returns at
1.2 s with fewer keys. With `<leader>` maps, press `<leader>`: it shows at 0.2 s and is gone for good at
1.0 s. Built-in vim prefixes (`z`, `Z`, `d`) have no timer and stay up, which is why only some prefixes
misbehave.

## Change

Once the popup shows for a prefix, that prefix stops timing out. It waits for the next key, `<Esc>`, a
blur or a keymap reload, as which-key.nvim does. Before the popup shows (a key typed within the delay,
`vim.which_key = false`, or a delay longer than `CHORD_TIMEOUT`), timeouts behave as today.

1. `kiln/crates/kiln-platform/src/keys.rs`: add `pub fn hold_chord(window: &Window, cx: &mut App) -> bool`.
   If a chord is pending for `window`'s focused element (the same `Target` check as
   `pending_continuations`), it cancels that chord's timeout by replacing `HeldChord::_timeout` with
   `Task::ready(())` and keeps `_focus_out`. It returns whether it held one. The next keystroke starts a
   new `HeldChord` with a fresh timeout, as today. Document it in the module doc next to
   `pending_continuations`.
2. `crates/fe-gpui/src/vim/which_key.rs`: wherever the popup becomes shown (the `at_once || delay.is_zero()`
   branch of `arm_which_key` and both show branches of `which_key_due`), call a new
   `EditorView::hold_prefix(window, cx)`:
   - for a chord target, `kiln_platform::keys::hold_chord`;
   - for a vim target, cancel this editor's `Vim::remap_wait` timer (replace `RemapWait::_timer` with
     `Task::ready(())`, keep `_blur`). Reach `super::Vim` directly from this child module. `vim.rs` is
     outside this card's scope (T659 and T1168 hold it).

   `which_key_due` needs the window: spawn the timer with `cx.spawn_in(window, …)` and `update_in`.
   Because a key that keeps a prefix waiting re-arms the timer (`wait_for_remap`, a new `HeldChord`) before
   the popup repaints at once, the hold applies again on every repaint. Update the module doc
   (`which_key.rs:1-27`): a shown popup's prefix doesn't time out.
3. Update the tests that assert the old behaviour. In `which_key_chord_tests.rs`, rewrite the "Timed out"
   part of `chord_end_closes_the_popup` so the chord still times out with no popup shown (a delay above
   `CHORD_TIMEOUT`, or `vim.which_key` off). In `which_key_tests.rs`, change `lists_user_remaps` (the
   popup stays past `CHORD_TIMEOUT`, and `<Esc>` closes it) and `prefixes_left_by_a_replay_show_it` (make
   the `xx` wait time out before any popup shows, for example with a delay above `CHORD_TIMEOUT`, so `z`
   still waits and shows its own popup).
4. Regression tests: a new integration test file, `crates/fe-gpui/tests/which_key_hold.rs`, using public
   API only (`fe_gpui::{init, set_vim_enabled, set_vim_which_key, set_vim_remaps, EditorView,
   EditorTheme, keymap::CHORD_TIMEOUT}`, `fe_vim::Remap`, `kiln_platform::keys::chord_pending`, and debug
   selectors `which-key-popup` / `which-key-entry-<key>` through `VisualTestContext::debug_bounds` after
   `window.draw(cx).clear(cx)`). Set it up as `which_key_chord_tests::vim_editor` does (activate and focus
   the window, delay 200 ms). The first check that fails at base must be an `assert_eq!`.
   - `a_shown_chord_popup_outlasts_the_chord_timeout`: on "one two\nthree four", press `j w`, then `g`,
     and advance 200 ms. `which-key-entry-b` is drawn. Advance `CHORD_TIMEOUT * 3`, run until parked and
     draw. `assert_eq!(debug_bounds("which-key-entry-b").is_some(), true)` and `chord_pending` is still
     true (at base the chord popup closed at 1 s and vim's `g` list, without `b`, replaced it). Then press
     `e`: the popup is gone and the caret is at (1, 4) (`ge`).
   - `a_shown_map_popup_outlasts_the_map_timeout`: with noremaps `<leader>ff` -> `gg` and `<leader>w` ->
     `<Cmd>fe:editor.undo<CR>`, leader `,`, on six lines, press `j j ,` and advance 200 ms.
     `which-key-entry-f` is drawn. Advance `CHORD_TIMEOUT * 3`, run until parked and draw.
     `assert_eq!(debug_bounds("which-key-entry-f").is_some(), true)` (at base the held `,` resolved at 1 s
     and the popup closed). Then press `f f`: the popup is gone and the caret is on row 0.
