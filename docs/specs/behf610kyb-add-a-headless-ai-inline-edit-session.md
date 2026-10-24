---
task_uid: "01m5n96sytn2473vbehf610kyb"
task: "T232"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "inline_request(InlineEditInput{path, language, text_before, selection, text_after, instruction, settings}) builds a ChatRequest: a system block that tells the model to answer with only the replacement for the selection in one fenced code block (no prose outside the fence), the file path and language, and one user message holding up to 4,000 bytes of text before and after (cut at line starts) and the selection between markers. A caret with no selection means insert at the caret. Tests request_holds_selection_between_markers, context_is_cut_at_line_starts, caret_means_insert, instruction_is_the_last_user_text"
    required: true
    manual: false
  - id: "C2"
    text: "InlineEditSession consumes ChatEvent items (text deltas, stop, error as fe-chat's provider.rs defines them) and exposes: text_so_far() (the fence's body read incrementally, tolerating a split ``` or a language tag arriving across deltas), state() in {Streaming, Done, Failed(ProviderError), NoCode}, and finish() -\u003e Option\u003cString\u003e (the replacement; a reply with no fence at all is NoCode, never guessed at). Tests fence_split_across_deltas, language_tag_ignored, prose_before_and_after_the_fence_ignored, no_fence_is_no_code, provider_error_is_failed, partial_reply_text_so_far_grows"
    required: true
    manual: false
  - id: "C3"
    text: "run_inline_edit(provider: \u0026dyn ChatProvider, input) -\u003e impl Stream of InlineEditUpdate{TextSoFar, Done(String), Failed} drives ScriptedProvider (fe-chat fake.rs) in tests; dropping the stream cancels (ScriptedProvider::dropped() counts it). Tests scripted_reply_streams_updates, dropping_cancels_the_provider"
    required: true
    manual: false
  - id: "C4"
    text: "to_proposal(snapshot, range, replacement, origin) -\u003e ProposedChange uses ProposedChange::from_text over the range with Origin::Ai{agent: \"fastedit.inline_edit\", session}, so only changed lines become hunks; an unchanged replacement yields no hunks. Tests replacement_becomes_hunks_for_changed_lines_only, identical_replacement_has_no_hunks, insertion_at_caret_is_one_hunk. fe-chat gains fe-buffer as a dependency only if it lacks it (internal crate)"
    required: true
    manual: false
  - id: "C5"
    text: "Headless, no new third-party dependency; runs on Linux"
    required: true
    manual: false
---

# Add a headless AI inline-edit session: build the model request from a selection (or caret) and an instruction, read the streamed reply into the replacement text, and give it to the editor as a ProposedChange, tested with the scripted provider (Mac T1102)

## Problem
VS Code's inline chat (cmd-I): select code, type an instruction in a small box, and the model's edit appears in place as a diff to accept or discard. fastedit has the pieces (fe-chat's `ChatProvider` + `fake::ScriptedProvider`, fe-buffer's `EditStream` and `ProposedChange`, the proposal diff blocks in fe-gpui `view/proposal*.rs`), and chat Apply (`fe-workbench/src/chat_apply.rs`) proposes a fenced block, but nothing turns a selection plus an instruction into an edit. This card is the headless core: request, reply reading, and the ProposedChange; the UI is card 10.

## Change
New `crates/fe-chat/src/inline.rs`, declared and re-exported from `lib.rs`. Reuse `ChatRequest`, `SystemBlock`, `Effort`, `ChatSettings` (request.rs) for model, max_tokens and effort; do not use `build_request` (it builds chat history). Fence reading can share a helper with `edits.rs::parse_edit_block` only by calling it on the completed reply; streaming fence reading is new and stays in inline.rs. Design note: the reply is turned into a ProposedChange when the stream finishes (plus TextSoFar updates for a live preview line); a live-applied EditStream is not used in this card because rejecting would then be an undo; a progressive in-place diff is a possible follow-up.
Do not touch `chat_apply.rs`, `provider.rs` or the Anthropic client.

## Tests
New `crates/fe-chat/tests/inline_edit.rs`. Use `fe_buffer::Buffer` for the snapshot (see `crates/fe-buffer/tests/proposed_change.rs` for construction). Headless, Linux-OK.
