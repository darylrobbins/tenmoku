---
task_uid: "01m6w09q0v5e3fbk6brsbkv936"
task: "T1580"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::build_timeline_view (cargo nextest run -p fe-workbench) pass: layout_scales_bars_to_the_build_length (pure function of timeline and width), fresh_crates_are_2px_lines, the_local_crate_is_accent, axis_ticks_step_1_5_10_30_seconds_by_length, the_segmented_control_switches_to_output, rows_have_accessible_names"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C3"
    text: "On the Mac, the audit scene named in the body renders in both themes and its probes pass (recorded in the decision file) (audit/runs_build.rs)"
    required: true
    manual: false
---

# Build detail: the crate waterfall (fresh lines, rebuilt bars, local crate in the accent) with Timeline or Output

# Build detail: the crate waterfall, Timeline or Output

Spec: Runs: build (`docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`).

## Change

- `crates/fe-workbench/src/build_timeline_view.rs` (new), shown in the Runs
  panel for a Build run, with a Timeline | Output segmented control (T1518's):
  - Timeline: one row per crate in start order, 18 px tall: fresh crates as a
    thin 2 px line, rebuilt ones as a bar from start to end scaled to the
    build's length, a local crate's bar in the accent, others in `text.muted`
    at 50%; the name in code small beside it, the duration right-aligned; a
    time axis along the top with ticks every 1, 5, 10 or 30 s by the length.
  - Warning and error counts beside a crate (▲ in kiseto, ✕ in shinsha); a
    crate row selects its messages in the Problems panel.
  - Output: the rendered diagnostics and `Compiling` lines, in code small,
    scrolling.
  - Live: a running build grows the bars; the layout is a pure function of
    the timeline and the width, tested without a window.
- An audit scene `audit/runs_build.rs`, both themes.

## Not in this card

Starting builds (T1579), cargo features per crate, comparing builds.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-build-runs-…` in
`crates/fe-workbench/l10n/en-US/build_runs.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**; the audit scene is recorded on the Mac.
