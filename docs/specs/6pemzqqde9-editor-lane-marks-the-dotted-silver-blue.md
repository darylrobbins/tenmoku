---
task_uid: "01m7b0mqgr6fskpp6pemzqqde9"
task: "T351"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-workbench/src/lane_marks_tests.rs: (own; the design gives no Tests line) accepted agent rows (provenance) and a pending hunk's rows draw the dotted lane in the change-bar column in place of the git bar, which shows on every other row; a row the user rewrote whole loses it"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-workbench/src/lane_marks_tests.rs: (own) the rows a pending hunk replaces get the `Tint::Agent` row tint under the proposal block, whose added and removed bands keep their own colours"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-workbench/src/lane_marks_tests.rs: (own) the proposal header shows `lane 2 · pending`, `lane 2 · stale` or `lane 2 · 1 conflict` before Accept and Reject; two lanes' pending hunks on one text each say `also changed by lane N` while both are pending"
    required: true
    manual: false
  - id: "C4"
    text: "Test in crates/fe-workbench/src/lane_marks_tests.rs: (own) an Inline Edit row (`fastedit.inline_edit`, no lane) draws the dotted lane and no chip"
    required: true
    manual: false
  - id: "C5"
    text: "Test in crates/fe-workbench/src/lane_marks_tests.rs: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes"
    required: true
    manual: false
---

# Editor lane marks: the dotted silver-blue change lane on agent rows, the faint agent row tint under pending hunks, and the lane N pending chip in proposal headers (agent lanes card 11) (Mac T1806)

# Editor lane marks: the dotted silver-blue change lane on agent rows, the faint agent row tint under pending hunks, and the lane N pending chip in proposal headers (agent lanes card 11)

Source: `docs/designs/agent-lanes.md` § Card breakdown, card **11** (the agent lanes design, Mac T1567, on main as e553a094a). The design files no cards itself; this is its card 11. Read the sections it cites before starting (quoted below or named).

Linux-OK: nothing in the card needs macOS; it builds and tests on Linux.

## The card, verbatim from § Card breakdown

**Wave C, after 6 (all ∥ with each other unless a dependency says so):**

11. **Editor lane marks.** Size **m**. Deps: 0, 2, 6. ∥. Files:
    `crates/fe-editor/src/decorations.rs` (`Tint::Agent`,
    `GutterMark::Dotted`), `crates/fe-editor/src/agent_rows.rs` (new:
    `AgentRows`, `LaneMark`), `crates/fe-gpui/src/element/decorations.rs`,
    `crates/fe-gpui/src/element/proposal.rs` and `view/proposal.rs`
    (`ProposalBadge`, `set_proposal_badges`; the `also changed by lane N`
    note), `crates/fe-workbench/src/lane_marks.rs` (new).

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
- Since the design, T1549 (Editor gutter B3) already draws a dotted agent lane in the change bar (`crates/fe-gpui/src/element.rs` `agent_lane_dots` / `paint_agent_lane`, colour `EditorTheme::agent_lane`), but only for the opt-in provenance column (`EditorView::set_gutter_annotations_with_agent_lane`, `crates/fe-gpui/src/gutter_annotations.rs`). This card makes it always on through `GutterMark::Dotted`, reusing that paint, and the provenance column must not draw it twice.
- The faint row tint is "`agent.*`'s soft ground"; `EditorTheme` has no soft agent colour, so add one as an `[editor]` key in every theme (a new `[editor]` key needs Ember and Monokai values too; `crates/fe-config/tests/design_role_keys.rs` pins the keys).

## Design sections it cites, verbatim

### Editor: dotted silver-blue change lane, faint row tint, "lane 2 · pending" chip

