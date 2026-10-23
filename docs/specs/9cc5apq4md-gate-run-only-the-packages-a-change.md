---
task_uid: "01m5fzymcjdsas2f9cc5apq4md"
task: "T1073"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/ci-test-affected.sh (POSIX sh, executable) takes the base from FASTEDIT_TEST_BASE or else `git merge-base HEAD main`; it lists changed files with `git diff --name-only --no-renames \u003cbase\u003e HEAD` (committed state only), maps each to a workspace package from `cargo metadata --format-version 1 --locked --offline` (a file belongs to the package whose manifest directory is its longest prefix; the root package only owns src/, tests/, benches/, examples/ and build.rs), adds every transitive reverse dependency among workspace members (normal, dev and build dependencies), and execs `cargo nextest run --profile ci -p \u003cpkg\u003e ...` with each affected package; it prints the chosen set and the reason to stderr"
    required: true
    manual: false
  - id: "C2"
    text: "It falls back to `cargo nextest run --workspace --profile ci` (and says why) when: no base can be found, the base equals HEAD, the diff is empty, jq or cargo metadata is unavailable, or any changed path is workspace-wide: /Cargo.toml, /Cargo.lock, /rust-toolchain*, /.cargo/**, /.config/**, /scripts/**, /regatta.toml, /deny.toml, /clippy.toml, /rustfmt.toml, or any path outside every package other than docs/** and *.md at the root; extra arguments after the script name are passed to nextest"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/test-ci-test-affected.sh is a self-test run by hand and by this task, using a throwaway git repo with a two-crate cargo workspace fixture (a depends on b) built under $TMPDIR; it asserts, from the script run with FASTEDIT_TEST_BASE and a dry-run switch (FASTEDIT_AFFECTED_DRY_RUN=1 prints the nextest command instead of running it): a change in b selects -p b -p a; a change in a selects only -p a; a doc-only change selects the empty set and runs nothing (or the smallest sensible set, state which in the script header); a Cargo.lock change, a regatta.toml change and an unknown base each select --workspace"
    required: true
    manual: false
  - id: "C4"
    text: "regatta.toml: component command test becomes `scripts/ci-env.sh scripts/ci-test-affected.sh`; test_all (from the slow-tests task) and test_one stay `--workspace`, so post_merge keeps the full workspace run; JUnit still lands at target/nextest/ci/junit.xml (the script passes --profile ci and no other profile); `rgt config check --strict` passes"
    required: true
    manual: false
  - id: "C5"
    text: "rgt ci run --gate --clean, on a commit that changes only one leaf crate, passes and its test.fastedit.report.json contains tests only from that crate and its reverse dependencies, with the report in the same place as before; on a commit that changes Cargo.lock it contains the whole non-slow workspace (compare events counts and state them in the commit message)"
    required: true
    manual: false
  - id: "C6"
    text: "TESTING.md Running section names scripts/ci-test-affected.sh and scripts/test-ci-test-affected.sh with one line each, and adds the rule that a task criterion or --regression-test must name a test in a crate the task changes (or one of its reverse dependencies), because the gate runs only those crates and Regatta refuses a criterion whose test did not run in the gate"
    required: true
    manual: false
---

# Gate: run only the packages a change affects and their reverse dependencies (scripts/ci-test-affected.sh), the full workspace in post_merge

Why: with the slow set gone (previous task), the gate's test stage still runs all non-slow tests of every crate on every change. The user approved on 2026-10-21 (gate-policy changes) testing only the affected crates in the gate.

