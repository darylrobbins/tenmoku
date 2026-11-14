---
task_uid: "01m7b0mqb47xckfv4wd2qswf5t"
task: "T350"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in the suites § Today's chat agent names (crates/fe-workbench/src/chat_apply_tests.rs, chat_apply_state_tests.rs, chat_changeset_tests.rs, chat_commands_tests.rs, ai_changes_panel_tests.rs, provenance_column_tests.rs, and crates/fe-editor/tests/changeset.rs, scripted_ai.rs, scripted_ai_followups.rs): the suites in § Today's chat agent pass unedited"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/lanes_tests.rs: lane 1's transition rows"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/lanes_tests.rs: Apply after a finished reply going back to PendingReview"
    required: true
    manual: false
  - id: "C4"
    text: "Test in crates/fe-workbench/src/lanes_tests.rs: an older turn still pending after a newer Review"
    required: true
    manual: false
  - id: "C5"
    text: "`LaneStore::counts` leaves out Abandoned lanes with zero pending hunks; test: a restored Working and a restored PendingReview external lane with none pending count 0"
    required: true
    manual: false
  - id: "C6"
    text: "The rail count reads `LaneCounts::pending` alone (it already includes Working lanes, lane.rs:237); test: one Working lane with pending hunks counts 1, not 2"
    required: true
    manual: false
---

# Chat as lane 1: LaneStore on the Workbench, LaneWork per lane, AI Changes as a view of lane 1 newest turn, Apply proposals adopted, the reply end as the turn end (agent lanes card 6) (Mac T1801)

# Chat as lane 1: LaneStore on the Workbench, LaneWork per lane, AI Changes as a view of lane 1 newest turn, Apply proposals adopted, the reply end as the turn end (agent lanes card 6)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **6** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 6. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave B:**

6. **Chat as lane 1.** Size **m**. Deps: 1. ∥ with 5. Files:
   `crates/fe-workbench/src/lanes.rs` (new: `LaneStore`, an entity on the
   `Workbench`, lane 1 made first; `LaneWork` with each unresolved turn's
   `AiChanges` and loose proposals; `counts`, `summaries`, `written_files`;
   state from turn ends and `ProposalEvent`s),
   `crates/fe-workbench/src/workbench.rs` (the field),
   `ai_changes_panel.rs` (`ai_changes` becomes lane 1's newest turn, the
   panel a view of it; `set_changeset` delegates and keeps the older turn in
   `LaneWork`), `chat_apply.rs` and `chat_changeset.rs` (claim the session,
   `LaneStore::adopt`), `chat_panel.rs` (a reply finishing, failing or
   stopped ends the turn). Tests: the suites in § Today's chat agent pass
   unedited, plus `lanes_tests.rs`: lane 1's transition rows, Apply after a
   finished reply going back to PendingReview, an older turn still pending
   after a newer Review.

### The breakdown's preamble, verbatim

Ordered; each card is at most m (about 2k lines with tests). **∥** marks
cards that can run in parallel with the others in the same wave once their
dependencies have landed. Files are where the card's change is; tests sit
beside them in each crate's `*_tests.rs` convention. Wave C's cards each
register commands or a panel through `workbench.rs` and the command
catalog, so parallel lanes there should expect small rebase conflicts in
those lists, not in each other's files.

## The AI agents record (`docs/decisions/50t8wt9we2-ai-agents.md`, T1779, helium 629e37c1)

**AI agents dependency.** This card is one of the agent-lanes cards the AI agents record (`docs/decisions/50t8wt9we2-ai-agents.md`, T1779, § 10 › Needed first) lists before **AG11** (Mac T1792, the Claude Agent session host): cards 1, 3, 4, 5, 6, 7, 8 and 18. The record makes Claude Agent `LaneKind::External { host: "acp" }` with `Backing::Worktree`, made by the workbench when a Claude Agent chat starts (not through `lane.open`), agent `acp.claude-agent`, name "Claude Agent", its `sessions` holding the ACP session id.

The record's § 10: "The API tool loop needs no lanes card. It is lane 1's work once card 6 lands, and reaches review through `set_changeset` before then." AG10 (T1791) calls `Workbench::set_changeset` at a tool-loop turn end, so keep `set_changeset` the one-line delegate § Today's chat agent says. § 8: Request changes "go through `LaneStore::instruct` once agent-lanes card 6 lands"; `instruct` is not in this card's text, so it is not required here (AG13, T1794, sends straight to the chat panel until a card adds it). No change to the card.

## Design sections it cites, verbatim

## Today's chat agent

Lane 1 is the built-in chat, as the chat works today. The change is where
its state is kept and what it is called, not what it does.

