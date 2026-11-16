# Release builds for every platform (T1908)

## Decisions

- **One job per platform, then one publish job.** `version` checks the tag against
  `Cargo.toml` and hands the version on; `macos-arm64` (macos-latest), `windows` (a matrix:
  x64 on windows-2025, arm64 on windows-11-arm, shown as `windows-x64` and `windows-arm64`) and
  `linux-x86_64` (ubuntu-24.04) each build, pack, smoke-test and upload their files as an
  artifact on every run; `publish` needs all of them, runs on tag pushes only, downloads the
  artifacts into `dist/` and runs the earlier create/upload step unchanged (appcast guard
  included). It is the only job with `contents: write` and the only one that runs `gh release`,
  so a dispatch publishes nothing.
- **macOS output unchanged.** The macOS steps are today's minus the tag check (now in `version`)
  and the publish step (now in `publish`), plus a `--version` smoke step. Its artifact name and
  contents (zip, `.sha256`, `appcast.xml` when the secret exists) are as before.
- **Windows targets named explicitly** (`--target x86_64-pc-windows-msvc` /
  `aarch64-pc-windows-msvc`), so an emulated x64 toolchain on the ARM runner could not silently
  ship x64. The smoke step reads the PE header's machine field (8664 / AA64) as well as running
  `--version`. The ARM image had rustup 1.29.3 preinstalled; the install-if-missing step stays as
  a guard. No launch check on Windows: `--version` only, as the spec allows.
- **Portable archives, one folder each.** `scripts/package-windows.ps1` and
  `scripts/package-linux.sh` lay out the binary plus `Licenses/` (the texts `bundle-macos.sh`
  ships, fonts' OFL under `Licenses/fonts/<family>/`); the Linux one adds
  `dev.tenmoku.tenmoku.desktop` (the Wayland app id and X11 WM class), the SVG icon and a
  per-user `install.sh` (`~/.local/bin` link, desktop entry, icon; `--uninstall`).
  `test-release-workflow.sh` checks each `ship` line of `bundle-macos.sh` appears in both
  scripts. The Windows zip is written with .NET's `ZipFile`, not `Compress-Archive`, so entry
  paths use `/`.
- **Smoke checks run the packed binary**, extracted from the archive, not `target/`: what ships
  is what is tested. Linux also launches it under Xvfb with Mesa's software Vulkan for 5 s and
  fails if it exited or its log says `panicked`.
- **Minimum glibc** is the newest `GLIBC_` symbol version in the binary (objdump -T), output by
  the Linux job and written into the release notes (`--notes`, prepended to the generated
  notes). On ubuntu-24.04 that is 2.39, the runner's own.

## Proof run

workflow_dispatch of `release.yml` at e1cb2320d on `lane/n96kb4s26j` (after review round 1):
<https://github.com/darylrobbins/tenmoku/actions/runs/43885753229>. Green: version,
macos-arm64, windows-x64, windows-arm64, linux-x86_64; publish skipped (dispatch). Each smoke
step printed `tenmoku 0.2.0`; Linux logged "the app is alive after 5 s with no panic" (GPUI
created its X11 window); the Windows checks matched machines 8664 and AA64, and dumpbin listed
no Visual C++ runtime import. The first run, before the static runtime, was
<https://github.com/darylrobbins/tenmoku/actions/runs/43873849643> (also green).

| Artifact | File | Size (bytes) | SHA-256 |
| --- | --- | --- | --- |
| Tenmoku-0.2.0-macos-arm64 | Tenmoku-0.2.0-macos-arm64.zip | 34,549,596 | `11ddf21fc3037baedaf4492f0b49e5bb82c8f1c8649b05fd66cf11e521528efc` |
| Tenmoku-0.2.0-windows-x64 | Tenmoku-0.2.0-windows-x64.zip | 42,131,959 | `b4c430b2198197e90e8519e5764ec8ba2944aa9d53343e3803f07bfa6251a082` |
| Tenmoku-0.2.0-windows-arm64 | Tenmoku-0.2.0-windows-arm64.zip | 37,905,403 | `d9b9e4651865cb68eaa2195398885d5efb16ecd0562535323b909b0983f412bf` |
| Tenmoku-0.2.0-linux-x86_64 | Tenmoku-0.2.0-linux-x86_64.tar.gz | 44,988,832 | `adb18664e063d07a83256be204f51e7ed4c71b9442d518d6ceda8b6bb338c361` |

Each `.sha256` checks (`shasum -a 256 -c`) against its downloaded file. Unpacked, tenmoku.exe is
119 MB (x64) and 101 MB (arm64). The Linux binary links only libxcb, libxkbcommon(-x11), libgcc_s,
libm, libc and ld-linux; Vulkan is loaded at run time.

## Review round 1

- **F1, Windows C runtime.** The first builds imported the Visual C++ runtime, which the zip
  did not carry and a fresh Windows lacks. The Windows build now sets
  `RUSTFLAGS=-C target-feature=+crt-static` (cc and cmake follow it for the C and C++
  dependencies; with `--target`, build scripts are unaffected), and the smoke step fails if
  dumpbin lists a vcruntime, msvcp, concrt, vcomp, ucrtbase or api-ms-win-crt import. x64's
  imports are now all system DLLs: kernel32, advapi32, user32, gdi32, d3d11, dxgi, dcomp,
  dwrite, uiautomationcore, ole32/oleaut32/combase, shell32, comctl32, dwmapi, winmm, imm32,
  icuuc (Windows' own ICU, Windows 10 1903 and later), userenv, ws2_32, bcrypt,
  bcryptprimitives, crypt32, psapi and api-ms-win-core/shcore sets. Static linking over
  shipping the redistributable DLLs app-locally: one file, nothing to keep in step.
- **F2, install.sh escaping.** The folder path is no longer a sed replacement: install.sh
  escapes it as the Desktop Entry spec requires (``" ` $ \`` backslashed in the quoted argument,
  then backslashes doubled, `%` as `%%`), writes the entry with awk to a staged file, and changes
  nothing until that succeeds. The test installs from a folder named
  ``A b&c|d"e'f\g%h$i`j`` and checks the exact Exec line and the link.

## Left for other cards

Authenticode and Developer ID signing, installers (MSI/winget, .deb/AppImage/Flatpak), Linux
arm64, and a launch check on Windows. The version bump to 0.2.1 and the tag are the release
itself, after this lands.
