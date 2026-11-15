---
task_uid: "01m7f02vp1jg1kc095kh4ew7f0"
task: "T389"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::test_failure_detail (cargo nextest run -p fe-workbench) pass: assertion_blocks_use_shinsha_left_celadon_right_with_glyphs, a_non_assertion_failure_shows_output_only, the_flaky_note_reads_failed_3_of_its_last_8_runs_likely_flaky (plural variants), open_test_jumps_to_the_function"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Test failure detail: the assertion with left in red and right in green, the flaky note, and Open test (Mac T1577)

# Test failure detail: the assertion, left and right, and the flaky note

Spec: Runs: tests (`docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`):
"A failure shows the assertion with left in red and right in green, a 'failed
3 of its last 8 runs, likely flaky' note".

## Change (`crates/fe-workbench/src/test_run_view.rs`, after the tree)

- The detail slot of a failed test shows, in code small: the `Assertion`
  (T1573) as two blocks, `left` on a shinsha-soft ground and `right` on a
  celadon-soft one, each with its glyph (− and +) so it never rests on hue; the
  panic message and location above; the captured output below, scrolling.
  When the failure is not an assertion, the output alone.
- The flaky note, a plural message ("failed 3 of its last 8 runs, likely
  flaky"), when `History::flaky` says so.
- An action row that later cards fill (Ask an agent, Debug this test): empty
  here, with a stable place for it.
- The failed row's path to the test source: `Open test` jumps to the function
  (the runnables lookup of `test_runnables.rs`).

## Not in this card

Ask an agent, Debug this test, diffing long values (they wrap; no word diff).

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-test-runs-…` in
`crates/fe-workbench/l10n/en-US/test_runs.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**
