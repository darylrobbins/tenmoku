---
task_uid: "01m7b0n6thz02axmn2txwec6zc"
task: "T375"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-editor/tests/lane_project.rs: an unchanged current places every hunk, repeated lines included"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-editor/tests/lane_project.rs: base `x\\nx\\n` to current `x\\n` leaves a hunk on either line stale"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-editor/tests/lane_project.rs: a target deleted while an identical line survives elsewhere is stale"
    required: true
    manual: false
  - id: "C4"
    text: "Test in crates/fe-editor/tests/lane_project.rs: moved text is stale"
    required: true
    manual: false
  - id: "C5"
    text: "Test in crates/fe-editor/tests/lane_project.rs: insertions beside an edit are stale"
    required: true
    manual: false
  - id: "C6"
    text: "Test in crates/fe-editor/tests/lane_project.rs: a property test that a placed hunk's window is unique in both texts and lands at the diff's image"
    required: true
    manual: false
---

# Three-way projection in fe-editor: lane_project(base, lane, current, origin) through from_detached, the unique-window placement rule, and new and deleted file preconditions (agent lanes card 5) (Mac T1800)

# Three-way projection in fe-editor: lane_project(base, lane, current, origin) through from_detached, the unique-window placement rule, and new and deleted file preconditions (agent lanes card 5)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **5** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 5. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave B:**

5. **Three-way projection.** Size **m**. Deps: 3. ∥ with 6. Files:
   `crates/fe-editor/src/lane_project.rs` (new: `lane_project(base, lane,
   current, origin) -> ProposedChange` through `from_detached`: hunks
   placed by § Detached hunks' placement rule, everything else stale; the
   whole-file preconditions of § New and deleted files as
   `FilePrecondition`). Tests: an unchanged current places every hunk,
   repeated lines included; base `x\nx\n` to current `x\n` leaves a
   hunk on either line stale; a target deleted while an identical line
   survives elsewhere is stale; moved text is stale; insertions beside an
   edit are stale; a property test that a placed hunk's window is unique in
   both texts and lands at the diff's image.

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

### New and deleted files

A whole-file change has no hunk to go stale, so each carries a precondition
checked when it is proposed and again when it is accepted.

- **New file**: the path must not exist, on disk or as a buffer. If it does
  at accept (the user made it meanwhile: add/add), the change is stale; the
  lane diff shows the user's file beside the lane's, and the user can copy
  from it. `Changeset::add_new_file` gains this check for every lane; today
  accepting over a path the user has since made makes a second buffer.
- **Deleted file**: the user's current text must equal the text the
  deletion was made against (the revision before, for a worktree lane). If
  the user edited it (delete/modify), the deletion is stale. If the user
  already deleted it, accepting changes nothing (the `AcceptedNoChange` of a
  hunk) and the deletion is decided.
- **Renames** are a deletion and a new file, each with its precondition.

## Scope

- `crates/fe-editor/src/lane_project.rs`
- `crates/fe-editor/src/lib.rs`: declares the new module (beyond the card's file list).
- `crates/fe-editor/tests/lane_project.rs`: new test file, fe-editor's `tests/` convention (beyond the card's file list).
- `crates/fe-editor/l10n-allow.toml`: fe-editor's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion.

1. Test in `crates/fe-editor/tests/lane_project.rs`: an unchanged current places every hunk, repeated lines included
2. Test in `crates/fe-editor/tests/lane_project.rs`: base `x\nx\n` to current `x\n` leaves a hunk on either line stale
3. Test in `crates/fe-editor/tests/lane_project.rs`: a target deleted while an identical line survives elsewhere is stale
4. Test in `crates/fe-editor/tests/lane_project.rs`: moved text is stale
5. Test in `crates/fe-editor/tests/lane_project.rs`: insertions beside an edit are stale
6. Test in `crates/fe-editor/tests/lane_project.rs`: a property test that a placed hunk's window is unique in both texts and lands at the diff's image

## Notes

- fe-editor already has `pub mod text_diff` and `imara-diff`; `proptest` is already a dev-dependency, so no Cargo change.
- imara-diff's Histogram row diff is quadratic on many interleaved changes: keep the property test's texts small (a few hundred lines at most) so the gate stays fast.

## Implementation notes

- `fe_editor::lane_project(base, lane, current, origin)` diffs `base`→`lane` by rows (`TextDiff`) and restores the hunks through `ProposedChange::from_detached`; `lane_project::project` gives the same hunks with their base ranges, and `lane_project::Placement` places any byte range of a base against a current (for card 7's restore of saved hunks). Unplaced hunks are `at: None`, so stale for good, per `DetachedHunk`'s contract.
- "Touches no changed region" is read conservatively: a base→current change starting right after the window, or ending right before it (an insertion at either edge included), touches it.
- "Occurs exactly once" counts every byte offset, overlaps included. It is exact but indexed: an occurrence lines up the window's line breaks with the text's, so its inner rows are whole rows, found from the rarest of them through a per-text row index, stopping at the second match; windows with no inner row (two rows at most, only at a text's end or in a tiny text) are searched for directly.
- A hunk reaching the end of the text takes the line break before it, so a last row without one is replaced right.
- `base` and `lane` are read with `\r\n`/`\r` as `\n`, as a buffer holds its text. `FilePrecondition` (`Absent` for a new file, `Text` for a deletion) checks against the file now: `Holds`, `Stale` (add/add, delete/modify), or `AlreadyDone` (already deleted); deletion texts compare line endings aside.
- `origin` is passed through opaquely; tests use `Origin::Host`.
- No `l10n-allow.toml` change: the module's only literals are line breaks.
