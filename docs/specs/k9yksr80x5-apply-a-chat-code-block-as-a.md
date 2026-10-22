---
task_uid: "01m5f6t81cg1yrevk9yksr80x5"
task: "T251"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-chat test edit_blocks_parse (tests/edits.rs): path= is read from the info words; a body without markers is Replace; two SEARCH/REPLACE sections parse in order; an unbalanced marker is Malformed at its line; an absolute path or one with .. is a Path error"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test chat_panel::chat_panel_tests::apply::a_whole_file_block_proposes_its_diff: Apply on a path= block for an open file with unsaved edits shows a proposal in its editor whose hunks are only the changed lines, with origin Ai { agent: fastedit.chat, session: the chat id }; accepting a hunk is one User transaction that editor.undo takes back"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test chat_panel::chat_panel_tests::apply::search_replace_and_selection_blocks_target_their_ranges: sections propose at each unique match; a search text found twice or nowhere proposes nothing and toasts which; a block with no path replaces the selection attached when its turn was sent, after edits above it moved it"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test chat_panel::chat_panel_tests::apply::apply_is_an_ai_invocation: observe_invocations sees chat.propose_edit with args chat, turn and block, actor Ai { agent: fastedit.chat, session: the chat id } and source script for each Apply; the user's chat.send is not an Ai invocation"
    required: true
    manual: false
---

# Apply a chat code block as a ProposedChange in its file (whole file, SEARCH/REPLACE or the asked-about selection), dispatched as chat.propose_edit by the AI actor (Mac T971)

# Apply a chat reply's code block as a proposed change in the editor, by an AI-actor invocation

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) turns a
reply's code blocks into `ProposedChange` diff blocks the user accepts or rejects (record §8,
user decision 3), and every AI action is an `Invocation` with the AI actor (§9). The Markdown
card gives each code block an Apply button emitting `ChatViewEvent::ApplyBlock { turn, block }`;
nothing acts on it. The seam exists and nothing in the workbench uses it yet:
`ProposedChange::from_text` / `from_edits` (`crates/fe-buffer/src/proposed.rs`) and
`EditorView::set_proposals` (`crates/fe-gpui/src/view/proposal.rs`), whose accept is one
`Origin::User` transaction (`docs/ai-host.md`, "The undo rule").

## Change