What Regatta gives a stage command (from regatta's source, do not change Regatta):
- No base env var. A stage command's environment has only the toolchain's environment plus REGATTA_PIPELINE_UNIT and REGATTA_PIPELINE_RUNNER (internal/pipeline/stages.go:39,57; runner.go:533); the base sha is used only to evaluate `when.paths` (service/pipeline.go:437-440) and never exported. The command runs in the snapshot, a detached git worktree of the candidate's integrated commit under .worktrees/.verify/<id> that shares the primary repo's object database and refs (pipeline/snapshot.go "a fresh detached worktree of a commit"). In it `main` resolves, and the candidate was integrated onto base_sha, so `git merge-base HEAD main` is base_sha even when main has since moved. That is the script's default, with FASTEDIT_TEST_BASE as an override and --workspace as the safe fallback. (A separate request to the user: have Regatta export REGATTA_BASE_SHA; the script should read it first once it exists.)
- Post-merge runs on the landed tree, where HEAD is main, so merge-base equals HEAD; the script then runs the workspace. But post_merge uses the `test_all` verb anyway (previous task), which never calls the script.
- Stages are identified by their verb's command (regatta.toml [components.commands]); gate and fast share `test`. The fast pipeline thus also runs only affected packages.
- Report: Regatta reads the stack's JUnit at target/nextest/ci/junit.xml (the store dir in .config/nextest.toml and `--profile ci`); `-p` selection does not change it. The script must keep `--profile ci` and must not set another profile or --message-format.
- Criteria: `rgt finish` refuses evidence `{"test": name}` when the test is absent from the gate's reports (regatta internal/evidence/criteria.go:281, checkTest at :267) and `--regression-test` names run through test_one at the base and with the fix. test_one stays `--workspace` with `--ignore-default-filter`, so regression-at-base and flake reruns are unaffected. A criterion test lives in a crate the task changes in nearly every case; the TESTING.md rule covers the rest.
- Policy: the candidate is judged by regatta.toml at its base_sha (regatta docs/guide/concepts.md:364), so the new `test` command applies to gates of tasks based after this lands, and this task's own gate runs the old one. regatta.toml changes always need the user's approval.

Implementation notes:
- Workspace-wide files: anything that can change what every crate builds or tests (see criteria). A change under a package's Cargo.toml maps to that package and its reverse dependencies; kiln/ crates are ordinary workspace members.
- Build-script, golden and fixture files under a crate map to that crate. Cross-crate fixtures read through include_str!/path "../" get the reverse-dependency treatment only if cargo knows them; if a crate reads another crate's files, list that in the script's workspace-wide table with a comment.
- Package names with `-p` come from cargo metadata, not from directory names (directories like crates/fe-lsp match, kiln/ ones may not).
- Keep it POSIX sh with `set -eu`, jq-based (command -v jq guarded; no jq means --workspace), and fast (one cargo metadata call, offline).

Mac-only: Mac gate policy; the Linux GitHub jobs keep `cargo nextest run --workspace` and are untouched.

## Implementation notes

- A docs-only change (docs/** and root *.md alone) runs fe-text's tests
  (FASTEDIT_AFFECTED_DOCS_PACKAGE), not nothing: Regatta's runner fails a JUnit
  stage whose report is missing or has no test cases
  (internal/pipeline/runner_junit_test.go, TestRunnerJUnitWithoutUsableReportIsAnError).
- THIRD_PARTY.md is workspace-wide: fe-syntax's tests/indent.rs reads it. The other
  cross-crate reads (fe-workbench of fe-markdown's fixtures, fe-gpui of fe-config's
  themes, fastedit of fe-syntax's fixtures) are covered by the dependency graph.
- C5 runs (2026-10-22). `rgt ci run --gate --commit` judges a commit by regatta.toml at its
  base (main, d96281b), so both snapshot gates ran the old `--workspace` command (4732 tests
  each). The new command was exercised with `rgt ci run --stage test` in this lane, detached at
  probe commits on top of e4c5039 with FASTEDIT_TEST_BASE=e4c5039 (the stage environment
  passes it through):
  - fe-plugin only (5a00101, a leaf crate): `-p fe-plugin`, 1 test, report
    test.fastedit.report.json with 1 event (run 01m5gyvqkmgs8h56fqkt75cppa).
  - Cargo.lock only (be11142): "Cargo.lock is workspace-wide", 4732 tests across 32
    packages, 4732 events, the same count as the old gate (run 01m5h09q4ra548fntbntw04sng).
- Review F1: fastedit's tests/third_party.rs reads every .rs, .scm and .toml file under
  crates/ and kiln/, and fe-plugin and kiln-plugin are not among its dependencies. Rather than
  making those two crates workspace-wide, any such file also selects fastedit (a leaf, so it adds
  only itself), which covers crates added later too.
- After F1, a fastedit-only probe (13055bf on 46ad321, FASTEDIT_TEST_BASE=46ad321, worktree
  `rgt ci run --stage test`): `-p fastedit`, 46 tests, 46 events, all fastedit (run
  01m5h499dpsnjva1w823ja9hyq), against 4732 for the Cargo.lock probe.
- C5's clean snapshot gate with the new command can only run once this lands: the gate takes its
  commands from regatta.toml at the candidate's base (concepts.md "trusted policy snapshot"), and
  until then `git merge-base HEAD main` is a main without this task, whose diff includes scripts/
  (workspace-wide). Check it on the first leaf-only task gated after landing.
- Review F2, clean gates under the new policy: a scratch clone (`git clone --shared`) whose
  `main` is this task's head 45c08f5, registered with `rgt init` against its own control DB
  (XDG_STATE_HOME in a scratch dir, so the real one is untouched), ran `rgt ci run --gate
  --commit` with no FASTEDIT_TEST_BASE or REGATTA_BASE_SHA. Base 45c08f5 in both manifests, so
  the new test command; the script found it as `git merge-base HEAD main`.
  - fastedit-only (6f4b6f4): gate passed; "changed: fastedit; with reverse dependencies:
    fastedit", 46 tests, test.fastedit.report.json with 46 events, all fastedit.
  - Cargo.lock only (725201f): gate passed; "Cargo.lock is workspace-wide", 4745 tests across
    32 packages, 4745 events (main had grown from 4732 since the earlier runs).
