---
task_uid: "01m6rmsv8drwkm9pfsbkrd2gzx"
task: "T1416"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "regatta.toml: [pipeline] gate is fmt, vet, test, no_gpui, syntax_features, deny (no build); the test stage has no needs; post_merge is still build then test_all with test_all needing build"
    required: true
    manual: false
  - id: "C2"
    text: "rgt config validation accepts the file, and the resolved fast pipeline is fmt, vet, test with no build stage (paste the command and its output in the finish notes)"
    required: true
    manual: false
  - id: "C3"
    text: "TESTING.md says the gate and fast runs no longer build bins, examples or benches, that post_merge does, and when to run the full build locally"
    required: true
    manual: false
---

# Gate speed-up: drop the build stage from the gate and from rgt ci run --fast (post_merge keeps it)

## Problem

The gate runs `build` (`cargo build --workspace --all-targets`, ~280 s quiet, 290-526 s under load) before `test`, and `test` declares `needs = ["build"]`, so `rgt ci run --fast` (`fast = ["fmt","vet","test"]`) silently pulls `build` in as well. Recorded stage runs (lead's bench, 2026-11-06): a quiet gate is ~635 s (fmt 5, vet 107, build 280, test 236, no_gpui 6); a fast run is ~12 min under load, of which ~4.6 min is the pulled-in build. The Mac runs ~55 gates and ~25 fast runs a day. `vet` already type-checks every target (`clippy --workspace --all-targets`), and `test` compiles what it runs; `build` adds only codegen and linking of bins, examples and benches that no gate test runs. The user approved pursuing gate speed-ups on 2026-11-06.

## Change

1. `regatta.toml`: remove `"build"` from `[pipeline] gate`, and remove `needs = ["build"]` from the `test` stage. Keep `build` in `post_merge` (and keep `test_all`'s `needs = ["build"]` there), so a link error in a bin, example or bench still fails main's post-merge check. Leave `fast`, `CARGO_BUILD_JOBS` (6) and `host_pipeline_slots` (2) alone: 12 jobs measured no faster under load.
2. Check with `rgt config` (its validate/inspect subcommand) that the edited file is accepted and that `fast` no longer resolves to a build.
3. `TESTING.md`: in the gate section, say the gate and `rgt ci run --fast` no longer build bins, examples or benches; post_merge does, so a link-only failure shows up there; run `scripts/ci-env.sh cargo build --workspace --all-targets` locally when changing a build.rs, a bin's link setup or an example.

## Out of scope

Regatta's worker prompt that asks for `rgt ci run --fast` (Regatta source). Load-related flake retries (~40% of 11-05's gates were re-gates) are a separate problem.
