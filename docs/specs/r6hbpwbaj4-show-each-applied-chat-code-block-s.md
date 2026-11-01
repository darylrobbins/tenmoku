---
task_uid: "01m69pbar0e7rph2r6hbpwbaj4"
task: "T273"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "After Apply proposes a block, its state follows the editor's ProposalEvents through the proposal's hunk counts. It is Proposed { pending, hunks } while any hunk is pending, then Decided { accepted, hunks }, shown on the header as chat-block-state-\u003cturn\u003e-\u003cblock\u003e. chat_panel::chat_apply::chat_apply_state_tests::an_applied_block_follows_its_hunks passes."
    required: true
    manual: false
  - id: "C2"
    text: "Every Apply refusal (bad markers, a path outside the root, no file named, a file read-only, binary or unopenable, a SEARCH found nowhere or twice, overlapping sections, a lost selection) sets Failed with the toast's reason, shown in a row under the header (chat-block-error-\u003cturn\u003e-\u003cblock\u003e). A block that changes nothing sets NoChange. The toasts are unchanged. chat_panel::chat_apply::chat_apply_state_tests::a_block_that_cannot_apply_says_why passes."
    required: true
    manual: false
  - id: "C3"
    text: "Applying the same block again replaces its state, closing the file's editor with hunks pending clears it, and a new chat shows none. chat_panel::chat_apply::chat_apply_state_tests::applying_again_or_closing_the_file_resets_the_state passes."
    required: true
    manual: false
  - id: "C4"
    text: "The labels (Proposed, Proposed · \u003cn\u003e left, Accepted, Rejected, Accepted \u003ca\u003e of \u003ch\u003e, No changes, Not applied plus the reason) are laid out by block_states_label_the_header (crates/fe-chat-view/tests/block_state.rs). chat_panel.rs is untouched. The existing chat_apply_tests and chat_changeset_tests pass. cargo test -p fe-chat-view -p fe-workbench and clippy are clean."
    required: true
    manual: false
---

# Show each applied chat code block's outcome on its header: proposed with hunks left, accepted, rejected, partly accepted, no changes, or not applied with the reason that today only goes to a toast (Mac T1263)

## Goal

After Apply, a chat code block's header shows what came of it, and keeps showing it as the user
decides the proposal in the editor:

- proposed (hunks still to review)
- accepted
- rejected
- partly accepted
- no changes
- not applied, with the reason

Today the reason a block couldn't be applied goes only to a toast, and nothing in the chat
shows whether a proposal was taken. The toasts stay as they are.

## Facts found

- `crates/fe-workbench/src/chat_apply.rs:594-655` `propose_edit` runs the paths that can fail:
  a `Path` or `Malformed` block (`:616-625`), "This block doesn't say which file it's for"
  (`:638-641`, a warning toast), and a selection cut to fit (`:642-645`). The async paths
  `when_located` (`:713-736`), `open_to_apply` (`:740-765`) and `file_read` (`:770-800`) end in
  `refused(file, why, cx)` (`:481-487`, an error toast) or in `show_proposal` (`:854-911`).
  `show_proposal` is a free function that can refuse (`:867-884`), toast "The block changes
  nothing in …" (`:885-892`), or push the new `Rc<RefCell<ProposedChange>>` onto the editor's
  proposals (`:893-897`). The `(turn, block)` asked for is in `ProposeEdit` (`:106-110`) but is
  not carried in `Edit` (`:188-194`).
- `Applies` (`:157-175`) is the panel's Apply state. `new_chat` (`:211-215`) and `stale`
  (`:219-222`) reset it. A new chat makes a new `ChatView` (`chat_panel.rs:1374-1376`), so
  view-side state needs no reset.
- How proposals report back: `EditorView` emits `fe_gpui::ProposalEvent::{Accepted,
  AcceptedNoChange, Rejected}` on each hunk decision (`crates/fe-gpui/src/view/proposal.rs:22-37`,
  emitted at `:140-144`). `fe_buffer::ProposedChange::status_count(HunkStatus)` and
  `hunk_count()` need no snapshot (`crates/fe-buffer/src/proposed.rs:416-430`). `HunkStatus` is
  `Pending | Accepted | Rejected | Stale` (`:30-39`). Undoing an accepted hunk leaves it accepted
  (`proposal.rs:7-11`); only the AI Changes changeset calls `reset_to_pending`
  (`crates/fe-editor/src/changeset.rs:827`).
- Precedent for subscribing: `crates/fe-workbench/src/ai_changes_panel.rs:572-590` subscribes to
  each editor's `ProposalEvent` and re-lists the changeset's counts.
- Each proposal has `Origin::Ai { agent: "fastedit.chat", session: <chat id> }`
  (`chat_apply.rs:226-237`).
- Review Changes (`chat_changeset.rs`) proposes a turn's blocks through a separate changeset and
  removes its proposals again on a re-review (`:444-470`).

## Changes

**fe-chat-view** (`src/view.rs`, `src/reply.rs`, `src/lib.rs`):
- Add an exported state enum:
  ```rust
  pub enum BlockState {
      Proposed { pending: usize, hunks: usize },
      Decided { accepted: usize, hunks: usize },
      NoChange,
      Failed(String),
  }
  ```
