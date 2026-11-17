---
task_uid: "01m7dn73vbcs7ya28af5s5qyw9"
task: "T367"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::test_lens_history (cargo nextest run -p fe-workbench) pass: passed_in_the_last_8_runs, failed_in_3_of_the_last_8_runs, flaky_is_kiseto, no_results_shows_nothing, it_updates_when_a_run_finishes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench test_lens passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Test code lens: passed in the last 8 runs, from the test history (Mac T1605)

# "passed in the last 8 runs" on a test's code lens

Spec: `docs/design/design-system.md#editor` ("Code lenses above tests in sans:
▷ Run test · Debug · passed in the last 8 runs"). `test_lens.rs` gives Run
Test; the lens annotation card (T1550) restyles it and defers this segment.

## Change

- `crates/fe-workbench/src/test_lens.rs`: after Run Test, a third segment from
  the test history (`fe-runlog` `History`, last 8 for the test's id): "passed
  in the last 8 runs", "failed in 3 of the last 8 runs", "flaky" (kiseto) or
  nothing when the test has no results. The id is matched from the lens
  row's test path to the history's `TestId` (the same `crate::path` form the
  runner records). Updates when a test run finishes.
- It is plain text, not a click target.

## Not in this card

Debug (a separate card), per-test durations.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-lens-…` in
`crates/fe-workbench/l10n/en-US/lens.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**

## Implementation notes

- `LensItem` gained plain text (`invocation: None`) and a `warning` flag (kiseto); "flaky" = it has failed and its last run passed.
- A test's history id is resolved from the root's `cargo metadata` targets (read off the main thread when the root changes): the target whose root file is the lens's file, or whose module folder is the deepest one holding it, gives the nextest binary id (`p`, `p::bin/p`, `p::more`) the runner records; the module path is the file's under that folder. A file two targets could own (`src/lib.rs`'s and `src/main.rs`'s modules), or none, shows nothing rather than a guess. `#[path]` modules are found where they are, not where their `mod` says. The cache key includes the file's path and the targets' generation (which moves with the root).
- Not covered: a Save As without an edit has no test of its own yet.
