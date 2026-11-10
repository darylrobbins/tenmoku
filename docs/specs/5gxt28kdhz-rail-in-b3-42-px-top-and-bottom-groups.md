---
task_uid: "01m6vvtn3r8yt69j5gxt28kdhz"
task: "T1540"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::rail_b3 (cargo nextest run -p fe-workbench) pass: 42 px rail, 32 px items; order Project, Search, Changes, Chat (robot icon), then on-demand panels, and Problems, Git log, Terminal at the foot; the selection's 2 px accent bar is on the panel-facing edge and flips with the side; Changes' badge shows the changed-file count, Problems a dot when there are errors"
    required: true
    manual: false
  - id: "C2"
    text: "Test rail_side_parses_and_defaults_left (cargo nextest run -p fe-config --test rail_setting) passes; changing workbench.rail moves rail and side dock at once (tests::rail_b3::rail_setting_applies_live)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (the five fe-workbench goldens)"
    required: true
    manual: false
---

# Rail in B3: 42 px, top and bottom groups, the accent edge on the panel side, badges, and a workbench.rail left/right setting

# Rail in B3: 42 px, groups, the rust selection edge, badges, and workbench.rail

Spec: `docs/design/design-system.md#workbench-anatomy` (Rail, Rail selection,
Rail badges; Measure: 42 px wide, 32 px items) and `#agent-provenance`
(the count on Agents waits for agent lanes). Audit finding PB1.

## Change

- `kiln/crates/kiln-ui/src/activity_rail.rs`: 42 px wide (`RAIL_WIDTH` 36
  today), 32 px items, 16 px icons in the text colour of their state.
  Selection: a tinted tab behind the icon that runs to the rail's
  panel-facing edge, with a 2 px accent bar on that edge, flipping with the
  rail's side (today the bar is always on the left). Badges bottom-right with
  a 2 px cut-out ring, using the controls card's `Badge`, never over the glyph.
- Order (`crates/fe-workbench/src/dock.rs` `PANELS`, and the footer in
  `crates/fe-workbench/src/workbench.rs`): top Project (was Explorer), Search,
  Changes (was Source Control), Chat with the robot icon; the panels shown on
  first use (References, Hierarchy, Quickfix, History) after them; bottom
  Problems (`triangle-alert`, was `X`), Git log, Terminal. The names change in
  each panel's `SIDEBAR` const (`project_panel.rs`, `source_control.rs`,
  `chat_panel.rs`, `problems_panel.rs`).
- Badges: Changes shows the changed-file count in `status.modified`; Problems a
  red dot when there are errors.
- `workbench.rail = "left" | "right"` (default `"left"`), user and project
  settings, applied live: the side dock and rail move together (today's
  `move_sidebar` commands write the same state). `crates/fe-config/src/settings.rs`,
  schema, template (and its generated ftl after C1), applied in
  `crates/fe-workbench/src/workbench_root.rs`.
- Tests that pin the rail follow (`crates/fe-workbench/src/tests/activity_rail.rs`,
  `kiln/crates/kiln-shell/tests/dock_rail_actions.rs`).
- Regenerate the five fe-workbench goldens on the Mac.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `dock.rs` and `dock_layout.rs` (the panel names in the other panels' files are migrated by those panels' cards), the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/dock.ftl` (keys `workbench-dock-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (goldens).

## Implementation notes

The design calls (selection tint and inset, badge placement, host-set
badges, the Git log as a rail action, the saved layout yielding to
`workbench.rail`) and the files touched beyond the scope list are in
`docs/decisions/5gxt28kdhz-rail-b3.md`.
