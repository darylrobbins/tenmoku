---
task_uid: "01m7b0mkjpztbczn1r96k61ahs"
task: "T348"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-editor/tests/lane.rs: each row of the transition table (§ Lifecycle › Tests: \"Card 1 tests every row of this table headlessly, with the pending count and the worktree's tree id as inputs\")"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-editor/tests/lane.rs: the restart rules"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-editor/tests/lane.rs: counts (known limit: `Lanes::counts` has no hunks, so it counts every Abandoned lane, including a restored external lane with none pending; `LaneStore::counts` excludes those, T350)"
    required: true
    manual: false
---

# Headless lane model in fe-editor: LaneId, Lane, LaneKind, Backing, LaneState with every lifecycle transition and restart reconciliation, Lanes numbering and claims, LaneCounts (agent lanes card 1) (Mac T1796)

# Headless lane model in fe-editor: LaneId, Lane, LaneKind, Backing, LaneState with every lifecycle transition and restart reconciliation, Lanes numbering and claims, LaneCounts (agent lanes card 1)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **1** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 1. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave A, no dependencies (all ∥):**

1. **Headless lane model.** Size **s**. Deps: none. ∥. Files:
   `crates/fe-editor/src/lane.rs` (new: `LaneId`, `Lane`, `LaneKind`,
   `Backing`, `LaneState` with every transition of § Lifecycle and its
   restart reconciliation, `Lanes` with numbering, claims and `lane_for`,
   `LaneCounts`), `crates/fe-editor/src/lib.rs`. Tests: each row of the
   transition table, the restart rules, counts.

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

The record's § 4 Session lifecycle › Crash: "every session is marked disconnected and its lane Abandoned (agent-lanes § Lifecycle). The next send respawns and `session/load`s with the whole current profile, which reopens the lane." So the `Abandoned → Working` row ("the same agent reopens the same session ... before the lane is cleared") is reached by an ACP `session/load` as well as by `lane.open`: make the model's reopen transport-neutral (a reopen of a session), not tied to `lane.open`. Following the record; the transition table itself is unchanged.

## Design sections it cites, verbatim

### Identity

```rust
// crates/fe-editor/src/lane.rs (new, headless)
pub struct LaneId(NonZeroU32);           // the number: "lane 2"
pub struct Lane {
    pub id: LaneId,
    pub name: Arc<str>,                  // "Chat", "claude-code", the task's title
    pub agent: Arc<str>,                 // Origin::Ai's agent: "fastedit.chat", a plugin id
    pub sessions: Vec<Arc<str>>,         // Origin::Ai sessions it owns
    pub kind: LaneKind,
    pub backing: Backing,                // § Isolation
    pub state: LaneState,
    pub started: SystemTime,
}
pub struct Lanes { /* by id; next number; claims */ }
```

- **The number** is given by the workspace's `Lanes` in order of creation and
  is never reused while the workspace's lane record lasts, so "written by lane
  2" means the same lane in Problems today and in a commit message next week.
  Lane 1 is the chat, made first.
- **The name** is what the agent calls itself (the plugin's display name, the
  CLI's command, the task's title), shown after the number where there is
  room: `lane 2 · claude-code`.
- **Claims.** An edit's `Origin::Ai { agent, session }` finds its lane through
  `Lanes::lane_for(agent, session)`: the lane whose `agent` matches and whose
  `sessions` hold the session. The chat lane adds each chat's id to its
  sessions as that chat proposes, so every chat is lane 1. An origin no lane
  claims (Inline Edit, scripted hosts) has no lane.
- **Provenance carries the session.** `Author::Ai { agent }` becomes
  `Author::Ai { agent, session }`, copied from the `Origin` as it is today
  minus the drop, so a row's lane is `lane_for` of its author. Runs stay
  merged per author, so two chats' lines become two runs; nothing else in
  `Provenance` changes.

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

### § Surfaces, shared data

One entry per row of the provenance table. Each names the data the surface
reads, the type that carries it, and the file that reads it. "Lanes" is the
window's `LaneStore` (`crates/fe-workbench/src/lanes.rs`, new) over the
headless `fe_editor::Lanes`.

Shared data, read by more than one surface:

```rust
// crates/fe-editor/src/lane.rs
pub struct LaneCounts { pub working: usize, pub pending: usize }   // Lanes::counts()
pub struct LaneSummary {                                           // LaneStore::summaries()
    pub id: LaneId, pub name: Arc<str>, pub state: LaneState,
    pub files: Vec<(PathBuf, fe_editor::Counts)>,                  // from the lane's Changeset
    pub conflicts: usize,                                          // stale hunks not yet rejected
}
```

## Scope

- `crates/fe-editor/src/lane.rs`
- `crates/fe-editor/src/lib.rs`
- `crates/fe-editor/tests/lane.rs`: new test file, fe-editor's `tests/` convention for public API (beyond the card's file list).
- `crates/fe-editor/l10n-allow.toml`: fe-editor's literal lint is `all = true`; any literal in the new module needs an entry or `tr!` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion.

1. Test in `crates/fe-editor/tests/lane.rs`: each row of the transition table (§ Lifecycle › Tests: "Card 1 tests every row of this table headlessly, with the pending count and the worktree's tree id as inputs")
2. Test in `crates/fe-editor/tests/lane.rs`: the restart rules
3. Test in `crates/fe-editor/tests/lane.rs`: counts

## Notes

- Headless: no GPUI, no I/O. Persistence (`lanes.json`) is card 7's, so only derive what card 7 will need if it is free (e.g. `Clone`, `Eq`).
- `LaneSummary` (§ Surfaces, shared data) needs `LaneStore` (card 6); this card gives `LaneCounts` and `Lanes::counts()`.
