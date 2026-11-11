---
task_uid: "01m6vvv64ytyrf3gsatpz5d5rf"
task: "T1547"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::problems_b3 (cargo nextest run -p fe-workbench) pass: This file shows only the active editor's problems, Project all; each severity filter shows its count and hides that severity when off; rows are grouped by file with a count badge; every row carries its severity glyph"
    required: true
    manual: false
  - id: "C2"
    text: "The count(…, \"warning\") helper is gone: counts are plural messages (a fake-locale test)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac, the audit's problems scene renders in both themes and its probes pass (recorded in the decision file)"
    required: true
    manual: false
---

# Problems panel in B3: grouped by file, This file / Project scope, severity filters that double as counts, glyphs, centred empty state

# Problems panel in B3: grouped by file, scope switch, severity filters as counts

Spec: `docs/design/design-system.md#problems`. Audit finding PB2 (empty state).

## Change (`crates/fe-workbench/src/problems_panel.rs`)

- Header: a segmented scope switch, This file / Project (From agents waits for
  agent lanes), and severity filters that double as counts (✕ 3, ▲ 5, ⓘ 2,
  each a toggle; off hides that severity).
- Grouped by file (exists): the file row with its icon, path in muted, and a
  count badge; problem rows 24 px with the severity glyph in its colour, the
  message in ink, source and line:col in faint code small.
- Selected row: the focused-selection row; Enter opens it (as now).
- The `count(…, "warning")` helper becomes plural messages.
- Empty state centred: icon, "No problems" heading, one hint.
- An audit scene `audit/problems.rs` with rows (the audit card's hook), in
  both themes, with probe points on a row, a badge and the selected row.
- The detail pane (snippet, Apply fix ⌥↵, Ask an agent, Explain) is separate
  work.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `problems_panel.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/problems.ftl` (keys `workbench-problems-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK** (the audit scene is the Mac visual check).
