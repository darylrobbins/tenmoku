---
task_uid: "01m7b0mr71btst8r7fpkett7rs"
task: "T355"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/lane_diff_tests.rs: (own; the design gives no Tests line) `agent.diff_lane_file` opens a side-by-side diff of one of a lane's files headed `Before` and `After · lane N`, its right side the buffer with the lane's pending hunks applied (`ProposedChange::dry_run(snapshot).text()`), as `git_diff.rs` opens `DiffView`s with `fixed_side()`"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/lane_diff_tests.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Lane diff with the After lane N header: DiffSide::agent for a silver-blue right header, agent.diff_lane_file with the right side from DryRun text (agent lanes card 15) (Mac T1810)

# Lane diff with the After lane N header: DiffSide::agent for a silver-blue right header, agent.diff_lane_file with the right side from DryRun text (agent lanes card 15)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **15** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 15. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

15. **Lane diff with the "After · lane N" header.** Size **s**. Deps: 0, 6.
    ∥. Files: `crates/fe-gpui/src/diff_view.rs` (`DiffSide::agent`),
    `crates/fe-workbench/src/lane_diff.rs` (new: `agent.diff_lane_file`,
    right side from `DryRun::text`).

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
- Opening from the Agents panel, the Changes group's rows and the AI Changes panel (§ Diff › Reads it) is wired by whichever of cards 10 and 14 lands after this one, or left as the command; this card gives the command.

## Design sections it cites, verbatim

### Diff: "After · lane 2" header

- **Shows**: reviewing one of a lane's files side by side, the left header
  `Before` and the right `After · lane 2`, the right header's text
  silver-blue. Comments becoming the lane's next instruction is the review
  panel's (`docs/designs/agent-review.md`); the lane end of it is
  `LaneStore::instruct(lane, text)`, which delivers a `lane.instruction`.
- **Data**: `fe_gpui::DiffSide { title, model, read_only }`
  (`crates/fe-gpui/src/diff_view.rs`) with the title `After · lane 2` and a
  new `DiffSide::agent(bool)` for the header's colour; the right text is
  the buffer with the lane's pending hunks applied,
  `ProposedChange::dry_run(snapshot).text()` (`DryRun`).
- **Reads it**: `crates/fe-workbench/src/lane_diff.rs` (new), opened from
  the Agents panel, the Changes group's rows and the AI Changes panel, as
  `git_diff.rs` opens `DiffView`s with `fixed_side()`.
- **Gap**: headers today are free-form (`HEAD`, `Index`, `Working Tree`,
  `Saved`); there is no Before/After pair or agent header.

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

- `crates/fe-gpui/src/diff_view.rs`
- `crates/fe-workbench/src/lane_diff.rs`
- `crates/fe-workbench/src/lane_diff_tests.rs`: new test file, declared from `lane_diff.rs` (beyond the card's file list).
- `crates/fe-workbench/src/lib.rs`: declares the module and puts `agent.diff_lane_file` in the command catalog (beyond the card's file list).
- `crates/fe-workbench/src/git_diff.rs`: a `DiffSide { .. }` literal: only if a new field (not a builder) needs it (beyond the card's file list).
- `crates/fe-workbench/src/git_blame.rs`: a `DiffSide { .. }` literal, as above (beyond the card's file list).
- `crates/fe-workbench/src/git_history.rs`: a `DiffSide { .. }` literal, as above (beyond the card's file list).
- `crates/fe-workbench/l10n/en-US/diff.ftl`: `Before`, `After · lane N` and the command's title (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-workbench/src/lane_diff_tests.rs`: (own; the design gives no Tests line) `agent.diff_lane_file` opens a side-by-side diff of one of a lane's files headed `Before` and `After · lane N`, its right side the buffer with the lane's pending hunks applied (`ProposedChange::dry_run(snapshot).text()`), as `git_diff.rs` opens `DiffView`s with `fixed_side()`
2. Test in `crates/fe-workbench/src/lane_diff_tests.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes
