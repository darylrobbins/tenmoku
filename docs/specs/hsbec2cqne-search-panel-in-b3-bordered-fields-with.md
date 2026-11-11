---
task_uid: "01m7456eb2y7d4kzhsbec2cqne"
task: "T322"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::search_panel_b3 (cargo nextest run -p fe-workbench) pass: matches drawn with the find_match_other fill; long lines end in an ellipsis; the refresh, clear, tree and collapse actions are in the dock header, not a toolbar row; no results shows the centred empty state"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Search panel in B3: bordered fields with inner toggles, match fills, ellipsis and tabular line numbers, toolbar into the header, centred empty state (Mac T1546)

# Search panel in B3

Spec: `docs/design/design-system.md#workbench-anatomy` (pane controls),
`#surfaces`. Audit findings SR1–SR4 (the held cards P6, search half, and P13's
toolbar part).

## Change (`crates/fe-workbench/src/search_panel.rs`, `search_replace.rs`)

- The query and replace fields are the controls card's inputs, toggles inside
  as in the find row.
- Matches get the `find_match_other` fill, not bold only (SR2); result lines
  end in an ellipsis; line numbers in UI small with tabular figures (SR3).
- The toolbar row (refresh, clear, view as tree, collapse) moves into the dock
  header as the view's own actions (SR4), using the sidebar card's header API.
- Rows 24 px; the empty state (no results) is centred: icon, a heading, one
  hint line.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `search_panel.rs` and `search_replace.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/search.ftl` (keys `workbench-search-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Implementation notes

- The header actions go through a new `dock_item!(…, actions = …)` arm
  (`dock.rs`) that supplies `DockItem::actions`. `PaneAction` has no disabled
  look, so an action that doesn't apply (Refresh with no query, Clear with
  nothing to clear, Collapse with no results) does nothing when clicked.
- Registry titles come from `command-search-*` in `commands.ftl`
  (`command_title`, as the project panel does).
- The empty state replaces the status line's "No results" (its heading says it);
  `status()` still reports it.
- The audit probes in `examples/screenshots/audit/search_panel.rs` weren't
  re-checked: the screenshot harness is macOS-only.
