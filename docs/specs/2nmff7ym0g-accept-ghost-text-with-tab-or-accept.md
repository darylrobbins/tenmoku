---
task_uid: "01m4rqejkfk1akf62nmff7ym0g"
task: "T53"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::ghost_text_key_tests::tab_accepts_one_undo_step: tab with a suggestion shown inserts the rest and emits Accepted { whole: true }, cmd-z removes exactly that, and tab with none shown still runs editor.tab"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::ghost_text_key_tests::completion_tab_wins: with the completion menu and a suggestion both shown, tab accepts the completion item"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::ghost_text_key_tests::vim_insert: in vim insert mode tab accepts and typing consumes; one escape drops the suggestion, emits Dismissed and enters normal mode; u then undoes the insert session including the accepted text"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::ghost_text_key_tests::escape_blur_and_accept_word: outside vim, escape and blur each drop it with one Dismissed and no edit, and cmd-right (ctrl-right off macOS) accepts one word with the rest still shown"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-gpui/src/command_tests.rs and keymap_tests.rs pass unchanged, with the four ghost_text.* commands registered in the ghost_text context and rows in all three keymap files"
    required: true
    manual: false
---

# Accept ghost text with tab or accept-word, and dismiss it with escape or blur, in plain and vim insert mode, with an event telling the host which (Mac T391)

Filed on helium from Mac T391 (~/coordination/helium-cards/T391.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1, the proposal/AI chain, run strictly one after another for crates/fe-gpui/src/view.rs): counts the card pins (keymap rows in default-macos.toml, keymap_tests.rs and similar) were taken on Mac main; on helium use the counts on your base and change them by exactly the deltas the card states. Screenshot goldens are Metal-only and do not run on helium: a golden criterion is met here by the Linux suite passing, and the Mac regenerates or checks goldens after the sync. Argon's Mac T427 (multi-cursor keys, alt-click) also changes view.rs and vim.rs but is not on helium: do not port it or code around it.

## Problem

After the render card, `EditorView` shows a `GhostText` but nothing accepts or dismisses it. The user's ask
(2026-10-11): "Tab (or a command) accepts it as one undo step …; Esc / moving away dismisses; accept-word /
accept-line commands are nice-to-have", and vim insert mode must coexist. Today `tab` is `editor.tab` in the
`editor` context, which vim's insert mode keeps (`crates/fe-keymap/keymaps/fastedit.toml` ~191, and the insert
rows ~696: "Insert mode keeps the editor's keys; `escape` and `ctrl-[` end the session"). The completion menu
already solved the same problem: a key context flag stamped on the editor frame while it shows
(`completion_menu::stamp`, used in `EditorView::render` and `vim::live_stack`, `crates/fe-gpui/src/vim.rs:252`),
rows "later at the same depth" winning over the editor's and vim's insert keys, commands registered in
`crates/fe-gpui/src/commands.rs` `editor_registry`, and `escape`/blur closing it through
`close_completion_on_escape_and_blur`. The host app needs to hear what the user did, to tell the agent, as with
T258's `ProposalEvent`.

## Change

In `crates/fe-gpui/src/view/ghost_text.rs`:
- Commands `ghost_text.accept`, `ghost_text.accept_word`, `ghost_text.accept_line`, `ghost_text.dismiss`,
  registered in `editor_registry` (`commands.rs`) in the `ghost_text` context and run by the view like the
  completion commands: they call `GhostText::accept` / `accept_word` / `accept_line` on `self.model`, then
  `self.changed(cx)`. After `accept` the suggestion is dropped; after a partial accept it stays while live.
- Key context flag `ghost_text`, set on the editor frame while a live suggestion shows: in `EditorView::render`
  and in `vim::live_stack` (one `stamp` call each, like `completion_menu::stamp`).
- `escape` anywhere and losing focus drop the suggestion, through a keystroke interceptor and `on_blur` like
  `close_completion_on_escape_and_blur`; `escape` is not bound to a ghost command, so one `escape` in vim insert
  mode both drops it and returns to normal mode.
- `pub enum GhostTextEvent { Accepted { transaction: TransactionId, text: String, whole: bool }, Dismissed }` with
  `impl EventEmitter<GhostTextEvent> for EditorView`: `Accepted` after each accept; `Dismissed` once when the
  suggestion goes for any reason but an accept or the host's own `set_ghost_text`. Export it from
  `crates/fe-gpui/src/lib.rs` (one `pub use` line).

Keymaps, in `fastedit.toml`, `default-macos.toml` and `default-linux.toml`, rows just before the completion rows
(so the completion menu's `tab` wins when both show): `tab` → `ghost_text.accept` and accept word on
`{ macos = "cmd-right", default = "ctrl-right" }` → `ghost_text.accept_word`, both in context `ghost_text`.
`accept_line` and `dismiss` are unbound (palette).

Not in this card: asking a provider for suggestions or debouncing (the host pushes them).

## Tests

New file `crates/fe-gpui/src/view/ghost_text_key_tests.rs` (declared `#[cfg(test)]` from `view.rs`), simulating
keystrokes through the default keymap as `crates/fe-gpui/src/view/completion_tests.rs` does.

## Implementation notes

- **C5 can't hold literally.** `command_tests.rs::table_is_the_only_binding_source` and
  `keymap_tests.rs::default_keymap_matches_the_old_table` pin `DEFAULT_MACOS` at 167 rows, and the first lists the
  contexts a row may be in outside the editor's frame. The card's two rows make 169 and add the `ghost_text`
  context, so both tests changed by exactly that: 167 → 169 in each, and `"ghost_text"` in command_tests' context
  list (its comment gains "2 for the ghost text"). Nothing else in them changed; `OLD_TABLE` is a prefix of the
  keymap and the new rows sit near its end. T68 needed the same change and got these files added to its scope by
  a respec.
- `GhostTextEvent::Accepted { whole }` is true for `ghost_text.accept`, and for a partial accept that took the
  last of it (no `Dismissed` follows). `text` is what went in.
- `Dismissed` covers every other way it goes: `escape`, blur, `ghost_text.dismiss`, typing what isn't suggested,
  typing all of it, moving the caret, leaving vim's insert mode. `set_ghost_text` (replace or clear) emits nothing,
  and now also refuses a suggestion set where vim takes no text (normal or visual mode), so it can't be stored only
  to be dismissed by the next frame.
- In vim's insert and replace modes the `ghost_text.*` commands don't split the insert session, as the completion
  commands don't; `GhostText::accept` joins the open transaction, so `u` takes the accepted text back with the rest.
- A read-only view refuses the three accepts, as it refuses `completion.accept`.
- Carried T52 F1: `drop_stale_ghost_text` (render, and `vim::live_stack` before a key is matched) now syncs the
  model before checking liveness, so a peer edit that ends the suggestion clears `ghost_text()` in the same frame
  that paints nothing. `ghost_text_key_tests::shared_buffer_edit_drops_it_in_one_frame` fails without it.