- **Made first.** `LaneStore::new` makes lane 1 before anything else:
  `LaneKind::Chat`, `Backing::Overlay`, agent `fastedit.chat`
  (`chat_apply::AGENT`), name `Chat`, state `Accepted` (nothing pending) until
  the first send makes it `Working`. Its number is fixed at 1 in the lane
  record too, so a restored record never gives 1 to anything else.
- **Every chat is lane 1.** Each `chat.propose_edit` and
  `chat.review_changes` adds its `Actor::Ai` session (the chat id) to lane
  1's sessions before it proposes. Switching chats, starting a new one or
  restoring History doesn't change the lane.
- **What moves, and where to.**

  | Today | Lane 1 |
  | --- | --- |
  | `Workbench::ai_changes: Option<AiChanges>` | Lane 1's newest turn in its `LaneWork`, the same `AiChanges`, which the AI Changes panel shows |
  | `Workbench::set_changeset(changeset, ..)` replacing the old one | `LaneStore::set_changeset(LaneId(1), changeset, ..)`, with `set_changeset` kept as a one-line call to it; the old turn stays in `LaneWork` while it has pending hunks |
  | Apply's proposal, held only by its editor and `Applies` | Also registered as one of lane 1's loose proposals (`LaneStore::adopt(lane, buffer, proposal)`), so it counts as pending |
  | The chat reply finishing, failing or stopped | The turn's end: lane 1 to `PendingReview` if it has pending hunks |
  | Apply or Review Changes on a finished reply | Hunks added outside a turn: lane 1 back to `PendingReview` |
  | `ProposalEvent`s the AI Changes panel follows | The same, also recounting the lane's state |

