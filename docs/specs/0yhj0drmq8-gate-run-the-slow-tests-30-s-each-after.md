---
task_uid: "01m5fzym0cpbz6np0yhj0drmq8"
task: "T1072"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "In .config/nextest.toml, [profile.ci] has default-filter = a filterset \"not (\u003cslow set\u003e)\" naming the tests in the slow list in the spec (one parenthesised package(...) and test(=...) term each); the list is commented with where the list came from (2026-10-21, gate run 01m5fxp99b1bhcpxe072zx1n1d). [profile.ci.junit] path stays \"junit.xml\" and [store] dir stays \"target/nextest\", so the report still lands at target/nextest/ci/junit.xml"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest list --workspace --profile ci --message-format json lists none of the slow tests, and the same command with --ignore-default-filter lists every slow test (state both counts in the commit message; the difference must equal the number of tests in the slow set, 7 for the sibling list)"
    required: true
    manual: false
  - id: "C3"
    text: "regatta.toml: component command test_one gains --ignore-default-filter so a regression test or a flake rerun that names a slow test still runs it (nextest applies default-filter to -E too); test stays `cargo nextest run --workspace --profile ci`, which now skips the slow set through the profile; a new component command test_all = `scripts/ci-env.sh cargo nextest run --workspace --profile ci --ignore-default-filter` runs everything and writes the same junit.xml"
    required: true
    manual: false
  - id: "C4"
    text: "regatta.toml: a new [[pipeline.stages]] name = \"test_all\" run = \"test_all\" needs = [\"build\"]; [pipeline] post_merge becomes [\"build\", \"test_all\"]; gate and fast are unchanged and still list \"test\"; `rgt config check --strict` passes"
    required: true
    manual: false
  - id: "C5"
    text: "In the lane worktree (which reads this change's own regatta.toml), rgt ci run --gate passes; its test report (jq .tests) contains none of the slow tests and its events count is the then-current total minus the slow set. A --clean snapshot takes policy from the candidate's base, so it cannot show the new policy before this lands; the lead checks the first gate after landing."
    required: true
    manual: false
  - id: "C6"
    text: "In the lane worktree, rgt ci run --stage test_all passes and its report contains every slow test with outcome pass."
    required: true
    manual: false
  - id: "C7"
    text: "TESTING.md, in Running next to the cargo nextest run line, gains a short \"Slow tests\" rule: a test belongs in the profile.ci default-filter slow set when it takes over 30 s on the gate machine in a nextest run, or over 10 s and is not on a path the gate exists to protect; add it by editing the filterset in .config/nextest.toml in the same task that adds the test; first try to make it faster or smaller (see imara Histogram and large-text notes); a test that a task criterion or --regression-test names must NOT be in the slow set, because the gate must run it (Regatta refuses a criterion whose test did not run in the gate); how to run them by hand (cargo nextest run --profile ci --ignore-default-filter, or the unfiltered default profile); and that post_merge runs them"
    required: true
    manual: false
---

# Gate: run the slow tests (>30 s each) after merge only, as a nextest default-filter on the ci profile, with a full run in post_merge

Why: the gate's test stage takes about 257 s of a median 16 min gate, and 44 of 4,622 tests take over 10 s (20 CPU-min of 46). The user approved on 2026-10-21 (gate-policy changes) running the slowest after merge only.

Design, from Regatta's source (do not change Regatta):
- A stage's command is the component's command for its `run` verb, and gate, fast and post_merge share it (regatta.toml `[components.commands]`; stage `run`, `[pipeline]`). So excluding the slow set from `test` excludes it from all three; post_merge needs its own verb with its own stage name. It gets `test_all`, which runs everything in one nextest invocation (one junit.xml; a second stage writing the same target/nextest/ci/junit.xml would overwrite the first, so a separate slow-only stage is not used).
- The exclusion is a nextest `default-filter` on `[profile.ci]`, not a -E on the command, so every caller of `--profile ci` (the gate, `rgt ci run`, the stack's report path target/nextest/ci/junit.xml) is unchanged. `--ignore-default-filter` turns it off.
- `test_one` MUST pass `--ignore-default-filter`: nextest applies the default filter to -E expressions as well, and the regression-at-base overlay and the flake rerun (service/gate_flake_rerun.go rerunTestID) run `test_one` per named test.
- Criteria: `rgt finish` refuses a `{"test": name}` evidence reference whose test is absent from the gate's reports ("test ... did not run in the gate", regatta internal/evidence/criteria.go:281) or skipped. So a test cited by a criterion must not be in the slow set; that is the rule in TESTING.md. `{"stage": "test_all"}` can't be cited either: criteria.go:263 checkStage accepts only gate stages.
- regatta.toml always needs the user's approval (merge.human_paths policy in Regatta) and a test-command change raises the informational test_config_changed flag (regatta internal/evidence/flags.go:162,210-225; human_flags is empty here). The new policy applies to gates run from the base after this lands (the candidate is judged by regatta.toml as committed at its base_sha, regatta docs/guide/concepts.md:364); this task's own gate runs the old policy and so still runs every test.

Slow set (initial): the sibling drafter's list, one nextest filterset per line, to be or'd inside `not (...)` (these names supersede the >30 s fallback from run 01m5fxp99b1bhcpxe072zx1n1d, whose tests were 11: fe-display long_rows_fill_the_view_with_hints_and_the_inlay_anywhere 164 s, fe-vim huge_virtual_counts_stay_bounded 89 s, fe-buffer default_limit_keeps_typing_undoable 89 s, and so on; the sibling's names for the fe-vim ones differ, so check each name exists with `cargo nextest list --workspace -E '<filterset>'` and fall back to the run's report names for any that matches nothing; a filterset that matches no test is a silent no-op):

    package(fe-workbench) and test(=tests::m3_signoff::a_100k_file_project_end_to_end)
    package(fe-project) and test(=tests::finder::a_100k_file_index_bounds_query_work)
    package(fe-project) and test(=tests::large::a_100k_file_tree_loads_lazily)
    package(fe-editor) and test(=a_new_file_saves_on_an_exfat_volume)
    package(fe-vim) and test(=counted_replace_of_regional_indicators_caps_at_max_put)
    package(fe-vim) and test(=huge_virtual_counts_cap_at_max_put)
    package(fe-vim) and test(=a_huge_virtual_count_caps_at_max_put)

Wrap each line in parentheses and or them inside `not (...)`. Keep the existing [[profile.ci.overrides]] and the fsevents test-group untouched: the fe-workbench 100k test is also in the fsevents group, which is harmless when it is filtered out.

Note the test times are from a loaded run (max_parallel = 1, but the Mac was shared); a test that is under 30 s on an idle machine may still qualify, so apply the rule's 30 s measurement from the gate's own JUnit elapsed.

Mac-only: this changes the Mac gate's policy; the Linux CI jobs (GitHub workflows) run `cargo nextest run --workspace` on the default profile and are untouched.
