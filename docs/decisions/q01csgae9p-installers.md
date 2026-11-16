# Installers: Velopack Setup.exe and AppImage (T1909)

The user (2026-11-16): "let's make it easy to install. So are we creating an installer?"

Yes: on Windows and Linux, Velopack (`vpk`, at the velopack crate's pin, 1.3.39-g8d2844f) makes
the installer and does the updates, as the T1427 spike
([mh0vr0a5jj-velopack-spike.md](mh0vr0a5jj-velopack-spike.md), § 10) laid out. macOS stays on
Sparkle and the zip. How users install, update and uninstall:
[docs/release/platforms.md](../release/platforms.md).

**Proof run:** workflow_dispatch of `release.yml` on `lane/q01csgae9p`:
<https://github.com/darylrobbins/tenmoku/actions/runs/RUN_ID> (green; artifacts only, nothing
published).

## What a release now carries

| File | Made by | Notes |
| --- | --- | --- |
| `Tenmoku-<v>-windows-x64-Setup.exe`, `-arm64-` | `scripts/velopack-pack.ps1` | per user into `%LocalAppData%\Tenmoku`, no admin prompt; Start menu shortcut; Add/Remove Programs entry |
| `Tenmoku-<v>-linux-x86_64.AppImage` | `scripts/velopack-pack.sh --linux` | one file; UpdateNix replaces it in place through `$APPIMAGE` |
| a `.sha256` beside each | the same scripts | `<hash>  <name>`, LF, as T1908's |
| `releases.windows-x64-stable.json`, `releases.windows-arm64-stable.json`, `releases.linux-x86_64-stable.json` | vpk | the feeds the app reads |
| `Tenmoku-<v>-<channel>-full.nupkg` | vpk | the full package each feed lists |
| `releases.<channel>.json.sig` | `sign_update_feed`, in the pack scripts | only when signed (below) |

T1908's zip and tar.gz stay. vpk's own portable zip is not made (`--noPortable`): its layout
(`Update.exe`, `current\`) is not T1908's folder, and the release would carry two portable
Windows zips. vpk's `assets.<channel>.json` and legacy `RELEASES-<channel>` are not published:
the app reads neither.

## Decisions

1. **A channel per OS and arch, named as the downloads:** `windows-x64-stable`,
   `windows-arm64-stable`, `linux-x86_64-stable`. All three feeds and their packages sit in one
   GitHub Release without clashing, and the feed base is the same for all of them,
   `https://github.com/<repo>/releases/latest/download` (overridable by the `TENMOKU_UPDATE_FEED`
   repository variable), baked in at build time. The app names its channel itself
   (`release_channel(os, arch)`, passed to velopack as `ExplicitChannel`) rather than trusting
   the manifest's: an x64 build on an ARM PC still reads the x64 feed. Downgrades stay off.
   macOS has no release channel; the spike reads the one it was packed with.
2. **`Install::locate` learns both layouts** instead of using velopack's auto-locate, so the
   macOS symlink handling stays and every layout is tested headless on any host
   (`Install::locate_at(os, arch, exe, $APPIMAGE, $HOME)`):
   - Windows: the binary in `<root>\current\` with `sq.version`, and `<root>\Update.exe`;
     packages in `<root>\packages`; portable when `<root>\.portable` exists. The exe path is not
     canonicalized there (its `\\?\` prefix is not what velopack's locator sees).
   - Linux: the binary in `<mount>/usr/bin/` with `UpdateNix` and `sq.version`, and `$APPIMAGE`
     an absolute path to a file; the AppImage is the root; packages in
     `/var/tmp/velopack/<id>/packages`, as velopack's own locator.
   `startup()` is unchanged: it now runs Setup.exe's `--veloapp-install`/`--veloapp-updated`
   hooks on Windows too, before the CLI.
3. **Signing: both keys or no Velopack updates.** The feeds are signed in the pack step when the
   `TENMOKU_UPDATE_PRIVATE_KEY` secret is set, with the `TENMOKU_UPDATE_PUBLIC_KEY` variable
   (also baked into the app) checked against the signer's output. Until T1368's key exists
   neither is set: the feeds go out unsigned, and the app is built with no public key, so its
   policy is off ("this build has no … TENMOKU_UPDATE_PUBLIC_KEY") and Check for Updates… falls
   to the GitHub release check, which offers the download page. No unsigned feed is ever applied.
   An app that has the key refuses an unsigned feed with the existing refusal ("the feed has no
   signature: releases.<channel>.json.sig could not be read"), tested for every platform's
   layout and channel. A private key without the public one fails the pack (signing would be for
   nothing); a public key without the private one packs with a warning, as that build refuses
   its own feed. An empty variable counts as absent (`Policy::decide`).
4. **Publish guard:** as for `appcast.xml`, a rerun that made no `releases.*.json.sig` over a
   release that has one is refused, since the new feed under the old signature fails every check.
5. **The feed signer is built with the app** (`--bin tenmoku --example sign_update_feed`): a few
   seconds of linking, and the pack step needs no second cargo run when the secret appears.
6. **CI proves the installers, not just the packing.** Windows: `Setup.exe --silent`, then the
   install under `%LOCALAPPDATA%\Tenmoku` (`current\tenmoku.exe`, `Update.exe`), `--version`,
   the Start menu shortcut, the HKCU uninstall entry with the version, and its
   `QuietUninstallString` run until the binary, shortcut and entry are gone. Linux: the
   AppImage's `.sha256`, `--version`, and 5 s alive under Xvfb with no panic (FUSE from
   `fuse3`/`libfuse2t64`, mksquashfs from `squashfs-tools`).
7. **Not code signed.** Authenticode needs a certificate from the user (a separate card);
   SmartScreen's "Windows protected your PC" and how to pass it are in platforms.md. AppImages
   have no OS signature; the feed signature is the only check there.

## Known limits

- A refused Velopack check (an unsigned feed under a keyed build, a network error) is only
  logged: the spike's follow-up 3 (toasts for refusals and progress) still applies.
- No delta packages (`--delta None`): each pack starts in a fresh directory, so vpk has no
  previous full package to diff against, and the spike's § 4 concern about rebuilt fulls stays
  moot. A follow-up can download the previous release's `.nupkg` before packing.
- The packages directory used for the post-download hash check is the layout's; on Windows,
  velopack falls back to `%LocalAppData%\<id>\packages` if the root is not writable, and a
  per-user install's root always is. A mismatch would fail the check closed (nothing applied).
- A human install on a real Windows PC (SmartScreen, the Start menu, an update between two
  signed releases) is a separate human card.
