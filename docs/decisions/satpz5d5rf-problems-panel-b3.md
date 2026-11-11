# Problems panel in B3 (T1547)

Spec: `docs/specs/satpz5d5rf-problems-panel-in-b3-grouped-by-file.md`;
design: `docs/design/design-system.md#problems`; audit finding PB2.

## What changed

- **Header** (`ProblemsPanel::render_header`): kiln-ui's `SegmentedControl`
  (small), This file / Project, then the three severity filters, right
  aligned. The filters are one group, so in the default 248 px sidebar they
  wrap together onto a second line, not 2 + 1. On one line in a wider panel.
  From agents waits for agent lanes.
- **Scope.** Project lists every file, as before, and is the default. This
  file lists the active editor's file alone. The workbench tells the panel
  which file that is as it renders (`Workbench::follow_problems_file`, called
  from `follow_active_editor` beside the outline's `set_editor`) and in
  `sync_problems`. A tab switch doesn't always notify, so the render hook is
  what follows it.
- **Severity filters** are kiln-ui `ToggleButton`s labelled with their count
  in the scope. Each has its glyph in its severity's colour, through a new
  `ToggleButton::glyph` (below). Off, a filter hides its severity's rows but
  keeps counting them. Hints go under the information filter: the design has
  three filters. A filter's accessible name and tooltip is a plural message,
  "2 errors".
- **Rows.** Grouped by file as before. A file is left out when the scope or
  the filters hide all its problems. A file row has its file icon
  (`file_icon_ref`), its path in `text.muted`, and a count badge (6 px
  padding, `radius::CONTROL`, `list.row_hover` fill, `UI_SMALL`) that counts
  the problems shown. Problem rows are `ROW_HEIGHT` (24 px). Each has its
  severity glyph (14 px, in the editor's diagnostic colour), its message in
  `text.primary`, then its source and `row:col` in `text.faint`, code small.
  Related rows put their `file:row` in the same faint code small, and their
  message in muted. The selected row is the list's focused (or unfocused)
  selection, as before. Enter still opens it.
- **Glyphs.** Error `circle-x` and warning `triangle-alert`, as on the status
  bar. Information is `circle-dot`: the bundle has no Lucide `info`, and
  adding one means a new asset plus both THIRD_PARTY.md lists. A hint is
  `diamond`, so its shape tells it from information, not only its colour.
- **Empty state** (PB2): centred in the space under the header, with the
  `circle-check` icon, a `UI_HEADING` heading and one muted hint. There are
  three cases: nothing listed at all ("No problems" / "Errors and warnings
  in the open files show here"), This file clean ("No problems" / "None in
  this file; Project lists every open file"), and every problem hidden by
  the filters ("No problems shown" / "Turn a severity filter back on to list
  its problems").
- **Text in the catalogue** (`l10n/en-US/problems.ftl`,
  `workbench-problems-…`). The `count(n, "error")` helper is gone. The status
  items' tooltip is one plural message, `workbench-problems-status-tooltip`,
  and its English output is byte-equal ("1 error, 2 warnings"). The panel's
  name in the dock header and the rail tooltip is
  `workbench-problems-title`. The old "No problems in the open files" line is
  replaced: the design rewrites the empty state. `problems_panel.rs` is on
  `l10n-allow.toml`'s migrated list. Its element-id and selector format
  strings, and the SidebarPanel const's `name` (as for Project), are listed
  literals.

## Tests

`src/tests/problems_b3.rs` (C1, C2):
`this_file_lists_the_active_editors_problems_and_project_every_files`,
`each_filter_shows_its_count_and_hides_its_severity_when_off`,
`rows_are_grouped_by_file_with_a_count_badge`,
`every_row_carries_its_severity_glyph`, `the_empty_state_is_centred`, and
`the_counts_are_plural_messages`. The last switches to a fake `qps-ploc`
locale whose filter and status messages select `[one]`/`[other]`.
`problems_panel_tests::lists` now expects the new empty-state text.

## Out of the spec's file list

- `kiln/crates/kiln-ui/src/toggle_button.rs`: `ToggleButton::glyph(icon,
  color)`, an optional icon before the label, in `color` while on and the
  toggle's muted text colour while off. This is an additive builder: no
  existing caller changes and no golden moves. Without it, a filter's glyph
  can't be in its severity's colour inside a focusable, AccessKit-toggled
  control.
- `crates/fe-workbench/src/dock.rs`: the panel's `dock_item!` takes its
  title from the catalogue.
- `crates/fe-workbench/src/workbench_chrome.rs`: one line,
  `follow_problems_file`, in `follow_active_editor`.
- `crates/fe-workbench/src/l10n.rs`, `l10n-allow.toml`,
  `src/tests/mod.rs`: append-only lines.
- `crates/fe-workbench/src/problems_panel_tests.rs`: the empty-state
  assertion.
- `examples/screenshots/audit_scenes.rs`: the scene's `mod` line and entry.
  `audit/problems_rows.rs` became `audit/problems.rs`, renamed as T1553
  renamed `git_panel` to `changes`.

## Audit (C4)

On the Mac (2026-11-11),
`cargo run -p fe-workbench --example screenshots -- audit <dir>` wrote all
49 captures. The scene `audit/problems.rs` (`problems_dark.png`,
`problems_light.png`) renders README.md and src/main.rs grouped, with count
badges 1 and 4, every severity's glyph, the header's counts ✕ 1, ▲ 2, ⓘ 2,
and the first problem selected (`problems.select_first`). Its probes, all
passing in both themes:

| Point | Token |
| --- | --- |
| (174, 400), below the rows | `surface.panel` |
| (50, 212), a problem row's indent | `surface.panel` |
| (253.5, 140), README.md's badge left of its digit | `list.row_hover` over `surface.panel` |
| (50, 164), the selected row's indent | `list.row_selected` over `surface.panel` |
| (400, 400), the editor | `surface.background` |

```
ok      problems_panel_dark probes (2)
ok      problems_panel_light probes (2)
ok      problems_dark probes (5)
ok      problems_light probes (5)
```

The empty-state scene (`problems_panel`) shows the icon, heading and hint
centred under the header, and its probes still pass. The run's eight other
failures were stale before this card, as T1543's decision file records:
chrome, yohen and hakuji (600, 45), settings (501, 354) and
shortcuts_editor (900, 140). None of them is the Problems panel's.
