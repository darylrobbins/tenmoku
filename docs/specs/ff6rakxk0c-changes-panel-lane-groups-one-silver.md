---
task_uid: "01m7b0mr1h7rdq7rff6rakxk0c"
task: "T354"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/git_panel_groups_tests.rs: (own; the design gives no Tests line) under the user's groups, one group per lane with pending work, headed `lane 2 · \u003cname\u003e` with `Review` at its right; its rows are the lane's files with `+a −r` and the pending count, loose proposals included. Documented limit: `+a −r` is read from the open editors' proposals and the shown turn's pending new files, kept as last read (also for a file closed before the panel's first draw); totals over every lane-owned turn (an unshown non-chat turn's new files, several pending turns at one new-file path) need LaneSummary to carry them, follow-up T369"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/git_panel_groups_tests.rs: (own) lane rows offer no Stage, Unstage or Discard, by button or key; `Action::ReviewLane` dispatches `agent.review_lane` for its lane"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/git_panel_groups_tests.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Changes panel lane groups: one silver-blue group per lane with pending work, file rows with counts and no staging actions, and a Review link (agent lanes card 14) (Mac T1809)

# Changes panel lane groups: one silver-blue group per lane with pending work, file rows with counts and no staging actions, and a Review link (agent lanes card 14)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **14** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 14. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

14. **Changes: lane groups.** Size **m**. Deps: 0, 6. ∥. Files:
    `crates/fe-workbench/src/git_panel.rs` (`Group::Lane`, `Row::LaneFile`,
    `Action::ReviewLane`), `git_panel_groups_tests.rs`.

### The breakdown's preamble, verbatim

Ordered; each card is at most m (about 2k lines with tests). **∥** marks
cards that can run in parallel with the others in the same wave once their
dependencies have landed. Files are where the card's change is; tests sit
beside them in each crate's `*_tests.rs` convention. Wave C's cards each
register commands or a panel through `workbench.rs` and the command
catalog, so parallel lanes there should expect small rebase conflicts in
those lists, not in each other's files.

## Since the design (Mac main at fe5d87fd7)

- **Card 0 has landed** as Mac T1509 (c79a9ec40), so this card's dependency on it is met. The tokens it gave differ in name from card 0's text: kiln's theme has `agent.base` and `agent.soft` (`kiln/crates/kiln-core/src/theme/tokens.rs`, `kiln/crates/kiln-core/themes/{dark,light}.toml`; no `agent.fg`), and fe-gpui's `EditorTheme` has only `agent_lane` (`crates/fe-config/themes/*.toml` `[editor] agent_lane`; no `agent`/`agent_soft`). Read the Colour rule's `agent.*` as these.
- `agent.review_lane` is registered by card 19; until it lands, `Action::ReviewLane` dispatches the command id and the test asserts the dispatch.

## Design sections it cites, verbatim

### Changes: its own group, no checkboxes, Review link

- **Shows**: under the user's groups, one group per lane with pending work,
  `lane 2 · claude-code` in silver-blue with `Review` at its right; its rows
  are the lane's files with `+a −r` and the pending count, and offer no
  Stage, Unstage or Discard, so they can't reach a commit. Accepted text is
  in the user's buffers and shows in the user's groups as any change.
- **Data**: `LaneSummary` per lane (`LaneStore::summaries()`): its files and
  `Counts` from the lane's `Changeset::files`, plus its loose proposals.
- **Reads it**: `crates/fe-workbench/src/git_panel.rs`: `enum Group { Merge,
  Staged, Changes }` gains `Lane(LaneId)`; lane rows are a new
  `Row::LaneFile(LaneId, usize)` beside `Row::File`, since `Entry` carries a
  `fe_git::ChangeKind` and lane files carry `Counts`. `Action` gets
  `ReviewLane`, which runs `agent.review_lane`.
- **Gap**: the panel has no checkboxes at all today (staging is buttons and
  keys); "no checkboxes" here means no staging actions on lane rows.

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

## Colour rule

Confirmed: **silver-blue (oil spot, `#b4c2cc` Yohen / `#3f6587` Hakuji)
is only ever an agent signal, and nothing an agent did is drawn in rust.**
Every lane surface takes its colour from the `agent.*` tokens (Mac T1509:
the agent signal and its soft ground, by theme), and no other surface does.

- **Silver-blue marks the agent's authorship or state**: the dotted change
  lane, the row tint, the `lane 2 · pending` chip, the title bar chip, the
  rail badge, the Problems and test chips, the agent terminal's tab, the
  Changes group's header, the diff's `After · lane 2` header, an agent
  toast's icon and its Review changes button.
