---
task_uid: "01m55mj91tbcbptakkn44023sw"
task: "T162"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test occurrence_carets_add_next_and_all (top level, in crates/fe-editor/tests/occurrence_carets.rs) passes: editor.add_caret_at_next_occurrence adds carets at the word's next whole-word, case-sensitive occurrences at the same offset into the word, wrapping once and skipping occurrences that hold a caret; editor.add_carets_at_all_occurrences adds them all; neither leaves a selection."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test vim_multi_cursor_keys_resolve_in_normal_mode (top level, in crates/fe-keymap/tests/vim_multi_cursor_keys.rs) passes: g b everywhere, cmd-d, cmd-shift-l and cmd-alt-up/down on macOS and alt-shift-up/down elsewhere resolve to their commands only in vim normal mode with nothing pending, and the vim-off bindings resolve as before."
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test vim::visual_tests::multi_caret_tests::gb_adds_carets_that_vim_edits_together (new crates/fe-gpui/src/vim/multi_caret_tests.rs, declared in crates/fe-gpui/src/vim/visual_tests.rs) passes: in a real view with vim on, g b g b then c i w x escape changes three occurrences, and escape again leaves only the primary caret."
    required: true
    manual: false
---

# Add carets in vim normal mode: gb and cmd-d at the word's next occurrence, cmd-shift-l at every one, cmd-alt-up and down, and escape back to one caret (Mac T901)

Filed on argon from Mac T901 (card ~/coordination/argon-cards/zzp/T901.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55mer7qt9b9cxn1kvxeh9qq. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Held in the inbox: waits until Mac T900 (another host's or the Mac's) is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T901)".

## Problem

At Mac main 6f032b12 every multi-cursor binding is `editor && !vim.enabled`
(`crates/fe-keymap/keymaps/fastedit.toml`, "Multi-cursor, as VS Code binds it, with vim off: with vim on, its
keys (escape, ctrl-d and the rest) stay vim's in every mode"). That's right off macOS, where `secondary` is
ctrl, but on macOS cmd keys never reach vim (`vim_key` returns `None` for `platform`), so vim users lose
`cmd-d`, `cmd-alt-up`/`down` and `cmd-shift-l` for nothing. fe-vim's normal mode already acts at every caret
(operators, `.`, inserts). VSCodeVim adds `gb` (add the next occurrence) for the same in vim mode. The user
approved multi-cursor in vim mode in the "Vim parity" track on 2026-10-17.

## Change

- fe-editor (`crates/fe-editor/src/multi_cursor.rs`): new commands `editor.add_caret_at_next_occurrence`
  (a caret at the next whole-word, case-sensitive occurrence of the word under the primary caret, after the
  bottommost caret, wrapping once, at the same offset into the word; the first use with one caret adds the
  next occurrence's caret) and `editor.add_carets_at_all_occurrences`, both leaving carets, not selections.
  Registered by the module's `register` as its other commands are.
- Keymap, in `editor && vim.mode == normal && !vim.pending`: on macOS `cmd-alt-up`/`cmd-alt-down` →
  `editor.add_cursor_above`/`below`, `cmd-d` → `editor.add_caret_at_next_occurrence`, `cmd-shift-l` →
  `editor.add_carets_at_all_occurrences`; on every platform `g b` → `editor.add_caret_at_next_occurrence`;
  off macOS `alt-shift-up`/`down` → add cursor above/below (vim reads them as the arrows today).
- `crates/fe-gpui/src/vim.rs`: an `escape` in normal mode with nothing pending and several carets keeps only
  the primary caret, as with vim off.
- Update `crates/fe-gpui/src/command_tests.rs`'s binding count.

Visual-mode multi-selection (VSCodeVim's `cmd-d` selecting in visual mode) is out of scope: fe-vim's visual
mode has one selection.

Tests: a new `crates/fe-editor/tests/occurrence_carets.rs` (next, wrap, all, word boundaries, case), a new
`crates/fe-keymap/tests/vim_multi_cursor_keys.rs` (each binding resolves only in vim normal mode, per
platform), and a new `crates/fe-gpui/src/vim/multi_caret_tests.rs`, declared in the existing
`crates/fe-gpui/src/vim/visual_tests.rs` by `#[path = "multi_caret_tests.rs"] mod multi_caret_tests;`: in a
real view with vim on, `g b g b` then `c i w x escape` changes three occurrences, and `escape` again leaves
one caret.

fastedit.toml and command_tests.rs are also changed by the IDE-keys card: this card depends on it.

## Implementation notes

- An occurrence holds a caret when a caret is on one of its chars, or where its new caret would go. The word
  is the one at the primary (bottommost) caret, as `select_next_occurrence` seeds it; on whitespace, or at
  the end of the text after it, there's none and no caret is added. Either way every selection becomes a
  caret at its head.
- `escape` collapses through `editor.single_selection`, both typed and in a macro replay (`<Esc>` in normal
  mode with nothing pending).
- Five existing tests outside the listed scope count or list the commands and bindings, and needed the new
  ones: `crates/fe-editor/src/tests/commands.rs` (registered commands), `crates/fe-editor/tests/viewer.rs`
  (unsupported in a viewer), `crates/fe-gpui/src/keymap_tests.rs` (binding counts: the new keys share the
  language-server keys' context, which that test counts with the workbench's) and
  `crates/fe-gpui/src/view/multi_cursor_tests.rs` (`vim_keeps_its_keys`: the cmd keys now add carets in
  normal mode, still not in visual mode). `crates/fe-workbench/src/tests/default_keymaps.rs` lists
  `editor.add_carets_at_all_occurrences` as macOS-only: the spec gives it no key off macOS, where
  `ctrl-shift-l` is vim's.
