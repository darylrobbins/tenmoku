---
task_uid: "01m7j6e84mz591xeq01csgae9p"
task: "T1909"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "A tag push also publishes Tenmoku-\u003cv\u003e-windows-x64-Setup.exe, Tenmoku-\u003cv\u003e-windows-arm64-Setup.exe and Tenmoku-\u003cv\u003e-linux-x86_64.AppImage with .sha256 files and the Velopack feed files per OS/arch/channel; workflow_dispatch builds them as artifacts and publishes nothing"
    required: true
    manual: false
  - id: "C2"
    text: "velopack_update tests cover Install::locate (or auto-locate) for the Windows layout and the AppImage, the feed URL per OS and arch, and refusal of an unsigned feed on every platform"
    required: true
    manual: false
  - id: "C3"
    text: "The Linux AppImage launches under Xvfb in CI and stays alive 5 s with no panic; test-release-workflow.sh checks the vpk steps and passes with actionlint clean"
    required: true
    manual: false
  - id: "C4"
    text: "A workflow_dispatch run on the lane branch is green and the decision file links it; docs/release/platforms.md covers installing, updating and uninstalling on each OS and the unsigned warnings"
    required: true
    manual: false
---

# Installers: Velopack Setup.exe (per-user, Start menu, uninstall) for Windows x64 and ARM64 and an AppImage for Linux x86_64, with Velopack Check for Updates on those platforms

The user (2026-11-16): "And let's make it easy to install. So are we creating an installer? What process are we using to make it easy to set up?"

Velopack is the installer and updater: the T1427 spike (docs/decisions/mh0vr0a5jj-velopack-spike.md, scripts/velopack-pack.sh, crates/fastedit/src/velopack_update.rs) proved it on macOS, and § 10 lays out Windows and Linux.

## Change (builds on T1908's per-platform release jobs)
- **Windows x64 and ARM64:** `vpk pack` makes `Tenmoku-<v>-windows-<arch>-Setup.exe`: a per-user install to %LocalAppData%\Tenmoku with no admin prompt, a Start menu shortcut, an Add/Remove Programs entry and uninstall; plus vpk's portable zip (replacing T1908's hand-made zip if equivalent). The install hooks (`--veloapp-install`/`--veloapp-updated`) are already handled by `startup()`; `Install::locate` learns the Windows layout (Update.exe beside current\) or uses velopack's auto-locate there.
- **Linux x86_64:** `vpk pack` makes `Tenmoku-<v>-linux-x86_64.AppImage` (single file, chmod +x and run, self-updating through UpdateNix via $APPIMAGE), alongside T1908's tar.gz.
- **Updates on Windows and Linux:** Check for Updates uses the Velopack feed for that OS and arch, signed with our Ed25519 key as on macOS (the `.sig` check in front); the feed files (releases.<channel>.json, .nupkg) are published as release assets by the publish job. Use a `stable` channel per OS/arch. Until T1368's key secret exists, the job builds unsigned feeds and the app's update check refuses them with its existing "unsigned feed" message (no silent unsigned updates).
- macOS stays on Sparkle and the zip in this card.
- vpk installed in the jobs at the crate's pin (dotnet tool). Authenticode signing is a separate card (needs a certificate from the user); unsigned Setup.exe shows SmartScreen's "Windows protected your PC", which docs/release/platforms.md explains.

## Testing
velopack_update tests for the Windows and Linux locate/feed selection (headless); the release workflow test checks the vpk steps; a workflow_dispatch run produces Setup.exe for both arches and the AppImage; the AppImage launches under Xvfb in CI. A human install check on a real Windows machine is a separate human card the lead files.

A workflow edit: needs the user's risk approval at review.
