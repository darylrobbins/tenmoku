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

workflow_dispatch of `release.yml` at df3bca55f on `lane/n96kb4s26j`:
<https://github.com/darylrobbins/tenmoku/actions/runs/43873849643>. Green: version,
macos-arm64, windows-x64, windows-arm64, linux-x86_64; publish skipped (dispatch). Each smoke
step printed `tenmoku 0.2.0`; Linux logged "the app is alive after 5 s with no panic" (GPUI
created its X11 window); the Windows checks matched 8664 and AA64.

| Artifact | File | Size (bytes) | SHA-256 |
| --- | --- | --- | --- |
| Tenmoku-0.2.0-macos-arm64 | Tenmoku-0.2.0-macos-arm64.zip | 34,525,897 | `3ef8adee78b260a4433157e3f9a98dfe115c8c7dfe1a25ddb4a3a76a1fc4e6e2` |
| Tenmoku-0.2.0-windows-x64 | Tenmoku-0.2.0-windows-x64.zip | 41,951,891 | `7c09d495e2673fa875e4abf61993026e671ab81088d0bc93705aae9c2be706d1` |
| Tenmoku-0.2.0-windows-arm64 | Tenmoku-0.2.0-windows-arm64.zip | 37,774,931 | `2256b6c33905dbc494c80ea79b8f1333321ae8812b4fa4d3b4237e7ac8a2e667` |
| Tenmoku-0.2.0-linux-x86_64 | Tenmoku-0.2.0-linux-x86_64.tar.gz | 44,964,528 | `25aa05b0c8caff35d8245d3fb577ba3dfcfb2a1072a329479a53295aeb232774` |

Each `.sha256` checks (`shasum -a 256 -c`) against its downloaded file. Unpacked, tenmoku.exe is
119 MB (x64) and 101 MB (arm64). The Linux binary links only libxcb, libxkbcommon(-x11), libgcc_s,
libm, libc and ld-linux; Vulkan is loaded at run time.

## Left for other cards

Authenticode and Developer ID signing, installers (MSI/winget, .deb/AppImage/Flatpak), Linux
arm64, and a launch check on Windows. The version bump to 0.2.1 and the tag are the release
itself, after this lands.
