---
task_uid: "01m7j6dhzfah308zn96kb4s26j"
task: "T1908"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "A tag push builds macos-arm64 (outputs unchanged), windows-x64, windows-arm64 and linux-x86_64 in separate jobs and publishes Tenmoku-\u003cv\u003e-macos-arm64.zip, Tenmoku-\u003cv\u003e-windows-x64.zip, Tenmoku-\u003cv\u003e-windows-arm64.zip and Tenmoku-\u003cv\u003e-linux-x86_64.tar.gz, each with a .sha256, from one publish job; workflow_dispatch uploads the four as artifacts and publishes nothing"
    required: true
    manual: false
  - id: "C2"
    text: "Each build job's smoke step passes: tenmoku --version prints the Cargo version (crates/fastedit/tests/version_flag.rs covers the flag), and the Linux job launches the app under Xvfb and it is alive with no panic after 5 s"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/test-release-workflow.sh checks the new jobs (runners, smoke steps, publish needs, contents: write only on publish, dispatch publishes nothing) and passes with actionlint clean"
    required: true
    manual: false
  - id: "C4"
    text: "A workflow_dispatch run of the lane's release.yml is green on all four jobs; the decision file links it and lists each artifact's size; docs/release/platforms.md says what each download is and how to run it on each OS (unsigned for now: SmartScreen and Gatekeeper notes)"
    required: true
    manual: false
---

# Release builds for Windows x64 and ARM64 and Linux x86_64 beside macOS: release.yml split into per-platform build jobs (portable zip / tar.gz + .sha256, a smoke run of each binary) and one publish job

The user (2026-11-16): "let's go and update the release process. We create Windows builds as well. Let's create Windows for Intel as well as ARM64. And what the hell, let's create a Linux build too. So let's update the release process to build all of these ... let's just do a 0.2.1 that includes those new builds. And let's make it easy to install. ... we can prioritize this work above the other stuff."

## Today
.github/workflows/release.yml has one job on macos-latest: tag check, build, bundle Tenmoku.app, optional sign/notarize, zip + .sha256, optional Sparkle appcast, then `gh release create/upload`. A workflow_dispatch run builds the same files as a workflow artifact and publishes nothing. ci.yml already builds and tests on windows-2025 (x64) and ubuntu-24.04, so the code compiles on both.

## Change
Split release.yml into build jobs and one publish job:
- **macos-arm64**: today's steps, unchanged in output (zip, .sha256, appcast when the secret exists).
- **windows-x64** (windows-2025) and **windows-arm64** (GitHub's windows-11-arm runner): `cargo build --release -p fastedit` for the native target, then a portable zip `Tenmoku-<v>-windows-<arch>.zip` holding tenmoku.exe (the icon is already embedded by build.rs), the licence texts and NOTICE/THIRD_PARTY files the macOS bundle ships, and its .sha256.
- **linux-x64** (ubuntu-24.04): the release binary for x86_64-unknown-linux-gnu in `Tenmoku-<v>-linux-x86_64.tar.gz` with a `.desktop` file and the icon, licences, and its .sha256. Install the system packages ci.yml's Linux job installs; record the minimum glibc (the runner's) in the release notes text.
- Each build job runs a smoke check of its own binary: `tenmoku --version` prints the tag's version (add the flag if it doesn't exist), and the Linux job also launches the app headless under Xvfb for a few seconds and checks it's still running and logs no panic (see the run-fastedit-linux skill for the Xvfb recipe). Windows: `--version` only, unless a launch check is cheap on the runner.
- **publish** (needs all build jobs, tag pushes only): downloads every job's files into dist/ and runs today's create/upload logic once (keep the appcast guard). workflow_dispatch: every build job uploads its artifact and nothing is published.
- Keep the job permissions minimal (contents: write only on publish), pin actions as today, and keep actionlint clean.

## Testing
Extend scripts/test-release-workflow.sh for the new jobs (job names, runners, smoke steps, publish needs, permissions, dispatch publishes nothing). Prove the workflow end to end with a workflow_dispatch run on the lane branch (`gh workflow run release.yml --ref <branch>` after pushing the branch to origin; ask the lead before pushing), and link the green run and its four artifacts in the decision file.

This is a workflow edit, so it needs the user's risk approval at review. Signing (Authenticode, Developer ID) and installers are separate cards.
