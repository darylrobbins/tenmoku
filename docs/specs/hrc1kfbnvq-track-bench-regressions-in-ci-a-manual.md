---
task_uid: "01m55bq4p0vtx290hrc1kfbnvq"
task: "T137"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/test-bench-compare.sh passes: on generated sample.json fixtures, a case whose fastest sample is slower than base by more than the threshold makes bench-compare.sh exit non-zero naming that case, and equal or faster cases exit zero"
    required: true
    manual: false
  - id: "C2"
    text: ".github/workflows/bench.yml is workflow_dispatch only, takes a base ref, builds base and HEAD bench binaries for fe-buffer, fe-display, fe-syntax (10k) and fe-gpui keystroke, runs them alternately, uploads the sample.json files and fails when bench-compare.sh does"
    required: true
    manual: false
  - id: "C3"
    text: "ci.yml, lsp.yml and soak.yml are unchanged; PERF.md's Measuring block names the workflow and the script"
    required: true
    manual: false
---

# Track bench regressions in CI: a manual workflow that compares base and head by fastest sample (Mac T405)

Filed on argon from Mac T405 (card ~/coordination/argon-cards/zzl/T405.json rev 1; note zzl, perf track). Mac task uid 01m4p2hdyhd1hfz7n15et2hv81. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced). Mac lead's note (zzl): build it with the spec's defaults: runner macos-15, a 15% threshold, workflow_dispatch only (each is a one-line change later, and a manual-only workflow runs nothing when it lands). Write both scripts as POSIX sh (#!/bin/sh, like scripts/test-prune-build-cache.sh) so they run under macOS's /bin/sh too; jq is fine. One stale line in the spec: ci.yml now also has a linux job; still leave ci.yml, lsp.yml and soak.yml unchanged. Title the host commit "... (Mac T405)".

**NEEDS A USER DECISION before ready: which runner (GitHub-hosted macos-15 at macOS minute rates on this
private repo, or this Mac as a self-hosted runner), the regression threshold, and how often it runs
(manual only, or also weekly). The defaults below are the drafter's proposal.**

## Problem

The plan's performance section (line 99) says "CI tracks regressions". No workflow runs a bench:
`.github/workflows/ci.yml` runs fmt, clippy, nextest, no-gpui and deny, `lsp.yml` runs real language
servers, and `soak.yml` runs long proptests. Every number in PERF.md is a one-off run, so a regression shows
up only when someone re-runs a bench by hand.

Shared runners are noisy. On this Mac under load, criterion means swung 2–10× between runs of the same
binary. What did give stable base-vs-candidate answers (T272) was the following: build both bench binaries,
run them alternately, and compare the **fastest sample** of each case.

## Change

Proposed defaults, to be confirmed by the user:

- `.github/workflows/bench.yml`, `workflow_dispatch` only, like `lsp.yml` and `soak.yml`. Its input is a
  `base` ref (default `origin/main~1`), and it runs on `macos-15`. It builds the bench binaries for `base`
  and `HEAD` (`cargo bench --no-run --message-format=json`, executables copied out), for `fe-buffer`,
  `fe-display`, `fe-syntax -- 10k` and `fe-gpui --bench keystroke`. It runs them alternately for 3 rounds
  each (`--warm-up-time 1 --measurement-time 3`) and uploads every `sample.json` as an artifact.
- `scripts/bench-compare.sh <base-criterion-dir> <head-criterion-dir> [threshold-percent]` compares the
  fastest sample of each case across all rounds. It prints a table and exits non-zero if any case is slower
  than base by more than the threshold (default 15%). The workflow fails on that exit.
- `scripts/test-bench-compare.sh` tests the compare script on generated `sample.json` fixtures in a temp
  dir, the way `scripts/test-prune-build-cache.sh` tests its script.
- PERF.md's Measuring block names the workflow and the script.

## Implementation notes

- Built with the lead's defaults: `macos-15`, 15% (also a `threshold` input), `workflow_dispatch` only.
- fe-syntax builds only `--bench syntax`: `flat` has no 10k cases and `tree_ratio` is a plain `main`
  that measures tree memory, ignores the filter and writes no `sample.json`.
- Each side's round runs with its own `CRITERION_HOME`, so criterion never compares a run with an
  earlier one; within a round each bench runs on base, then on HEAD. Binaries run from their package
  dir, as `cargo bench` runs them, and both checkouts share one `CARGO_TARGET_DIR`.
- `bench-compare.sh` keys a case by the `full_id` in the `benchmark.json` beside each
  `new/sample.json`, so any number of rounds can sit under a side's dir; a case's time is the minimum
  over all samples of `times[i] / iters[i]`. Cases in only one side are listed (`new`, `gone`) and
  don't fail. Exit 1 on a regression, 2 on bad arguments or no results.
- Checked locally by running the workflow's build, run and compare steps with fe-text's bench in
  place of the four (same binary on both sides, 0.3 s measurements, other lanes loading the machine):
  the pipeline works, and four of 15 cases came out 30–45% "slower". Expect some noise even at 3 s
  and three rounds on a shared runner; the threshold is a one-line change.
