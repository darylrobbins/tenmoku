---
task_uid: "01m5n96tfqte25yhzta6jn62qd"
task: "T236"
rev: 3
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "A finished assistant turn whose Markdown holds two or more code blocks with a path= (parse_edit_block) shows a 'Review N changes' button in the turn's footer (selector chat-review-changes-\u003cturn\u003e) next to the per-block Apply buttons, which keep working unchanged; with one such block only Apply shows. Tests button_shows_for_two_path_blocks, single_block_has_only_apply, apply_still_proposes_one_block"
    required: true
    manual: false
  - id: "C2"
    text: "chat.review_changes (registry command, args {chat, turn}, dispatched as the AI actor from Source::Script exactly as chat.propose_edit is, so it is recorded and replayable, see chat_apply.rs) reads the turn's path blocks, proposes each against its file's current buffer (opening files as chat_apply does, with the same root, symlink and trust checks: reuse its locate/real_path functions, do not copy them), builds a Changeset (a block for a missing file is a NewFile entry only when its parent directory exists under the root) and calls Workbench::set_changeset, then shows the panel. Tests review_builds_changeset_for_all_blocks, review_skips_bad_block_and_toasts_the_reason, new_file_block_becomes_new_file_entry, path_outside_root_refused, review_is_dispatched_as_ai_actor"
    required: true
    manual: false
  - id: "C3"
    text: "A SEARCH that matches nowhere or twice, markers that do not match, a read-only or binary file: the file is left out of the changeset, a toast names the file and why (same messages as Apply), and the rest still load; if no block loads, no panel opens. Re-running Review on the same turn replaces that turn's changeset, never duplicating files. Tests partial_failures_do_not_block_others, rerun_replaces"
    required: true
    manual: false
  - id: "C4"
    text: "The Apply path for a block and this path share one function that turns (block, buffer) into a ProposedChange so errors and toasts cannot drift apart. Test same_error_text_as_apply"
    required: true
    manual: false
  - id: "C5"
    text: "Headless GPUI tests with scripted chat turns (chat_apply_tests.rs shows how a turn with blocks is built); Linux-OK; no golden"
    required: true
    manual: false
---

# Collect every file edit a chat reply proposes into one AI changeset with a Review Changes button on the turn, so a multi-file reply is reviewed, accepted or reverted together (Mac T1106)

## Problem
chat_apply.rs proposes one fenced block per Apply click. A reply that edits several files has no single action to see them all together. With cards 11 and 12 the proposals can be reviewed as one changeset with a checkpoint; this card connects chat to that.

## Change
- `chat_apply.rs` (876 lines): extract the block -> ProposedChange step (the part after the file is located and read) into a function shared with new `chat_changeset.rs`; no behaviour change for Apply. Keep the diff in chat_apply.rs to that extraction and the `pub(crate)` visibility it needs.
- `crates/fe-chat-view/src/view.rs` and `reply.rs`: a turn footer button that emits a new `ChatViewEvent::ReviewChanges{turn}`; count path blocks with `parse_edit_block` (fe-chat) over the turn's blocks already extracted for Apply. Any exhaustive `match` on `ChatViewEvent` in fe-workbench/chat_panel.rs must get an arm (rg `ChatViewEvent::`; chat_panel.rs is in scope only for that arm).
- Command registered like chat.propose_edit (find its registration in chat_apply.rs `registry`), plus a typed-command table row.

## Tests
New `chat_changeset_tests.rs`. Linux-OK. This card should be scheduled after card 12 lands; it edits chat_apply.rs, which no queued card touches (checked against T937 T939 T949 T1040 T1041 T1077-T1080 T877).
