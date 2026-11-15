---
task_uid: "01m7eqh5sbs0ng53wympdvp95r"
task: "T388"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::operations_status (cargo nextest run -p fe-workbench) pass: the_item_is_absent_when_idle_and_the_bar_does_not_shift, label_bar_count_and_plus_n, click_lists_every_operation, repaint_is_capped_at_10_per_second (fake clock), the_accessible_name_reads_label_and_n_more"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C3"
    text: "Known limit: the audit scene audit/operations.rs is built on Linux but is NOT recorded here (recording and the probes run on the Mac, Metal-only): the decision file says the scene is unrecorded and the Mac lead records it on the Mac when the batch lands"
    required: true
    manual: false
---

# Operation in progress in the status bar: spinner, label, 72 px bar, count, +N and the list on click (Mac T1586)

# Operation in progress in the status bar

Spec: as `Operations` (T1585): "spinner, label, a 72 px bar, a count, and +N
for other background tasks. Clicking lists them all."

## Change

- A left-side status item right after the Vim mode pill
  (`crates/fe-workbench/src/status_bar.rs`): a 12 px spinner (the icon from
  T1516, rotating only while visible), the primary operation's label in UI
  label, a 72 px bar (accent fill, 4 px tall) when it has a fraction, a count
  `3/12` when its message carries one, and `+2` for the others. With no
  operation the item is absent and the bar does not shift.
- Click opens a list above the item: one row per operation (spinner or bar,
  label, source), `Esc` closes. Accessible name: the label and "N more".
- The spinner and bar repaint at most 10 times a second; an idle workbench
  paints nothing.
- An audit scene `audit/operations.rs`, both themes.

## Not in this card

Cancel buttons, notifications for finished operations.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-status-…` in
`crates/fe-workbench/l10n/en-US/status.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**; the audit scene is recorded on the Mac.

## Implementation notes

- `operations_status.rs` is the item and its list; `StatusBar` splices it into
  the left items after the vim mode and its recording, and builds the frame
  itself only while an operation runs (otherwise the bar is kiln-shell's as
  before, so with none nothing changes).
- A timer ticks every 100 ms while operations exist and is the only thing that
  repaints the item (spinner step, and whatever the operations said since);
  with none it stops and nothing paints.
- The audit scene `audit/operations.rs` is built here and unrecorded: the Mac
  lead records it on the Mac (Metal-only) when the batch lands.
