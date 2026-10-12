---
task_uid: "01m4ewvkzggt9e65pt37pyjaz0"
task: "T230"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::scroll_tests::the_view_follows: with vim on, in an editor 10 rows tall over 100 rows, ctrl-d scrolls the view by 5 rows (the first visible row is 5 after a draw) and moves the caret 5 rows; zt with the caret on row 30 makes row 30 the first visible row after the draw, and autoscroll doesn't move it back; zb makes it the last; ctrl-e scrolls one row; with vim off ctrl-d does what the default keymap does today"
    required: true
    manual: false
  - id: "C2"
    text: "Scope: nothing under crates/fe-vim or crates/fe-editor changes; vim.rs changes only by the test module's declaration; fe-gpui's other vim tests pass unchanged; the screenshot golden images are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Off macOS vim keeps its scrolling keys: fastedit.toml binds ctrl-y, ctrl-f and ctrl-b to vim.key with platform = \"!macos\" in contexts editor \u0026\u0026 vim.mode == normal and editor \u0026\u0026 vim.visual, shaped as the ctrl-v/ctrl-q bindings; default-linux.toml gains exactly those six bindings and default-macos.toml none; fe-keymap test one_keymap passes; fe-workbench test tests::default_keymaps::vim_keeps_its_ctrl_keys_on_linux checks ctrl-y, ctrl-f and ctrl-b resolve to vim.key in normal and visual mode and not in insert mode (ctrl-y stays editor.redo there); the counts pinned in crates/fe-gpui/src/command_tests.rs and keymap_tests.rs (143 and 61, counted on default-macos.toml) are unchanged"
    required: true
    manual: false
---

# Vim scrolling in the editor: the view follows ctrl-d ctrl-u ctrl-e ctrl-y and zt zz zb, and autoscroll doesn't undo them

# Vim scrolling in the editor: the view follows ctrl-d, zt, zz and zb, and autoscroll doesn't undo them

Milestone M7 "Vim", second batch (resized), card 10b: the GPUI half of the old card 10. Read 10a
(`EditorHost::viewport` / `scroll`).

**Clean room** as every M7 card: no Vim/Neovim source, no hjkl, no Zed, no Helix.

## Change (`crates/fe-gpui/src/commands.rs`, `crates/fe-gpui/src/view.rs`)

- `ViewHost` answers `viewport()` from the last laid-out frame and turns `scroll()` into a pending
  scroll that `EditorView` applies after the dispatch: `scroll.y = top_row * line_height`, and
  autoscroll is off for that frame, so `zt` keeps the cursor's row at the top even though it is
  inside fe-display's autoscroll margin.
- Scroll distances (ctrl-d/u/e/y, zt/zz/zb alignment, cursor visibility) count rows as fe-vim does, through
  the `EditorHost` viewport/`TopRow` contract: buffer rows (fe-editor docs: "in the text's rows"). Only the
  view's top row is converted through `EditorModel`'s display map. Under folds or soft wrap crossing the
  viewport the distances are therefore buffer-row distances, not display-row exact; that is a follow-up
  that changes the contract (out of scope here).

### Linux keys (`crates/fe-keymap/keymaps/fastedit.toml`)

`docs/decisions/vim-engine.md` §10: in normal and visual mode vim's ctrl keys win. On macOS no
`ctrl-` binding applies in the editor, so all six reach vim unmatched. Off macOS three of them are
taken by non-vim bindings: `ctrl-y` (`editor.redo`, context `editor`), `ctrl-f` (`find.show`,
`editor`) and `ctrl-b` (`dock.toggle`, `workbench`, which matches from the editor when nothing
deeper does). `ctrl-d`, `ctrl-u` and `ctrl-e` are bound to nothing there and already reach vim.

- Add `vim.key` bindings for `ctrl-y`, `ctrl-f` and `ctrl-b` with `platform = "!macos"`, one in
  context `editor && vim.mode == normal` and one in `editor && vim.visual`, shaped exactly as the
  existing `ctrl-v`/`ctrl-q` ones in the vim layer and placed beside them: six bindings. Update the
  vim layer's comment that names ctrl-v and ctrl-q.
- Regenerate the resolved files: `crates/fe-keymap/keymaps/default-linux.toml` gains exactly these
  six (140 -> 146 bindings); `crates/fe-keymap/keymaps/default-macos.toml` gains nothing.
  `crates/fe-keymap/tests/one_keymap.rs` checks the three agree.
- The counts pinned in `crates/fe-gpui/src/command_tests.rs` and `crates/fe-gpui/src/keymap_tests.rs`
  (143 and 61) count `default-macos.toml`, so they rise only by what this adds there: nothing. Those
  files don't change.
- Outside normal and visual mode (insert mode, vim off) the three keep their Linux commands.

## Tests (new files)

`crates/fe-gpui/src/vim/scroll_tests.rs`, declared from `vim.rs` as its other test modules (the
declaration is `vim.rs`'s only change).

Plus, in the existing `crates/fe-workbench/src/tests/default_keymaps.rs`,
`vim_keeps_its_ctrl_keys_on_linux`'s key list gains `ctrl-y`, `ctrl-f` and `ctrl-b`.

## Size

About 150 non-test lines, plus six keymap bindings.

## Out of scope

Mouse-wheel behaviour, smooth scrolling, scrolling with vim off. Any fe-vim change. Any vim binding
on macOS.

## Decided (lead defaults; the user may revisit)

- **`zt` beats fastedit's autoscroll margin** for the frame it runs in; afterwards ordinary caret
  moves autoscroll as before. nvim's 'scrolloff' is 0, so nvim's results are the reference.

- **Buffer-row scroll semantics under folds and soft wrap** (lead decision, rev 3, after review round 2 F1):
  display-row exactness needs a display-row mapping on `EditorHost`, which changes fe-vim and fe-editor;
  that is a separate follow-up card, not this one.
- **The running-app check** (old C3, review F4) is a person's: it moved to a human check task, since a
  worker can't drive the GUI.

## Implementation notes

- `vim.rs` can't carry the new `ViewHost` fields, so the viewport and the pending scroll pass through thread-locals in `commands.rs`: `EditorView::page_rows` (asked at the start of every dispatch) publishes the viewport, and `EditorView::dispatched` applies a requested top row after the dispatch.
- vim's `ctrl-f` bindings sit after `find.show` in `fastedit.toml` rather than beside `ctrl-v`/`ctrl-q`: a later binding wins at equal depth, and `find.show` comes after the vim layer.
- Review round 1: the viewport and `TopRow` now go through the display map (buffer rows to fe-vim, display rows to the scroll offset); `EditorView::scrolled_top` stands in for the frame's top until the next frame is laid out, so scrolls replayed before a draw add up; any dispatch that asked for the viewport pins the top and turns autoscroll off, even when the top row didn't change.
