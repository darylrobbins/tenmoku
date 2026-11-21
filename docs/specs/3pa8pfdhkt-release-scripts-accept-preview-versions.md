---
task_uid: "01m7th2y8bycbsjb3pa8pfdhkt"
task: "T1996"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "`scripts/check-release-version.sh vX.Y.Z-preview.N` accepts a tag equal to `v` plus Cargo.toml's `[workspace.package] version` when that is `X.Y.Z-preview.N`, and still requires release-notes/\u003cversion\u003e.md; it refuses `v0.3.0-preview`, `v0.3.0-preview.01`, `v0.3.0-rc.1` and a tag whose version is not Cargo's. New tests/script scripts/test-check-release-version.sh cover each (it must run with sh on Linux)."
    required: true
    manual: false
  - id: "C2"
    text: "`scripts/bundle-macos.sh --version 0.3.0-preview.2` writes CFBundleShortVersionString `0.3.0-preview.2` and a CFBundleVersion that Sparkle orders correctly: decide it by reading Sparkle's SUStandardVersionComparator (scripts/fetch-sparkle.sh fetches the framework) and record the evidence in the finish note. Default, if Sparkle treats the `-preview.N` suffix as older than the bare version: CFBundleVersion is the full string. If it does not, use a numeric build version that sorts below the stable (e.g. X.Y.Z.0.N is NOT acceptable because it sorts above X.Y.Z; derive and test the right scheme) and keep the short string for display. scripts/test-bundle-macos.sh pins the choice with the cases preview.1 \u003c preview.2 \u003c stable and previous-stable \u003c preview.1."
    required: true
    manual: false
  - id: "C3"
    text: "scripts/draft-release-notes.sh takes the range since the previous tag of either kind and writes release-notes/\u003cversion\u003e.md for a preview version; its test covers it."
    required: true
    manual: false
---

# Release scripts accept preview versions: tag check, bundle version, notes (channels 4)

# Release scripts: preview versions

Files read first: scripts/check-release-version.sh (the dotted-only rule and notes check), scripts/bundle-macos.sh (--version, the Info.plist writes), scripts/draft-release-notes.sh, and their test-*.sh. scripts/make-appcast.sh also insists on X.Y.Z and is CH5's. Cargo accepts `0.3.0-preview.2` as a workspace version; check that `cargo metadata`/the version job's Cargo.toml parse in release.yml tolerates it (CH9 owns the workflow).


Read the files named before starting. Do not widen --scope; if a file outside it is needed, stop and respec. New tests go in new files.

## Implementation notes

- Sparkle 2.10.1's SUStandardVersionComparator (compiled and run against the fetched framework) ignores everything from a `-` on: `0.3.0-preview.1`, `0.3.0-preview.2` and `0.3.0` all compare equal, so the full string orders nothing. `0.3.0.0.2` sorts above `0.3.0`. `0.3.0.preview.N` sorts below `0.3.0`, orders N numerically (preview.9 < preview.10) and above `0.2.9`. So CFBundleVersion is `X.Y.Z.preview.N`; CFBundleShortVersionString is the string as given. test-bundle-macos.sh pins it, and runs the real comparator when clang and a fetched Sparkle.framework exist (SPARKLE_FRAMEWORK or target/sparkle).
- draft-release-notes.sh gained `--version V` (writes release-notes/V.md) and now starts at the nearest v* tag behind HEAD (`git describe`), since version sort puts v0.12.0-preview.1 after v0.12.0.
- Carried T1614 F1 (Tabler LICENSE not bundled) not fixed: scripts/test-release-workflow.sh requires package-linux.sh and package-windows.ps1 to carry every `ship` line of bundle-macos.sh, and both are outside scope. Needs a respec covering those two files.
