---
task_uid: "01m49phg86434rcnwq1h96nzv9"
task: "T147"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::completion::items_convert_to_byte_edits: items with textEdit, with additionalTextEdits, and with only insertText or only label convert to byte-range edits in the request's version, with an emoji earlier on the line, under utf-8 and utf-16 sessions; label, labelDetails, kind and detail carry over and isIncomplete is kept; initialize advertises snippetSupport false, insertReplaceSupport false, resolveSupport [documentation, detail] and labelDetailsSupport true"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::completion::resolve_triggers_and_timeouts: resolve fills documentation and detail; a resolve past 2 s and a completion past 3 s on the injected clock give the item unchanged and None, each sending $/cancelRequest; the server's triggerCharacters are reported for its documents"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test tests::completion::accept_is_one_user_transaction: a CompletionSession filters items by the word typed since the request and keeps the selection on the same item while it still matches; accepting an item with a main edit and an additionalTextEdits import line applies both as one Origin::User transaction through edit_from on the request's snapshot, also after two characters were typed since the request; one undo restores the text from before the accept"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::completion_tests::menu_opens_narrows_and_accepts: with a test CompletionProvider, ctrl-space opens the menu at the word start and typing a trigger character opens it too; typing narrows the rows; down and up move the selection; enter and tab accept; escape and moving the caret out of the word close it without editing; the menu shows label, detail and kind, and the selected item's documentation once resolved; with 1,000 items a frame builds no more rows than fit plus overscan"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::lsp_completion::complete_from_the_server: with a FakeLauncher whose fake answers textDocument/completion with an item carrying an additionalTextEdits import at the top of the file, ctrl-space then enter in an open .rs file inserts the item and the import, and one cmd-z removes both"
    required: true
    manual: false
  - id: "C6"
    text: "editor.trigger_completion (ctrl-space, editor context) and completion.select_prev, completion.select_next, completion.accept and completion.dismiss (up, down, enter and tab, escape, in the completion context) are registered through the command registry and bound in default-macos.toml; no KeyBinding::new or actions! is added; the binding counts in fe-gpui command_tests.rs and keymap_tests.rs are updated and pass; fe-gpui and fe-editor have no fe-lsp dependency; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Complete code from language servers: request, resolve, a completion menu, and accept as one undoable edit

# Complete code from language servers: request, resolve, a completion menu, and accept as one undoable edit

Card 9 of `docs/decisions/lsp-client.md` §8 (GPUI for the menu). The design is:

- §7's Completion row
- §3.5's timeouts: completion 3 s (the menu closes), resolve 2 s
- §3.6's last paragraph: an accepted completion, with its `additionalTextEdits`, is one `Origin::User`
  transaction

It builds on card 5's workbench wiring and card 6's request admission.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"): none beyond those cards'.

## Change

- **`crates/fe-lsp/src/completion.rs` (new).**
  - `LspManager::completion(buffer, version, offset, trigger)` and `resolve(item)`.
  - `textEdit` and `additionalTextEdits` convert to byte ranges in the request's version, with
    `insertText` or `label` as the fallback. `isIncomplete` is kept.
  - The server's `triggerCharacters` are exposed per document.
  - Advertise `completionItem { snippetSupport: false, insertReplaceSupport: false, resolveSupport:
    [documentation, detail], labelDetailsSupport: true }`.
- **`crates/fe-editor/src/completion.rs` (new): a GPUI-free `CompletionSession`.**
  - It keeps the request's snapshot, filters the items by the word typed since the request (fuzzy),
    and holds the selection.
  - **Accept.** Apply the main and additional edits as **one `Origin::User` transaction** through
    `fe_buffer::Buffer::edit_from` (`crates/fe-buffer/src/buffer.rs:346`) on the request's snapshot,
    so typing since the request is rebased and one undo removes the whole completion.
- **fe-gpui.**
  - **A provider seam.** The view gets a `CompletionProvider` seam (host-set trait object returning
    futures), so fe-gpui stays LSP-free.
  - **`src/completion_menu.rs` (new): the menu.** It is anchored at the word start and shows label,
    detail and kind. The selected item's resolved documentation shows when it arrives. Only visible
    rows are built.
  - **Opening.** `editor.trigger_completion` (ctrl-space) or typing a trigger character opens it.
    Typing word characters narrows it.
  - **Keys.** `completion.select_prev` / `select_next` (up/down), `completion.accept` (enter and tab)
    and `completion.dismiss` (escape), in a `completion` key context. Moving the caret out of the
    word closes it.
  - Registered through the editor command registry, bound in
    `crates/fe-keymap/keymaps/default-macos.toml`, and the counts in
    `crates/fe-gpui/src/command_tests.rs` and `crates/fe-gpui/src/keymap_tests.rs` are updated.
- **`crates/fe-workbench/src/lsp.rs`** sets each editor's `CompletionProvider`, backed by its
  buffer's server.

## Size

About 900 non-test lines estimated. If it runs over the cap, split off resolve and documentation
display rather than trim the accept path.

## Out of scope

- Snippets (`snippetSupport` is false).
- Insert/replace edits.
- Signature help.
- Auto-import beyond what `additionalTextEdits` carries.
- Completion from search.
