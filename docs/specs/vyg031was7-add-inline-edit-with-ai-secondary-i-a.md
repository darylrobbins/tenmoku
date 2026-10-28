---
task_uid: "01m5n96t6nb3dtkbvyg031was7"
task: "T234"
rev: 3
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "New command ai.inline_edit ('Inline Edit with AI', context editor, default secondary-i; no binding when an editor is in vim normal mode, so bound in context editor; if a vim insert/normal binding of the same key exists, follow how neighbouring chords are scoped with the vim.mode contexts in fastedit.toml and keep vim's meaning) in the registry. With an editor focused it opens a one-line input anchored under the selection's last row (selector inline-edit-input), focused, placeholder 'Edit selected code, or describe code to insert'; the selection (or the caret) is anchored in the buffer so typing elsewhere does not move it. Tests command_opens_input_under_selection, input_is_anchored_through_edits, read_only_editor_refuses_with_toast"
    required: true
    manual: false
  - id: "C2"
    text: "Enter with a non-empty instruction sends fe_chat::run_inline_edit through the chat panel's provider factory (chat_panel.rs ProviderFactory / set_provider_factory, the same Claude client and API key the chat uses; with no key, a toast 'Set an API key in the chat' and the input stays), shows 'Generating…' with a Cancel (Escape) in the input row, and on Done sets the proposal on the editor with EditorView::set_proposals beside any existing ones and reveals its first hunk, exactly as chat_apply does; the input closes. Failed or NoCode shows the message in the input row and keeps the instruction for retry. Tests enter_streams_and_shows_hunks, no_key_toasts, failed_keeps_instruction, no_code_reply_message"
    required: true
    manual: false
  - id: "C3"
    text: "Accepting or rejecting hunks uses the existing proposal keys and buttons (proposal_commands.rs); nothing new there. Escape while generating drops the stream (the fake provider's dropped() is 1) and leaves the buffer untouched; Escape in the empty input closes it. Edits made to the buffer during generation move the anchored range; if the range was deleted the result is discarded with a toast. Tests escape_cancels_the_provider, buffer_untouched_until_accept, deleted_range_discards_result"
    required: true
    manual: false
  - id: "C4"
    text: "A second ai.inline_edit while one is generating focuses the first; the input of an editor in an unfocused split does not steal focus. Tests second_invocation_focuses_existing"
    required: true
    manual: false
  - id: "C5"
    text: "Headless GPUI with ScriptedProvider; Linux-OK; no golden"
    required: true
    manual: false
---

# Add Inline Edit with AI (secondary-i): a small input over the selection, a streamed reply shown as proposal diff blocks in place with Accept / Reject, and Escape to cancel, over the chat's Claude provider (Mac T1103)

## Problem
Card 9 turns an instruction and a selection into a ProposedChange headlessly. VS Code shows the input over the code (inline chat) and the diff in place; this card is that front end for fastedit. "Ask About Selection" in the chat (chat.ask_about_selection) is the nearest existing feature; it goes through the side panel and Apply.

## Change
- New `crates/fe-workbench/src/inline_edit.rs` (entity per editor, kept in a map on the Workbench keyed by editor entity id; dropped with the editor). The input is a single-line editor view like the chat composer's (see `chat_composer` in chat_panel.rs and `find_bar.rs`), placed as an overlay on the `EditorView` using the same anchored-overlay mechanism the LSP rename box uses (`lsp_rename.rs`).
- The provider: add a small `pub(crate) fn provider(cx) -> Option<Arc<dyn ChatProvider>>` on the chat panel module (the only reason chat_panel.rs is in scope), reusing its factory/key resolution; do not duplicate key handling.
- Adding the proposal: reuse the helper `chat_apply.rs` uses to show a ProposedChange and reveal its first hunk; if it is private, make it `pub(crate)` (one-line change; chat_apply.rs added to scope only for that visibility change).
- Command + key + tables as in card 7's notes.

## Tests
New `inline_edit_tests.rs` and `crates/fe-keymap/tests/inline_edit_binding.rs`. Linux-OK. The model never runs for real in tests. A manual check in the running app with a real key is left to a verification card the lead can file (like T261/T503).

## Overlay (lead, 2026-10-24)
The overlay hook this card needs does not exist in EditorView; T242 adds `EditorView::set_overlay(Option<AnyView>, anchor: Range<usize>)` (+ `overlay_anchor_row()`), drawn deferred and anchored just under the anchor's last row. T242 is this card's dependency: use it for the input (selector `inline-edit-input`) and stay inside this card's scope; do not touch fe-gpui view.rs/element.rs. ChatPanel's `pub(crate) fn provider(cx)` stays in chat_panel.rs, which is in scope.

## Implementation notes

- The inputs are kept in an app `Global` keyed by editor entity id (workbench.rs is outside the scope), dropped with the editor.
- Enter and Escape in the input are `inline_edit.submit` / `inline_edit.cancel` (palette = false), bound in the `inline_edit` key context at the end of the default keymap; `ai.inline_edit` is bound `secondary-i` as the editing keys are scoped (`!vim.enabled`, vim insert and replace), so vim's `<C-i>` stays.
- The proposal is added as `chat_apply::show_proposal` does (beside the editor's proposals, first hunk revealed) in inline_edit.rs, as `show_proposal` is private to chat_apply.rs, which isn't in scope; the tab-preview branch doesn't apply to a focused editor.
- `chat_panel::provider(workbench, cx)` returns the panel's current provider and the chat settings, reading the key first if the chat never drew.
- fe-workbench gains `futures` as a regular dependency, and fe-gpui's keymap tests list the `inline_edit` contexts.