- **Rust stays the user's**, also on agent things: the caret on an agent row,
  the focus ring on a lane chip or the Changes group, a selected row in the
  Agents panel (focused selection), the active tab line when the agent
  terminal is the active tab, the user's Accept all in review. These are
  where the user is, not who wrote it.
- **After accept** the text is still the agent's in provenance, so its rows
  keep the dotted silver-blue lane (no chip, no tint: those are for pending
  work). A row stays the agent's while any of its text is
  (`Author::row_rank` ranks AI above the user); one the user rewrites whole
  is theirs and loses the mark.
- **Added, removed, modified, warning** keep their hues on agent work: a
  pending hunk's new rows are celadon and its old rows shinsha, as any diff;
  silver-blue is the lane mark beside them, never a replacement for them.
- **No fallbacks to accent.** Until T1509's tokens land on this trunk, the
  surface cards don't start (§ Card breakdown, card 0). A surface card's
  test reads its colour from the theme's `agent.*` token and asserts it is
  not the accent's, in both themes.
- **Colour-blind mode** doesn't touch silver-blue: it swaps green and red,
  and the agent mark already differs from both in lightness and carries a
  glyph (the robot head, the dotted lane).

## Scope

- `crates/fe-workbench/src/git_panel.rs`
- `crates/fe-workbench/src/git_panel_groups_tests.rs`
- `crates/fe-workbench/l10n/en-US/changes.ftl`: the group header and Review (the Changes panel's strings live there) (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-workbench/src/git_panel_groups_tests.rs`: (own; the design gives no Tests line) under the user's groups, one group per lane with pending work, headed `lane 2 · <name>` with `Review` at its right; its rows are the lane's files with `+a −r` and the pending count, loose proposals included
2. Test in `crates/fe-workbench/src/git_panel_groups_tests.rs`: (own) lane rows offer no Stage, Unstage or Discard, by button or key; `Action::ReviewLane` dispatches `agent.review_lane` for its lane
3. Test in `crates/fe-workbench/src/git_panel_groups_tests.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes

## Notes

- Ready task T1591 (pre-commit checks) also touches `git_panel.rs`.

## Implementation notes

- **`+a −r` is lines**, as the AI Changes panel's rows show them: the lines a
  lane's undecided hunks (pending, and stale until rejected) add and remove
  in the file. `LaneSummary` carries only `Counts`, and `lanes.rs` is outside
  the card (and, in review round 1, under T361's live claim, so a respec
  adding it was refused), so the panel reads the lines from the open
  editors' proposals, each claimed by its lane through `LaneStore::lane_of`,
  and a pending new file's text from the turn the AI Changes panel shows,
  and keeps the last read for a file whose editor closes or whose turn is
  set aside. It follows the store from the first status landing (the
  project opening reads it) or its first draw, whichever is sooner, so the
  lines are read while the file's editor is open, the panel shown or not.
  Limit (C1 as respecced after review round 2): totals over every
  lane-owned turn, such as an unshown non-chat turn's new files or several
  pending turns at one new-file path, need `LaneSummary` to carry them from
  `Workbench::tally`; that's follow-up T369, after T361 frees `lanes.rs`.
  The pending count is `pending + stale` per file, as the lane's own count
  is.
- **A lane's rows** are the summary's files with undecided hunks, sorted by
  path from the root; a lane with none has no group. Selection runs on from
  the user's files into the lanes' (one index over both), so the keys reach
  a lane row, and every staging path (`act`, `act_on_all`, the vim keys,
  `git_panel.*_row`, `git.stage_file` through `focused_selection`) does
  nothing on one. Enter, `o`, Alt-Enter and a click open the file: the lane
  diff (`lane_diff.rs`) is a later card.
- **Review** dispatches `agent.review_lane` with args `{ "lane": n }`; the
  command is card 19's, so the test reads the workbench's `commands_run`.
- **Listing**: the panel observes the window's `LaneStore` from its first
  status landing or first draw, and lists the lanes again as it notifies.
- **Carried T333 findings** (F1–F3) are fixed here, as the card touches
  `git_panel.rs`: the box's wheel stops at the box, the cap subtracts all the
  content but the box and the list as measured, and Up/Down start from the
  rows shown. Their regressions are in `src/tests/git_commit_box_size.rs`,
  added to the scope by a respec.