- **Behaviour that doesn't change** (the lane-1 card's test is that these
  suites pass unedited): `chat_apply_tests.rs`, `chat_apply_state_tests.rs`,
  `chat_changeset_tests.rs`, `chat_commands_tests.rs`,
  `ai_changes_panel_tests.rs` (including the refactor preview, which shows a
  language server's edit in the same panel and is not a lane),
  `provenance_column_tests.rs`, and fe-editor's `changeset.rs` and
  `scripted_ai*.rs` tests. In particular:
  - Apply still opens the file in the active pane and reveals the first
    hunk; block headers still follow `ProposalEvent`s.
  - The AI Changes panel still shows one changeset, lane 1's, with the same
    header text, rows, buttons, commands and `No pending changes`. Showing
    other lanes there is the review panel's
    (`docs/designs/agent-review.md`), not lane 1's.
  - Accept still lands as `Origin::User` with the text authored by the
    agent; undo still reverts it.
  - The provenance column's labels stay `AI · <agent>` and `Written by
    <agent>`. The `session` added to `Author::Ai` is not shown by it; the
    card that adds the lane to its label (`lane 1 · Chat`) changes those
    tests on purpose and says so.
- **Inline Edit** stays outside lane 1 (§ Lane model › Who creates a lane).
  Its proposals keep `fastedit.inline_edit` and no lane claims it.

### What a lane owns

Undecided work belongs to the lane, not to whatever shows it, so no panel,
tab or newer turn can drop it.

- **`LaneWork`**, one per lane in the `LaneStore`: every proposal the lane
  has made that still has undecided hunks, by file, each an
  `Rc<RefCell<ProposedChange>>` (the same one editors and changesets hold),
  whether in a turn's changeset or **loose** (Apply's, made outside one);
  every new file and deletion it proposes, with its precondition (§ New and
  deleted files); the detached hunks of files no buffer holds now
  (§ Detached hunks); and every turn's `AiChanges` whose changeset isn't
  resolved, in turn order. A proposal leaves `LaneWork` when its last hunk
  is decided; a turn's `AiChanges` when its changeset is resolved, except
  the newest, which stays for Revert Turn as today.
- **The AI Changes panel is a view of the newest turn**, as today: a lane's
  newer turn takes the panel, as a newer Review does now, and the earlier
  turn's `AiChanges` stays in `LaneWork`, its pending new files with it
  (text in its `Changeset`; a new file gets a buffer only when accepted,
  and a tab at once, as `land_ai_changes` does today), still pending and
  still counted.
- **One user buffer per path.** `LaneBuffers` (in the owner's `LaneStore`)
  maps each canonical real path (resolved as `chat_apply::locate` resolves
  a block's file) to the one `Buffer` every lane and every editor uses for
  it. A lane that needs a file that is open takes the editor's buffer; one
  that isn't open is read into a buffer kept there with no editor; an
  editor opened on the path later (`open.rs`) takes that buffer and every
  lane's proposals on it. So two lanes editing one unopened file propose
  against one `BufferId`, and accepting one's hunk stales the other's by
  the existing rule. A background lane never opens a tab (accepting, the
  user's act, does: below); today's Apply,
  which opens the file in the active pane, still does for lane 1.
- **An editorless buffer is always clean**, so no unsaved text ever lives
  where hot exit and the close prompt can't see it:
  - A buffer a lane reads with no editor is the file's text from disk, and
    follows disk changes as a clean open buffer does (`disk_sync.rs`), its
    hunks going stale by the usual rules.
  - **Accepting** a hunk, file or turn on a buffer with no editor first opens
    the file in a tab in the active pane, unfocused, so the dirty buffer has
    an editor from the moment it is dirty, and saving, hot exit and the
    close prompt work as today. Rejecting doesn't dirty, so opens nothing.
  - **The last editor closing** runs the close prompt as today. Then the
    lane's undecided hunks on the buffer are detached (saved as § Detached
    hunks says, with the buffer's text as their base) and the buffer goes as
    today. Opening the file again, or a lane needing it, restores them onto
    the text then (what the user saved, or the disk's if they discarded),
    so hunks over discarded edits come back stale.
  - Chat's block header still clears when its file's editor closes, as
    today; the lane keeps the hunks, and opening the file shows them again.
- **Nothing saved.** A pending hunk isn't in the buffer, so saving, the
  language server, auto-save, `git status` and commit never see it.

### Lifecycle

```rust
pub enum LaneState {
    Working,        // the agent is producing; it may already have pending hunks
    PendingReview,  // its turn is over and it has undecided hunks
    Accepted,       // every hunk decided, at least one accepted
    Rejected,       // every hunk decided, none accepted
    Abandoned,      // ended (closed, disconnected, exited) with hunks undecided
}
```

A lane's **pending hunks** are every undecided hunk it owns, wherever it is
shown or not (§ Isolation › What a lane owns): `Pending` hunks, and `Stale`
ones until the user rejects them (`ProposedChange::reject` takes a stale
hunk). A turn's **end** is the agent's signal for an external lane
(`lane.finish`), the reply finishing, failing or being stopped for the chat,
and a frozen revision for a worktree lane (§ Isolation › Worktree lanes),
whose next turn starts when its worktree changes again; a terminal still
running with nothing written leaves the lane as it is.

| From | To | On |
| --- | --- | --- |
| (new) | Working | `lane.open`, New agent terminal, the chat's first send |
| Working | PendingReview | The turn ends with pending hunks |
| Working | Accepted / Rejected | The turn ends with none pending (Rejected also when it made nothing) |
| PendingReview | Working | Request changes or a review comment sent as the next instruction; the chat's next send; the agent starting a turn by itself |
| PendingReview | Accepted / Rejected | The last pending hunk decided |
| Accepted / Rejected | PendingReview | Hunks added outside a turn: Apply or Review Changes on a finished reply, a worktree lane frozen again |
| Accepted / Rejected | Working | A persistent lane (the chat) or a reconnected agent starts another turn |
| PendingReview / Accepted / Rejected | Working | A worktree lane's live tree no longer equals its newest revision's: the agent wrote again (checked on the worktree watcher's debounced events, by tree id) |
| Working / PendingReview | Abandoned | Closed by the user, the agent disconnects, or its terminal exits, with hunks pending |
| Working / PendingReview | Accepted / Rejected | The same, with none pending |
| Abandoned | Accepted / Rejected | Its last pending hunk decided: an ended lane's hunks stay decidable |
| Abandoned | Working | The same agent reopens the same session (`lane.open` with it) before the lane is cleared |

- **Restart.** A restored lane is reconciled before anything reads it:
  lane 1 is PendingReview with pending hunks, else Accepted or Rejected; an
  external lane restored as Working or PendingReview is Abandoned (its agent
  is gone) until `lane.open` reopens its session; a worktree lane's terminal
  isn't restored, so it is frozen once (its worktree is still there) and
  goes to PendingReview or stays as decided.
- **Closing.** An Accepted, Rejected or Abandoned lane stays listed in the
  Agents panel, dimmed, until the user clears it; clearing an Abandoned
  lane with hunks pending asks first and rejects them. Clearing removes its
  worktree (if any) but not its number from the record. Lane 1 never closes.
- **Counts.** "Agents working" in the title bar counts lanes in `Working`;
  the rail's count is lanes with pending hunks (PendingReview, and Abandoned
  ones not yet decided) plus those `Working`.
- **Tests.** Card 1 tests every row of this table headlessly, with the
  pending count and the worktree's tree id as inputs; card 6 tests lane 1's
  rows through the chat; card 19 tests a terminal lane through review,
  decisions and the agent writing again (Working, PendingReview, Accepted,
  Working, PendingReview).

## Scope

- `crates/fe-workbench/src/lanes.rs`
- `crates/fe-workbench/src/lanes_tests.rs`: the card's `lanes_tests.rs`, declared from `lanes.rs` (beyond the card's file list).
- `crates/fe-workbench/src/workbench.rs`
- `crates/fe-workbench/src/ai_changes_panel.rs`
- `crates/fe-workbench/src/chat_apply.rs`
- `crates/fe-workbench/src/chat_changeset.rs`
- `crates/fe-workbench/src/chat_panel.rs`
- `crates/fe-workbench/src/lib.rs`: declares the new `lanes` module (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (lane 1's agent id and name literals) (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion.

1. Test in `the suites § Today's chat agent names (crates/fe-workbench/src/chat_apply_tests.rs, chat_apply_state_tests.rs, chat_changeset_tests.rs, chat_commands_tests.rs, ai_changes_panel_tests.rs, provenance_column_tests.rs, and crates/fe-editor/tests/changeset.rs, scripted_ai.rs, scripted_ai_followups.rs)`: the suites in § Today's chat agent pass unedited
2. Test in `crates/fe-workbench/src/lanes_tests.rs`: lane 1's transition rows
3. Test in `crates/fe-workbench/src/lanes_tests.rs`: Apply after a finished reply going back to PendingReview
4. Test in `crates/fe-workbench/src/lanes_tests.rs`: an older turn still pending after a newer Review

## Notes

- Ready tasks touching the same files: T1794 (AG13: `ai_changes_panel.rs`, `chat_panel.rs`) and T1793 (AG12: `chat_panel.rs`); expect rebase conflicts, not overlap in behaviour.

## Implementation notes

- **Where lane 1's newest turn lives.** The suites must pass unedited, and
  they read `workbench.ai_changes` as a field (`.as_ref()`, `.is_some()`).
  So the field stays, documented as the panel's view of lane 1's newest
  turn (or a refactor preview), and `AiChanges` gains `lane: Option<LaneId>`.
  `LaneWork` holds the older turns still undecided and the loose
  proposals. `LaneStore::set_changeset(workbench, lane, changeset, ..)` is
  an associated fn taking the workbench, and `Workbench::set_changeset` is
  the one-line call to it. Another lane's turn goes to its `LaneWork`, not
  the panel.
- **Counting.** `Workbench::sync_lanes` counts each lane's hunks (pending
  plus stale; kept turns, then the shown turn, then loose proposals) over
  the open editors' buffers, prunes resolved turns and decided proposals,
  and applies `Decided`, or `HunksAdded` when a decided lane has hunks
  pending again (Apply or Review on a finished reply, or a revert). It runs
  after every AI Changes listing (`sync_ai_changes` now lists, then syncs
  the lanes), on every open editor's `ProposalEvent`, and on edits while a
  lane holds work. `Hunks::any_accepted` is the accepted hunks still held
  (so a Revert Turn, which makes them pending again, takes them back), or
  any that left the work settled, with their decided proposal or resolved
  turn, since a turn last started with nothing pending (a turn reviewed
  again settles its accepted hunks before it's dropped). A settled file
  stays in `written_files` after its work is pruned. `recount` reports
  whether anything a reader sees changed (state, pending count, files and
  their counts, conflicts, files written), and the store notifies its
  observers then, so a hunk going stale is told too. `summaries` and
  `written_files` read the last count. `LaneSummary` lives in `lanes.rs`,
  since fe-editor isn't in scope.
- **Turns.** The chat panel starts lane 1's turn when its view starts
  streaming (observed) and ends it on `ChatViewEvent::TurnFinished`, or
  when a new view replaces one mid-reply. Both are deferred, because the
  workbench may be in an update. A turn that made nothing ends Rejected, as
  the table says. A turn ending while the shown turn still has accepted
  hunks (kept for Revert Turn) ends Accepted.
- **Restore.** `LaneStore::restore(record, next)` always makes lane 1 the
  chat: a record lane 1 of another kind is dropped. Lanes are reconciled
  with nothing pending (no work exists yet), and a lane saved as Accepted
  counts as having one accepted. C5 and C6 test `counts` and `rail_count`
  on the store directly.
- **Beyond the scope list.** `refactor_preview_view.rs` has two one-line
  changes. A preview taking the panel calls `keep_shown_turn` first, so it
  can't drop lane 1's turn. `discard_ai_reviews` goes from `pub(super)` to
  `pub(crate)`, for `LaneStore::set_changeset`. In `chat_panel.rs`,
  `chat_store_worker` is now `pub(crate)` so `lanes_tests.rs` can run the
  store's pending saves as the chat suites do.
