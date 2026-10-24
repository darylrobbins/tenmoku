---
task_uid: "01m5n8y54bfe3zcjb4qyap9vb9"
task: "T207"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "In a TestAppContext editor in insert mode with inoremap jk \u003cEsc\u003e, typing j then k leaves insert mode and types no text; typing j then x types jx; both go through feed_vim and the IME path as a real keystroke does (tests in the new remap_insert_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "A held insert prefix (inoremap jk X, typing j) types nothing, sets vim.pending, and after CHORD_TIMEOUT (test clock advance) or a blur resolves to the typed j; a second editor's wait or a mouse press resolves it as in normal mode"
    required: true
    manual: false
  - id: "C3"
    text: "Replace mode applies i maps and overwrites with the expansion; a composition in progress and a multi-char commit (paste, IME) are never matched"
    required: true
    manual: false
  - id: "C4"
    text: "A macro recorded in insert mode with an imap that fires records the typed keys (not the expansion), and @-replaying it (EditorView::replay_key Insert/Replace arm) fires the map again with the same result; a map whose expansion has \u003cEsc\u003e or \u003cCR\u003e works"
    required: true
    manual: false
  - id: "C5"
    text: "The keymap still wins: a left side the keymap binds in vim.mode == insert is never offered to remap_insert; existing remap_tests, remap_cmd_tests, insert_tests and macro_tests in fe-gpui pass unchanged"
    required: true
    manual: false
---

# GPUI editor: imap/inoremap in insert and replace mode through remap_insert, with the CHORD_TIMEOUT wait and replay (T877 C) (Mac T1091)

## Problem

T877 split, card C. With card A, fe-vim can resolve `i` maps for insert and replace
mode, but the GPUI editor never asks. Insert text reaches the editor three ways
and none calls `VimState::remap_insert`:
1. a printable keystroke unbound in the keymap goes to `EditorView::on_vim_key` ->
   `feed_vim`'s `accepts_text()` branch (`crates/fe-gpui/src/vim.rs` ~1665-1730),
   which returns `false` for plain chars, so the IME path (`view/ime.rs`
   `replace_text_in_range` -> `commit` -> `type_text`) types it;
2. unbound ctrl keys in the same `feed_vim` branch (`insert_key_typing`);
3. a macro/map replay: `EditorView::replay_key`'s Insert/Replace arm (vim.rs
   ~2536-2590) types keys itself.
Typed text is recorded for macros via `record_typing` (commands.rs `run_command`),
and replays set `vim_replaying` so nothing is recorded twice.
The keymap still wins first (record section 4 rule 1); while a prefix is held,
`live_stack` already sets `pending` for `remap_waiting()`, so the keymap is skipped.

## Change

- `crates/fe-gpui/src/vim.rs`: in `feed_vim`'s insert/replace branch, a keystroke
  that is a plain character or a ctrl key goes to `remap_insert` first. `Wait`
  swallows the keystroke (returns true, no text typed) and arms the existing
  `wait_for_remap` timer (`CHORD_TIMEOUT`, blur resolves it too);
  `Keys` types each key through the same paths a live key takes (`type_text`,
  `insert_key_typing`, indent/complete/register helpers, a map's `<Esc>` as
  `vim.normal_mode`), with expansion keys not recorded and the typed ones
  recorded once (`record_vim_key`/`record_typing` must not double count; a macro
  `qa ijk<Esc> q` records `ijk<Esc>`).
  `resolve_remap_from`/`replay_vim_from` already feed what `remap_timeout`
  resolves: make `replay_key`'s Insert/Replace arm call `remap_insert` for keys
  tagged as typed, as the Normal path does via `feed`.
- `crates/fe-gpui/src/view/ime.rs`: only if the IME path needs a guard (a
  composition in progress is never remapped; a text commit longer than one char,
  paste and IME results are not matched). Prefer no change.
- New tests in NEW file `crates/fe-gpui/src/vim/remap_insert_tests.rs`, registered
  with one `#[cfg(test)] #[path]` line next to `remap_tests` in `vim.rs`; use the
  `vim_editor` helper pattern of `remap_tests.rs` (TestAppContext, no screenshots,
  no OS APIs).
- Not in this card: the status-bar command line (card D), vimrc/settings (B).

## Notes

- Needs card A merged. Disjoint from card B (fe-vim vimrc, fe-config, fastedit), so
  B and C can run in parallel; tests here build `Remap` records as
  `remap_tests.rs` does.
- Linux-safe: same TestAppContext tests as `crates/fe-gpui/src/vim/remap_tests.rs`.
