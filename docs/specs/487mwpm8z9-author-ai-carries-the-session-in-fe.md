---
task_uid: "01m7b0mkqyx57sde487mwpm8z9"
task: "T349"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-buffer/tests/provenance.rs: (own; the design gives no Tests line for this card) two `Origin::Ai` edits with one agent and two sessions give `Author::Ai { agent, session }` authors that differ by session and two runs, while one session's adjacent edits still merge into one run"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/provenance_column_tests.rs: labels unchanged, `provenance_column_tests.rs` passes unedited (verbatim from the card's file list)"
    required: true
    manual: false
---

# Author::Ai carries the session in fe-buffer provenance, so two chats rows are two runs (agent lanes card 2) (Mac T1797)

# Author::Ai carries the session in fe-buffer provenance, so two chats rows are two runs (agent lanes card 2)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **2** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 2. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave A, no dependencies (all ∥):**

2. **`Author::Ai` carries the session.** Size **s**. Deps: none. ∥. Files:
   `crates/fe-buffer/src/provenance.rs` (`Author::Ai { agent, session }`,
   `From<&Origin>`), `crates/fe-buffer/tests/provenance.rs`,
   `crates/fe-workbench/src/provenance_column.rs` (the match arms only;
   labels unchanged, `provenance_column_tests.rs` passes unedited).

### The breakdown's preamble, verbatim

Ordered; each card is at most m (about 2k lines with tests). **∥** marks
cards that can run in parallel with the others in the same wave once their
dependencies have landed. Files are where the card's change is; tests sit
beside them in each crate's `*_tests.rs` convention. Wave C's cards each
register commands or a panel through `workbench.rs` and the command
catalog, so parallel lanes there should expect small rebase conflicts in
those lists, not in each other's files.

## Since the design (Mac main at fe5d87fd7)

- Since the design, T1549 made the provenance column leave agent rows blank (they draw as the change bar's dotted agent lane): `provenance_column.rs:178` is `Author::Ai { .. } => return None`, so the column's arm may need no edit at all. The column's tests now assert the dotted lane (`agent_rows`), not `AI · <agent>` labels; they must still pass unedited.
- `Author::Ai` is matched or built only in `crates/fe-buffer/src/provenance.rs`, `crates/fe-buffer/tests/provenance.rs` and `crates/fe-workbench/src/provenance_column.rs` on main (rg, fe5d87fd7).

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

## Scope

- `crates/fe-buffer/src/provenance.rs`
- `crates/fe-buffer/tests/provenance.rs`
- `crates/fe-workbench/src/provenance_column.rs`

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-buffer/tests/provenance.rs`: (own; the design gives no Tests line for this card) two `Origin::Ai` edits with one agent and two sessions give `Author::Ai { agent, session }` authors that differ by session and two runs, while one session's adjacent edits still merge into one run
2. Test in `crates/fe-workbench/src/provenance_column_tests.rs`: labels unchanged, `provenance_column_tests.rs` passes unedited (verbatim from the card's file list)
