---
task_uid: "01m796jyzkt7pxe363ght4k3wn"
task: "T1756"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "ci.yml check job has a guarded cleanup step with df -h before and after"
    required: true
    manual: false
  - id: "C2"
    text: "ci.yml still parses as valid YAML and the linux and windows jobs are unchanged"
    required: true
    manual: false
---

# CI macOS check job: free runner disk before building (ld errno=28)

CI `check` (macOS runner, macos-15) fails linking with `ld: write() failed, errno=28`: the runner
disk is full. The linux job already frees disk with a "Free runner disk" step (ci.yml ~line 57);
the macOS job has none, and its debug test build plus the screenshots examples are big.

Change .github/workflows/ci.yml, `check` job only, with the smallest effective fix:
 1. Add `df -h /` before and after a cleanup step so the margin is visible, removing
    preinstalled things this job never uses (e.g. `sudo rm -rf /Applications/Xcode_*.app` other than
    the selected one is risky; prefer Android/dotnet/hostedtoolcache removals that exist on macos-15:
    ~/Library/Android, /Users/runner/hostedtoolcache subsets, simulators via
    `xcrun simctl delete all`). Guard each rm with `|| true`.
 2. If that is not enough, run the macOS job with CARGO_PROFILE_DEV_SPLIT_DEBUGINFO unchanged but
    `CARGO_INCREMENTAL=0` and, only in CI, `CARGO_PROFILE_DEV_DEBUG=0` (Cargo.toml's dev profile has
    debug = "line-tables-only" and split-debuginfo = "unpacked"). Keep local profiles untouched.

NEEDS HUMAN RISK APPROVAL: workflow edits require it in this repo. Only the real macOS runner proves
it; the YAML can be validated locally (`actionlint` if installed, otherwise parse with yq/python).
