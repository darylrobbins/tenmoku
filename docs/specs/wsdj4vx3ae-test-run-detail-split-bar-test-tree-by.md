---
task_uid: "01m6w09gfx0edm3vwsdj4vx3ae"
task: "T1576"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::test_run_view (cargo nextest run -p fe-workbench) pass: split_bar_is_proportional_to_total, the_tree_groups_by_module_and_expands_failed_ones, bars_show_the_last_eight_oldest_left, flaky_tests_have_kiseto_outlined_bars, rerun_failed_runs_exactly_the_failed_names, rows_have_glyphs_and_accessible_names"
    required: true
    manual: false
  - id: "C2"
    text: "space t f runs runs.rerun_failed (cargo nextest run -p fe-keymap --test runs_bindings)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac, the audit scene named in the body renders in both themes and its probes pass (recorded in the decision file) (audit/runs_tests.rs)"
    required: true
    manual: false
---

# Test run detail: split bar, test tree by module, last-8 result bars with flaky outlines, and Rerun Failed Tests on space t f

# Test run detail: split bar, test tree, last-8 bars, rerun failed

Spec: Runs: tests (`docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`):
"a split bar (green passed, red failed, track remaining), a test tree, and each
test's last 8 results as small bars, so flaky tests stand out". Keys `␣tf`
rerun failed.

## Change

- `crates/fe-workbench/src/test_run_view.rs` (new): shown in the Runs panel
  when a Test run is selected (the panel hosts it; the list stays above or
  beside it per the panel's width). A split bar: celadon for passed, shinsha
  for failed, the track for what remains, proportional to `total`. Below, the
  test tree: modules as collapsible nodes (failed ones expanded), 24 px rows,
  a result glyph (✓ ✕ ▲ for flaky, a dash for ignored), the name in code
  small, the duration right-aligned, and the last 8 results as eight 3 x 10 px
  bars (celadon, shinsha, empty for none yet); a flaky test (T1574 `flaky`)
  has its bars outlined in kiseto.
- Selecting a test row exposes a detail slot (the failure detail is the next
  card).
- `runs.rerun_failed` (`␣tf`): runs only the failed tests of the selected
  run (`cargo test <names> -- --exact`), as a new Run. Palette title "Runs:
  Rerun Failed Tests".
- An audit scene `audit/runs_tests.rs`, both themes.

## Not in this card

The assertion view and flaky note (a later card), Ask an agent, Debug, filtering the tree.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-test-runs-…` in
`crates/fe-workbench/l10n/en-US/test_runs.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**; the audit scene is recorded on the Mac.

## Implementation notes

- The decision file `docs/decisions/wsdj4vx3ae-test-run-detail.md` has the
  details and the C4 recording.
- A rerun runs exactly the failed tests. Under nextest it's one run with a
  `-E` filterset of `binary_id(=…) & test(=…)` terms. Under `cargo test`
  it's one run per failing test executable: `-p <package> <its target>
  -- --exact <names>`. Cargo test runs carry `--message-format
  json-render-diagnostics`, whose `compiler-artifact` messages give each
  executable's package and target. A failure with no known target refuses
  the rerun. The first run's own name filters are dropped. The decision
  file's review rounds 1 and 2 have the details.
- `runs.rerun_failed` reruns the run selected in the Runs list if it's a
  test run, else the newest test run, so `␣tf` works from the editor.
- A failed test's glyph is ✕ even when it's flaky, and ▲ is a flaky test
  that passed. Under T1574's rule a fresh failure after passes counts as
  flaky, and ▲ there would hide the regression. The kiseto outline still
  marks it.
- The track is `border.control` (kiln's off-switch track): `border.subtle`
  barely shows on Yohen's panel.
- Scope added: `lib.rs`, `test_runner.rs`, `audit_scenes.rs`,
  `src/tests/mod.rs`, `src/tests/test_runner.rs`.