- `ChatView::set_block_state(turn, block, Option<BlockState>, cx)` stores or clears it and
  notifies. `ChatView::block_state(turn, block) -> Option<&BlockState>` reads it.
- `code_header` draws the state as a label between the path and Copy, with debug selector
  `chat-block-state-<turn>-<block>`:

  | State | Label |
  |---|---|
  | `Proposed`, none decided | "Proposed" |
  | `Proposed`, some decided | "Proposed · <pending> left" |
  | `Decided`, all accepted | "Accepted" |
  | `Decided`, none accepted | "Rejected" |
  | `Decided`, mixed | "Accepted <a> of <h>" |
  | `NoChange` | "No changes" |
  | `Failed` | "Not applied" |

  `Failed` labels are in `UiTheme::error_text`, the rest muted. A `Failed` block also gets a
  wrapping row under the header with the reason (`chat-block-error-<turn>-<block>`). The pinned
  header copy shows the same label. Apply stays enabled whatever the state: pressing it again
  proposes again.

**fe-workbench** (`src/chat_apply.rs` only; not `chat_panel.rs`, which T1258 holds):
- `Edit` carries its `(turn, block)`.
- Every refusal on the Apply path also sets `Failed(<the toast's why>)` on the view: the
  `refused` calls in `propose_edit`, `when_located`, `open_to_apply` and `file_read`, and "This
  block doesn't say which file it's for".
- `show_proposal` returns its outcome (the proposal, no change, or why not) instead of only
  toasting, so its callers (all `ChatPanel` methods) record it. Its toasts stay.
- On a proposal, `Applies.applied: HashMap<(usize, usize), Applied>` keeps:
  - the `Rc<RefCell<ProposedChange>>`
  - a subscription to that editor's `ProposalEvent`s
  - an `observe_release` on the editor

  On each event, the state is recomputed from `status_count`: pending is everything not
  accepted or rejected, stale included. It is pushed with `set_block_state` as `Proposed` while
  any hunk is pending, else `Decided`.
- Applying the same block again replaces its entry. If the editor is released with hunks still
  pending, the state is cleared (`None`). `Applies::new_chat` drops all entries.
- Out of scope: Review Changes' changeset proposals don't feed block states, and states aren't
  saved with the chat (a reopened chat shows none).

## Tests

- New `crates/fe-workbench/src/chat_apply_state_tests.rs`, declared from `chat_apply.rs` as
  `#[cfg(test)] #[path = "chat_apply_state_tests.rs"] mod chat_apply_state_tests;` (as
  `chat_changeset.rs:474-476` declares its tests; self-contained helpers like
  `chat_changeset_tests.rs`):
  - `an_applied_block_follows_its_hunks`: Apply a two-section block on `src/lib.go` gives
    `Proposed { pending: 2, hunks: 2 }` and `chat-block-state-0-0` is drawn. `accept_hunk` on
    one gives `Proposed { pending: 1, hunks: 2 }`. `reject_hunk` on the other gives
    `Decided { accepted: 1, hunks: 2 }`. Another block with all hunks rejected gives
    `Decided { accepted: 0, .. }`.
  - `a_block_that_cannot_apply_says_why`: a SEARCH not in the file gives
    `Failed("SEARCH 1 (\"…\") isn't in it")`, and the toast is still shown. A path outside the
    root and a no-path block with no selection also give `Failed`. A block that changes nothing
    gives `NoChange`.
  - `applying_again_or_closing_the_file_resets_the_state`: Apply again after deciding gives
    `Proposed` again. Closing the file's editor with hunks pending gives `None`. A new chat's
    view has no states.
- New `crates/fe-chat-view/tests/block_state.rs` (recording text system as
  `tests/markdown.rs`): `block_states_label_the_header`. `set_block_state` with each variant
  lays out the label text above. `Failed` draws `chat-block-error-0-0` and lays out its reason.
  `None` draws no label.
- Existing `chat_apply_tests` and `chat_changeset_tests` pass unchanged.

## Conflicts

- Depends on the SEARCH/REPLACE-diff card (same `code_header` in `reply.rs`; `view.rs`), and
  through it on the pinned-header card, whose pinned copy must show the label.
- `chat_apply.rs` is in no other open task. T1258 (ready) holds `chat_panel.rs` and
  `chat_panel_tests.rs`, which this card leaves alone.
- T1235 (blocked) holds `crates/fe-chat-view/tests/**`.

## Implementation notes

- `BlockState` lives in `reply.rs` beside `CodeBlock`, with `BlockState::label()` giving the
  header's text; `reply_element` takes the turn's states by block, and the pinned copy reads the
  same map, so it shows the same label (no error row when pinned).
- In `chat_apply.rs`, `ChatPanel::refuse` toasts and sets `Failed`; `ChatPanel::show` wraps
  `show_proposal`, which now returns `Shown::{Proposed, NoChange, Refused}`, and starts following
  a proposal. `set_state` drops the block's `Applied` entry first, so a later outcome, a failure
  included, stops the earlier proposal's events from overwriting it.
