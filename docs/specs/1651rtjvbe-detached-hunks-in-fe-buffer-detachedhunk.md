---
task_uid: "01m7b0n2q8ehv3qj1651rtjvbe"
task: "T373"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-buffer/src/tests/detached_hunks.rs: a sticky stale hunk can be rejected, never accepted, and survives `refresh`"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-buffer/src/tests/detached_hunks.rs: a mismatched placement is refused"
    required: true
    manual: false
---

# Detached hunks in fe-buffer: DetachedHunk and ProposedChange::from_detached, placed hunks checked against their old text, unplaced or stale ones stale for good (agent lanes card 3) (Mac T1798)

# Detached hunks in fe-buffer: DetachedHunk and ProposedChange::from_detached, placed hunks checked against their old text, unplaced or stale ones stale for good (agent lanes card 3)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **3** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 3. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave A, no dependencies (all ∥):**

3. **Detached hunks.** Size **s**. Deps: none. ∥. Files:
   `crates/fe-buffer/src/proposed.rs` (`DetachedHunk`,
   `ProposedChange::from_detached`: placed hunks checked against their old
   text and refused on a mismatch, unplaced or stale ones stale for good),
   `crates/fe-buffer/src/tests/` (a sticky stale hunk can be rejected,
   never accepted, and survives `refresh`; a mismatched placement is
   refused).

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

## Design sections it cites, verbatim

### Detached hunks: persisting and restoring without guessing

A restart, or the last editor on a file closing (§ What a lane owns),
needs an overlay lane's undecided work to outlive its anchors. It is never
found again by searching for its old text alone, nor by trusting whatever
alignment a diff happened to choose: an equal text elsewhere is not the same
place. Only undecided work is saved; the owner rewrites the file after each
proposal and decision.

- **What is saved**, in `<lane>.patch.json`, per file:
  - for hunks: the file's whole **base text** (the buffer's text the hunks'
    ranges are in: when the proposal was made, or when it was detached),
    each hunk's range in that base, its old and new text, and its status,
    `Stale` kept as stale;
  - for whole-file operations (§ New and deleted files): the operation
    (`New` or `Deleted`), a new file's whole text, a deletion's expected
    text, and whether it is already stale.
- **Restoring hunks** is the projection with the saved base as base, the
  base with the saved hunks applied as the lane's text, and the text as it
  is now (the open buffer, or the file on disk) as current, under the
  placement rule below; a hunk saved stale stays stale.
- **Restoring whole-file operations** checks each precondition again
  against the text as it is now: a new file whose path now exists, or a
  deletion whose expected text no longer equals the file's (or whose file is
  gone, which decides it as no change), comes back stale or decided as
  § New and deleted files says; one saved stale stays stale.
- **The placement rule**, used by restore and by every projection
  (`lane_project`): a hunk's **window** is the full lines its base range
  touches plus one whole line on each side (or the start or end of the
  text); for an insertion, the line it is in plus one on each side. If
  current equals base, every hunk is placed at its base range (there is no
  alignment to choose). Otherwise a hunk is placed only if both hold:
  1. its window lies inside a single unchanged run of the base→current
     diff and touches no changed region (the start or end of the text is
     not a change); and
  2. the window's text occurs exactly once in base and exactly once in
     current, at the position the diff maps it to.
  Otherwise it is stale. So with base `x\nx\n` and current `x\n`, a hunk on
  either `x` is stale (neither window is in current), not moved onto the
  survivor; text the user moved is a deletion and an insertion in the diff,
  so a hunk on it is stale; repeated text with the same neighbours is never
  unique, so is stale whenever anything changed around it.
- **`ProposedChange` can't express this today**: its constructors read old
  text from the snapshot they are given and have no stale hunk. Card 3 adds
  `ProposedChange::from_detached(current, hunks, origin)`, taking per hunk
  `DetachedHunk { at: Option<Range<usize>>, old_text, new_text, stale:
  bool }`: `at: None` or `stale: true` makes a hunk that is `Stale` for good
  (shown, rejectable, never acceptable), and a placed hunk whose `old_text`
  doesn't match the text at `at` is refused, not silently re-anchored.
- **Insertions** have no old text to mismatch, so only the placement rule
  decides them: an insertion beside or inside anything the user changed is
  stale, as is one whose surrounding lines aren't unique.

## Scope

- `crates/fe-buffer/src/proposed.rs`
- `crates/fe-buffer/src/tests/detached_hunks.rs`: new test file in the card's `crates/fe-buffer/src/tests/` (beyond the card's file list).
- `crates/fe-buffer/src/tests/mod.rs`: declares the new test file (beyond the card's file list).
- `crates/fe-buffer/l10n-allow.toml`: fe-buffer's literal lint is `all = true` (a refusal's error text, if any) (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion.

1. Test in `crates/fe-buffer/src/tests/detached_hunks.rs`: a sticky stale hunk can be rejected, never accepted, and survives `refresh`
2. Test in `crates/fe-buffer/src/tests/detached_hunks.rs`: a mismatched placement is refused
