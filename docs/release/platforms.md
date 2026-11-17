# Downloads by platform

Each GitHub Release of Tenmoku carries one download per platform, each with a `.sha256` beside
it, built by `.github/workflows/release.yml` (one job per platform, then one publish job; see
[macos-signing.md](macos-signing.md), "Releasing"):

| Download | For | What it is |
| --- | --- | --- |
| `Tenmoku-X.Y.Z-macos-arm64.zip` | macOS on Apple Silicon | `Tenmoku.app` |
| `Tenmoku-X.Y.Z-windows-x64-Setup.exe` | Windows 10 (1903 or later) or 11 on Intel or AMD | **the installer**: per user, Start menu, uninstall, updates |
| `Tenmoku-X.Y.Z-windows-arm64-Setup.exe` | Windows 11 on ARM (Snapdragon and other ARM64 PCs) | the same, built for ARM64 |
| `Tenmoku-X.Y.Z-windows-x64.zip`, `-windows-arm64.zip` | the same PCs | a portable folder: `tenmoku.exe` and `Licenses\` |
| `Tenmoku-X.Y.Z-linux-x86_64.AppImage` | Linux on x86_64 with glibc (the release notes name the oldest glibc) | **one file to run**, which updates itself |
| `Tenmoku-X.Y.Z-linux-x86_64.tar.gz` | the same | a portable folder: `tenmoku`, a desktop entry, the icon, `install.sh` and `Licenses/` |

The installers are made by [Velopack](https://velopack.io) (`vpk`), which also does the updates
on Windows and Linux. Its update feeds are release files too: `releases.<os>-<arch>-stable.json`
(`windows-x64`, `windows-arm64`, `linux-x86_64`) and the `.nupkg` each lists; you never download
those by hand. Nothing here needs administrator rights. Every build job runs what it built before
uploading it: `--version` on each binary, a silent install, run and uninstall of each Setup.exe,
and a 5 s launch under Xvfb of the Linux binary and of the AppImage.

**These builds are not signed with a publisher certificate yet** (no Authenticode certificate,
and the macOS app is ad-hoc signed until the Developer ID secrets exist). The OS warns the first
time; the steps below get past that once.

**Updates on Windows and Linux** come through Velopack only when the release's feed is signed
with Tenmoku's update key (a `releases.<...>.json.sig` beside the feed) and the app was built
with that key's public half. Until the key exists, release feeds are unsigned, the app has no key
to accept one, and **Check for Updates…** (in the Tenmoku menu, or from the command palette)
instead asks GitHub for the latest release and offers its download page. An app built with the
key refuses an unsigned or altered feed ("the feed has no signature") and installs nothing from
it.

## Check the download

```sh
shasum -a 256 -c Tenmoku-X.Y.Z-linux-x86_64.tar.gz.sha256   # macOS; sha256sum -c on Linux
```

On Windows, in PowerShell, compare `(Get-FileHash Tenmoku-X.Y.Z-windows-x64-Setup.exe).Hash` (or
the zip's) with the first word of its `.sha256` file (case does not matter).

## macOS

Unzip, and drag `Tenmoku.app` to Applications. **Gatekeeper:** an app without a Developer ID
signature is refused with "Apple could not verify ...". Open it once, close the dialog, then
System Settings > Privacy & Security > **Open Anyway** (on macOS 14 and older, right-click the
app > Open also works). Or, from a terminal:

```sh
xattr -dr com.apple.quarantine /Applications/Tenmoku.app
```

**Update:** through Sparkle (Tenmoku > Check for Updates…) when the release has an `appcast.xml`;
otherwise Check for Updates… offers the GitHub download page. **Uninstall:** drag
`Tenmoku.app` to the Bin; settings stay in `~/.config/tenmoku`.

## Windows

**The installer (recommended).** Run `Tenmoku-X.Y.Z-windows-x64-Setup.exe` (or `-arm64-` on an
ARM PC: the x64 build runs there too, but emulated and slower).

1. **SmartScreen:** "Windows protected your PC" appears, because the installer is not signed with
   a publisher certificate. Click **More info** > **Run anyway**. Your browser may first say the
   file "isn't commonly downloaded": choose **Keep**.
2. It installs for your user only, with no administrator prompt, into
   `%LOCALAPPDATA%\Tenmoku`, adds **Tenmoku** to the Start menu, and starts it.
3. **Update:** Check for Updates… (see above: Velopack once the feeds are signed, the
   GitHub download page until then). Installing a newer Setup.exe over it also updates it.
4. **Uninstall:** Settings > Apps > Installed apps > Tenmoku > **Uninstall** (or Control Panel >
   Programs and Features). That removes the folder, the Start menu shortcut and the entry; your
   settings in `%USERPROFILE%\.config\tenmoku` stay.

**The portable zip.** For no install at all:

1. Before extracting: right-click the zip > Properties > tick **Unblock** > OK. Otherwise every
   file extracted keeps the "downloaded from the internet" mark.
2. Extract it anywhere you can write, such as `%LOCALAPPDATA%\Programs\Tenmoku`, and run
   `tenmoku.exe`. To start it from a terminal as `tenmoku`, add that folder to your user PATH.
3. SmartScreen asks as above, once per new version. To update, extract the new zip in its place;
   to remove it, delete the folder.

The app needs a GPU with Direct3D 11, which every supported Windows PC has.

## Linux

**The AppImage (recommended).** One file, nothing to unpack:

```sh
chmod +x Tenmoku-X.Y.Z-linux-x86_64.AppImage
./Tenmoku-X.Y.Z-linux-x86_64.AppImage
```

Keep it anywhere, such as `~/Applications`. It needs FUSE to mount itself, which desktop
installs have; where it doesn't (some containers, Ubuntu 22.04 without `libfuse2`), run it with
`--appimage-extract-and-run`, or install `libfuse2` (`libfuse2t64` on Ubuntu 24.04).
**Update:** Check for Updates…; Velopack replaces the AppImage file in place once the
feeds are signed (until then, the GitHub download page: download the new AppImage over the old).
**Uninstall:** delete the file. Velopack's download cache is under `/var/tmp/velopack/Tenmoku`
and settings under `~/.config/tenmoku`. To have it in the app launcher, use an AppImage
integrator (such as Gear Lever or AppImageLauncher), or the tar.gz's `install.sh` below.

**The tar.gz.** A portable folder:

```sh
mkdir -p ~/.local/opt   # or anywhere
tar -xzf Tenmoku-X.Y.Z-linux-x86_64.tar.gz -C ~/.local/opt
~/.local/opt/Tenmoku-X.Y.Z-linux-x86_64/install.sh
```

`install.sh` links `~/.local/bin/tenmoku` to the binary and puts the desktop entry and icon under
`~/.local/share`, so Tenmoku shows in the app launcher; `install.sh --uninstall` removes those
three. Without it, run `./tenmoku` from the folder. To update, unpack the new archive and run its
`install.sh`. Linux has no Gatekeeper or SmartScreen step: the archive keeps the binary
executable, and the AppImage needs only the `chmod +x` above.

The binary links only glibc, `libxcb` and `libxkbcommon` (with `libxkbcommon-x11`), which
desktop installs have, and loads the Vulkan loader (`libvulkan1`) and a Vulkan driver when it
starts: Mesa's on most distributions (`mesa-vulkan-drivers` on Debian and Ubuntu,
`vulkan-radeon` or `vulkan-intel` on Arch). It runs on X11 and on Wayland. A distribution
whose glibc is older than the release notes name cannot run it (0.2.x is built on Ubuntu
24.04 and needs glibc 2.39: Ubuntu 24.04, Debian 13, Fedora 40 or later).

The Velopack feed URL is baked in at build time from `github.repository`
(`https://github.com/<owner>/<repo>/releases/latest/download`), so builds made
after the move to `tenmoku-editor/tenmoku` name it. Earlier builds name the old
owner; velopack's `ureq` follows GitHub's redirect from it.