**Headless, `crates/fe-chat/src/edits.rs`** (re-exported from `lib.rs`):
`parse_edit_block(info: &str, code: &str) -> EditBlock`, where `EditBlock { path:
Option<String>, body: EditBody }` and `EditBody` is `Replace(String)` (the whole file, or the
selection when there's no path) or `Sections(Vec<(String, String)>)` (search, replace) from
`<<<<<<< SEARCH`, `=======`, `>>>>>>> REPLACE` lines (exact markers on their own lines);
unbalanced markers are `EditBlockError::Malformed { line }`. `path=` is read from the info
string's whitespace-separated words (` ```rust path=src/lib.rs `); a `path` that's absolute or
leaves the root with `..` is `EditBlockError::Path`.

**In the workbench**, new `crates/fe-workbench/src/chat_apply.rs`, declared in `chat_panel.rs`
with `#[path]` as `chat_context.rs` is:

- `ApplyBlock` from the view: the panel finds the fence in the reply's text (T846's document),
  parses it, and dispatches `chat.propose_edit` with args `{ "chat": <chat id>, "turn", "block" }`
  as an `Invocation` with `Actor::Ai { agent: "fastedit.chat", session: <chat id> }` and
  `Source::Script`, so command observers see it (`kiln_platform::commands::observe_invocations`).
  If a `#[kiln::command]` method can't take args or see the actor, register `chat.propose_edit`
  in a small `fe_command::Registry` in `chat_apply.rs` (its handlers get the `&Actor`, as
  `dock.rs`'s do) and call `notify_observers` after dispatching; say which in the finish notes.
- The handler resolves the target, all against the buffer's current snapshot:
  - `path` given: opens the file in the active pane (an open buffer is reused, so unsaved
    edits are what's matched); `Replace` diffs the whole file (`from_text(snapshot,
    0..len, text, origin)`); `Sections` finds each search text's one exact match and builds
    `from_edits` (sections whose matches overlap are an error).
  - no `path`: the turn's selection attachment, kept as anchors in the buffer when the turn was
    sent, is replaced (`from_text` over that range). No selection and no path: a toast "This
    block doesn't say which file it's for".
  - `origin` is `Origin::Ai { agent: "fastedit.chat", session: <chat id> }`.
  - Shown with `EditorView::set_proposals`, adding to any proposals already shown in that
    view, and the editor scrolled to the first hunk.
  - A search text found nowhere or more than once, a malformed block, a binary or read-only
    file: a toast naming the file and the reason; nothing is proposed.
- `OpenPath { path, row }` from a reply's link opens the file at that row (relative to the root;
  outside it does nothing).
- The selection anchors: when `chat.send` attaches a selection, the panel keeps
  `(buffer id, start anchor, end anchor)` for the turn in memory (not persisted).

Tests: `crates/fe-chat/tests/edits.rs` (top-level fns); the workbench side in
`crates/fe-workbench/src/chat_apply_tests.rs`, declared from `chat_panel_tests.rs` as
`#[path = "chat_apply_tests.rs"] mod apply;`, on GPUI's test platform with a temp folder and
`fe_chat::ScriptedProvider` replies holding the blocks.

## Round 4 design (Mac y34 process: respec instead of an override)

Rounds 1-3 fixed containment, stale lookups and the other findings; one HIGH remains (F2): which selection an Apply block targets is *inferred* afterwards from composer-state notifications (`note_turns` sees `sent` false and `retried` true when `chat.send` is dispatched synchronously inside one App update), so a fresh send can inherit an earlier turn's anchors and Apply patches the wrong selection. Do not infer it:

- Capture the selection anchors **at send time**, in the send path itself (the `chat.send` handler / `ChatView` send), as an explicit field on the turn being created: the turn owns its anchors (with the buffer identity and generation they were taken under), set exactly once when the turn is created, from the selection chip that is attached to *that* message. Apply reads the turn's own anchors only; nothing is copied from "the current chip" or from the previous turn, and a retry keeps the original turn's anchors and generation.
- Add the tests the finding asks for: 'foo foo' with the first occurrence sent and refused, the second selected, then the same question set in the composer and `chat.send` dispatched synchronously via `dispatch_invocation` in one App update: Apply targets the second occurrence; the same via a keyboard send; and a retry of the first turn still targets the first occurrence.
- Criteria are unchanged; do not widen scope beyond the files already listed.

## Scope widened for round 5 (worker's proposal, accepted)

The send path is `ChatView::send_message` / `retry_turn` in `crates/fe-chat-view/src/view.rs`; the Send button and the Retry banner call them directly, so the panel cannot intercept them and any panel-side reading of composer edits is an inference. `crates/fe-chat-view/src/view.rs` and `crates/fe-chat-view/tests/turn_origin.rs` join the scope: the view records per-turn provenance as it makes the turn (an accessor such as `turn_origin(index) -> Option<TurnOrigin::{Sent, Retry(of)}>`, or an event), and the panel binds the selection chip's anchors at `Sent` and copies the original turn's anchors at `Retry`, never consulting composer edits. Tests: the clear-draft-then-Retry in one App update regression, and a fresh keyboard send after a refusal.

## Implementation notes

- `chat.propose_edit` is registered in a small `fe_command::Registry` in `chat_apply.rs`
  (`register_with_args`, args `ProposeEdit { chat, turn, block }`, the handler handed the
  `&Actor`), not as a `#[kiln::command]` method, which takes no args and can't see the actor.
  `ChatPanel::run_propose_edit` dispatches it there, then calls
  `kiln::platform::commands::notify_observers`; a recorded invocation replays through it too.
  The proposal's origin is the actor's (`Actor::Ai` → `Origin::Ai`).
- `parse_edit_block` returns `Result<EditBlock, EditBlockError>`. Non-blank text between
  sections and a `=======` inside a replacement are `Malformed` (an empty SEARCH is refused
  when applied); a section that
  never closes is `Malformed` at its `<<<<<<< SEARCH` line. `fe_chat::info_path` is exported
  so the toast for a malformed block can name its file.
- A section's hunk is its whole match (`from_edits`), not the lines that differ within it.
- Paths (blocks and links) are located off the main thread and resolved a part at a time:
  every symlink followed (a relative target from the link's folder, a dangling link's target
  too, at most 40), each `..` taken from what's already resolved, a part that isn't there and
  what follows joined on as named (a `..` after one, a loop or an unreadable part refuses).
  The result must be inside the canonical root, for new files too; file, folder, dangling,
  relative-`..`, chained and linked-folder links out of the root are refused. An editor already on the same file by another spelling (its identity, as saves
  resolve it) is what's opened, so its unsaved text is matched.
- Read-only: the view's flag, or the file's permissions (`readonly()` or no `access(W_OK)`)
  for this process. There is no buffer-level read-only state to check.
- The selection a turn is sent with (round 5): the view records how it makes each turn, as
  it makes it (`start_turn` takes the origin): `ChatView::was_sent(index)` for a turn sent
  from the composer (`chat.send` by a key, a command or the Send button) and
  `ChatView::retry_of(index) -> Option<usize>` for a Retry (from a key, a command or the
  banner's button). They're accessors on `ChatView` with a private `TurnOrigin` behind them,
  since `fe-chat-view/src/lib.rs`, which would re-export a public enum, isn't in scope. The
  panel never reads composer edits.
  - The chip's selection is anchored when gathered, stamped with the root/grants generation
    and the exact content block it renders as. The panel reads new turns before every chip
    change (`set_context` calls `note_turns` first) as well as on the view's notifications,
    so a sent turn binds the chip the view held when it was sent; a retry keeps the anchors
    of the turn it resent. Either only if the turn's content holds that block whole and its
    generation is current; anything else binds nothing and Apply refuses, as it does for a
    selection cut to fit the budget. A block with no path and `Sections` matches within
    that range.
  - Tests: `fe-chat-view` `turns_know_how_they_were_made` (key send, a draft set and cleared
    then Retry in one update, a set-and-send in one update, the Send button, the Retry
    banner's button); in the workbench, a send in one App update via `dispatch_invocation`,
    typed and sent as a bound key would be (no key is bound to `chat.send` yet) and by the
    Send button, a fresh keyboard send after a refusal, and a draft cleared then Retry in
    one update, each with the same text selected at another column, plus a retry of the
    first turn targeting the first occurrence.
- Files open at the resolved path that passed the root check (or an editor already on that
  file), not through the link, so a link retargeted after the lookup can't take the read
  elsewhere.
- What's found off the main thread stands only for the chat, root and trust it was asked
  under: a new chat, or the root or its trust changing, drops pending lookups and the Applies
  waiting on reads, and a link found after that opens nothing.
- Each Apply on a file not yet open waits on its own (a list of pending requests, each
  watching its pane); when the read lands in a viewer rather than an editor, a toast says it
  couldn't be opened in an editor. The first hunk is revealed through the pane
  (`Pane::open_at`), so a tab showing its Markdown preview in place shows the text.