- **Shows**, per editor:
  - the **dotted lane** in the 3 px change-bar column beside rows an agent
    wrote: accepted agent text (provenance) and the rows of its pending
    hunks. It takes the column's place on those rows; the git bar shows on
    every other row.
  - the **faint row tint** (`agent.*`'s soft ground) behind the rows a
    pending hunk replaces, under the proposal's diff block, whose own added
    and removed bands keep celadon and shinsha.
  - the **chip**, `lane 2 · pending` (or `lane 2 · stale`, `lane 2 · 1
    conflict`), in the proposal block's header row before Accept and Reject.
- **Data**: `fe_editor::AgentRows` (new), `Vec<(Range<Anchor>, LaneMark)>`
  with `LaneMark { lane: Option<LaneId>, pending: bool }`, built from
  `Provenance::row_authors` (accepted: `Author::Ai { agent, session }`
  through `Lanes::lane_for`) and from each shown `ProposedChange`'s
  `hunks(snapshot)` (pending: `HunkView` with `HunkStatus::Pending` or
  `Stale`, the proposal's `origin()` through `lane_for`). The chip's text is
  a `fe_gpui::ProposalBadge { text: SharedString, stale: bool }` per
  proposal, handed with `EditorView::set_proposal_badges`.
- **Reads it**: a decoration provider in
  `crates/fe-workbench/src/lane_marks.rs` (new), installed with
  `EditorModel::add_decoration_provider`, rebuilt after edits rest (as
  `provenance_column.rs` is) and on `ProposalEvent`s; it emits
  `DecorationKind::Gutter(GutterMark::Dotted)` and
  `DecorationKind::RowBackground` with a new `Tint::Agent`
  (`crates/fe-editor/src/decorations.rs`). Painted by
  `crates/fe-gpui/src/element/decorations.rs` (the change lane, colour from
  `tint_color`, `Tint::Agent` from `EditorTheme`'s agent colour) and the chip
  by `crates/fe-gpui/src/element/proposal.rs`.
- **Gap**: no agent `Tint`, no dotted `GutterMark`, no agent colour in
  `EditorTheme`. The provenance column (`editor.toggle_provenance`) is a
  separate, opt-in annotation and stays as it is.

### Conflicts

- **One hunk, one place.** A stale hunk shows in its editor block as stale
  (today's drawing), counts in its lane's chip (`lane 2 · 1 conflict`), its
  file's row in the review panel and the Changes group, and Request changes
  sends it back to the agent with the user's text as the next instruction.
- **Two lanes, one text.** Both lanes' hunks are on the one buffer
  `LaneBuffers` gives, each with its own lane chip. Whichever is accepted
  first goes in; the other's old text is gone, so it goes `Stale` by the
  existing rule (an insertion of the other lane at that point is stale by
  the position rule). The hunk header says `also changed by lane 3` while
  both are pending, read from the editor's other proposals' ranges.
- **The agent's base moved.** For a worktree lane, every hunk is checked at
  projection time, not when the agent wrote it, so a lane started on an old
  base shows conflicts only where the user's text actually differs.

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

- `crates/fe-editor/src/decorations.rs`
- `crates/fe-editor/src/agent_rows.rs`
- `crates/fe-editor/src/lib.rs`: declares `agent_rows` (beyond the card's file list).
- `crates/fe-gpui/src/element/decorations.rs`
- `crates/fe-gpui/src/element/proposal.rs`
- `crates/fe-gpui/src/view/proposal.rs`
- `crates/fe-workbench/src/lane_marks.rs`
- `crates/fe-workbench/src/lane_marks_tests.rs`: new test file, declared from `lane_marks.rs` (beyond the card's file list).
- `crates/fe-workbench/src/lib.rs`: declares `lane_marks` (beyond the card's file list).
- `crates/fe-gpui/src/element.rs`: the dotted agent lane T1549 already paints (`agent_lane_dots`, `paint_agent_lane`): `GutterMark::Dotted` reuses it (beyond the card's file list).
- `crates/fe-gpui/src/gutter_annotations.rs`: `set_gutter_annotations_with_agent_lane`, the provenance column's route to the same dots, which must not double-draw with the decoration (beyond the card's file list).
- `crates/fe-gpui/src/lib.rs`: `EditorTheme` gains the soft agent ground for `Tint::Agent` (T1509 gave only `agent_lane`) (beyond the card's file list).
- `crates/fe-gpui/src/theme.rs`: maps the new editor key (beyond the card's file list).
- `crates/fe-config/src/theme.rs`: the new `[editor]` key (beyond the card's file list).
- `crates/fe-config/themes/*.toml`: every theme file gets the new `[editor]` key (Ember and Monokai included) (beyond the card's file list).
- `crates/fe-config/tests/design_role_keys.rs`: pins the `[editor]` design-role keys (beyond the card's file list).
- `crates/fe-gpui/l10n/en-US/fe-gpui.ftl`: the `also changed by lane N` note (beyond the card's file list).
- `crates/fe-workbench/l10n/en-US/editor.ftl`: the chip texts `lane 2 · pending`, `lane 2 · stale`, `lane 2 · 1 conflict` (beyond the card's file list).
- `crates/fe-gpui/l10n-allow.toml`: fe-gpui's literal lint is `all = true` (beyond the card's file list).
- `crates/fe-workbench/l10n-allow.toml`: fe-workbench's literal lint is `all = true` (beyond the card's file list).

## Criteria

Each Tests item of the card, verbatim, as a criterion; items marked "(own)" are mine where the design gives none.

1. Test in `crates/fe-workbench/src/lane_marks_tests.rs`: (own; the design gives no Tests line) accepted agent rows (provenance) and a pending hunk's rows draw the dotted lane in the change-bar column in place of the git bar, which shows on every other row; a row the user rewrote whole loses it
2. Test in `crates/fe-workbench/src/lane_marks_tests.rs`: (own) the rows a pending hunk replaces get the `Tint::Agent` row tint under the proposal block, whose added and removed bands keep their own colours
3. Test in `crates/fe-workbench/src/lane_marks_tests.rs`: (own) the proposal header shows `lane 2 · pending`, `lane 2 · stale` or `lane 2 · 1 conflict` before Accept and Reject; two lanes' pending hunks on one text each say `also changed by lane N` while both are pending
4. Test in `crates/fe-workbench/src/lane_marks_tests.rs`: (own) an Inline Edit row (`fastedit.inline_edit`, no lane) draws the dotted lane and no chip
5. Test in `crates/fe-workbench/src/lane_marks_tests.rs`: § Colour rule: A surface card's test reads its colour from the theme's `agent.*` token and asserts it is not the accent's, in both themes

## Notes

- `Tint` is matched exhaustively in `crates/fe-gpui/src/element/decorations.rs` (in scope); `todo_picker.rs` and `git_conflicts_tests.rs` use `_`/panic arms that a new variant does not break.
- Ready task T1600 (italic and bold spans) also touches `crates/fe-gpui/src/element/decorations.rs`.

## Implementation notes

- **Theme key.** The soft agent ground is `[editor] agent_row` (`EditorTheme::agent_row`), set in the six themes that list every editor key: Yohen and Hakuji to their `agent.soft`, Tenmoku Dark, Ember and Monokai to Kiln Dark's `agent.soft` (`#8d9bb026`), Tenmoku Light to Kiln Light's (`#6a7a9226`). The other themes take the bundled palette's. `design_role_keys.rs` pins it to the theme's `agent.soft`. `Tint::Agent` resolves to `agent_lane` (`tint_color`), and as a row band to `agent_row` unfaded (`diff_color`).
- **Dotted lane.** `GutterMark::Dotted` places no quad: `Decorations::layout` records its runs (`Decorations::dotted`), which the frame joins with the provenance column's agent runs (`agent_lane_runs` in `element.rs`) before the existing `clear_bars_from_agent_runs` / `agent_lane_dots` / `paint_agent_lane`. Joined runs give each row its dots once, so the opt-in column never double-draws. `DrawnGutter::agent_dot_color` records the colour.
- **Band under the diff bands.** The proposals' removed band is opaque in Yohen and Hakuji, and was painted before decoration row backgrounds, so a `Tint::Agent` band in the usual layer would have covered shinsha. Agent bands paint in a new `Layer::AgentRows`, before `proposal::paint_bands`. As a result, in those two themes the band shows only where the removed band is translucent.
- **Agent lane colour (review F2).** Tenmoku Dark, Light, Ember and Monokai had `agent_lane` as the cursor's colour at 0.22, an accent fallback. Now that the lane is always on and colours the chip, each takes its Kiln theme's `agent.base` (`#8d9bb0` dark, `#6a7a92` light). `design_role_keys.rs` pins `agent_lane` to `agent.base`, and the C5 test covers all six themes that set the agent keys.
- **Whole rows (review F1).** A pending hunk's mark covers its rows whole, not just its bytes. The frame queries decorations by the bytes of the display rows in view, so a partial-row hunk on a wrapped row would drop out once its own piece scrolled off. The test is `a_word_replaced_in_a_wrapped_row_marks_every_piece_in_view`.
- **Pure insertions** replace no buffer row, so a pending insertion has no dotted rows and no band. It still gets its chip.
- **Chip.** `ProposalBadge` carries `lane: LaneId` beside the design's `text` and `stale`, because fe-gpui names the other lane in `also changed by lane N`. Badges are kept per proposal by `Weak` pointer. The workbench gives every proposal of a lane in one editor the same chip, counted over all of them: `pending` with no stale hunk, `N conflict(s)` with some stale and some pending, `stale` when all are. A stale chip's text is dimmed, as a stale header is. The chip is the agent's colour (`agent_lane` at full alpha) on `agent_row`. The note only names lanes whose overlapping hunks are `Pending` and whose proposal has a chip of another lane.
- **Out-of-scope touches.** `crates/fe-gpui/src/view.rs` gains the `proposal_badges` field and re-exports `ProposalBadge`, since `EditorView`'s fields live there. `fe-gpui/l10n-allow.toml` allows the `"agent rows"` `#[cfg(test)]` paint note.
- **Wiring.** `lane_marks::init` (from `fe_workbench::init`) observes each new workbench, as `test_lens` does, so `workbench.rs` is untouched. A split's copied provider is replaced, as spell squiggles do.
