---
task_uid: "01m7b0mqpe2pjc9abd6cv52prp"
task: "T352"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/problems_panel_tests.rs: (own; the design gives no Tests line) a file row whose text a lane wrote and the user accepted shows `written by lane N`; a file with only pending hunks shows no chip"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/problems_panel_tests.rs: (own) a problem on a row the lane wrote, in an open buffer, shows `new, lane N` (its row's `Author` through `Lanes::lane_for`)"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/problems_panel_tests.rs: (own) the scope switch is This file / Project / From agents, and From agents keeps only files with a lane chip"
    required: true
    manual: false
  - id: "C4"
    text: "Test in crates/fe-workbench/src/problems_panel_tests.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Problems lane chips and the From agents scope: written by lane N on files, new lane N on problems, and a third scope that keeps only files with a lane chip (agent lanes card 12) (Mac T1807)

# Problems lane chips and the From agents scope: written by lane N on files, new lane N on problems, and a third scope that keeps only files with a lane chip (agent lanes card 12)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **12** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 12. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

12. **Problems: lane chips and the scope switch.** Size **m**. Deps: 0, 2,
    6. ∥. Files: `crates/fe-workbench/src/problems_panel.rs`
    (`ProblemScope`, lane chips), `problems_panel_tests.rs`.

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
- Since the design, the Problems panel gained a scope switch (`problems_panel.rs:160`, `Scope { ThisFile, Project }`, header switch, `workbench-problems-scope-*`). Add `FromAgents` to that `Scope` rather than a separate `ProblemScope`.
- The tests row's `new, lane 2` waits for the Runs design (§ Card breakdown › Later); this card is the Problems side only.

## Design sections it cites, verbatim

### Problems and tests: "written by lane 2", "new, lane 2", From agents scope

- **Shows**: a `written by lane 2` chip on a file row whose text the lane
  wrote and the user accepted; `new, lane 2` on a problem on a row the lane
  wrote; a scope switch, This file / Project / From agents, where From
  agents keeps only files with a lane chip. A file with only pending hunks
  gets no chip: its problems are the language server's view of the user's
  text, which holds none of the lane's yet.
- **Data**: `LaneStore::written_files() -> HashMap<PathBuf, Vec<LaneId>>`
  (files with accepted hunks in each lane's changeset or loose proposals,
  and open buffers with rows the lane is author of) for file rows; for a
  problem, its row's `Author` from the open buffer's
  `Provenance::row_authors` through `Lanes::lane_for` (closed files have no
  provenance, so only the file chip). The rows the panel lists stay
  `fe_editor::diagnostics::FileProblems` and `Problem`; the panel's own
  `FileProblems` listing gains `lanes: Vec<LaneId>` and each listed problem
  `lane: Option<LaneId>`, and a new `ProblemScope { ThisFile, Project,
  FromAgents }` filters `rows`.
- **Reads it**: `crates/fe-workbench/src/problems_panel.rs`
  (`ProblemsPanel`, `Row::File` and `Row::Problem`).
- **Tests**: there is no Runs panel or test explorer yet; tasks run in
  terminal tabs (`task_run.rs`, `TaskTab`) and feed Problems. The tests
  row's `new, lane 2` waits for the Runs design, but the data contract is
  fixed here, because a result can arrive after the lane has moved on:
  - Each run captures **what it tested** when it starts: `Tested { lane:
    LaneId, tree: String }`, the git tree id of the worktree's whole state
    (tracked and untracked, written through a temporary index, as a frozen
    revision is), or for a run in the user's tree no lane.
  - Results and the problems a task's output gives carry that `Tested`
    (`TaskTab { tested: Option<Tested> }`, and through `task_problems.rs`'s
    `TaskProblem` to the panel's `SourceProblem`).
  - A result belongs to a frozen revision only if that revision's tree id
    equals `tested.tree`; otherwise it is shown as `stale result, lane 2`,
    never as the current revision's. Overlay lanes run nothing, so have no
    test results.
- **Gap**: no scope switch today; the panel always lists every open buffer,
  unopened files the server reported and task output.

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

- `crates/fe-workbench/src/problems_panel.rs`
- `crates/fe-workbench/src/problems_panel_tests.rs`
- `crates/fe-workbench/src/tests/problems_b3.rs`: pins the header's two-way scope switch, which gains From agents (beyond the card's file list).
- `crates/fe-workbench/l10n/en-US/problems.ftl`: the scope's and the chips' text (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-workbench/src/problems_panel_tests.rs`: (own; the design gives no Tests line) a file row whose text a lane wrote and the user accepted shows `written by lane N`; a file with only pending hunks shows no chip
2. Test in `crates/fe-workbench/src/problems_panel_tests.rs`: (own) a problem on a row the lane wrote, in an open buffer, shows `new, lane N` (its row's `Author` through `Lanes::lane_for`)
3. Test in `crates/fe-workbench/src/problems_panel_tests.rs`: (own) the scope switch is This file / Project / From agents, and From agents keeps only files with a lane chip
4. Test in `crates/fe-workbench/src/problems_panel_tests.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes

## Notes

- Ready task T1587 (Problems detail pane) also touches `problems_panel.rs`.

## Implementation notes

- `Scope` gained `FromAgents` (no separate `ProblemScope`), third in the
  switch. The panel keeps a `FileLanes` beside each listed file (`wrote`,
  and each problem's row lane) rather than changing `fe_editor`'s
  `FileProblems`.
- A file's chip lanes are `LaneStore::written_files` for its path plus, for
  an open buffer, the lanes of the rows its provenance gives an agent; a
  problem's lane is its row's. Only the rows an agent's spans reach are
  read (`row_authors`), so a buffer no agent touched costs one span walk.
- `Workbench::sync_problems` subscribes to the lane store the first time it
  runs (held on the panel), so the chips follow hunks being decided.
- C4 reads the chips the last frame drew (`drawn_chips`, test only) under
  the default dark and light themes and compares them with the active
  theme's `agent.base` / `agent.soft`, asserting neither is the accent.
