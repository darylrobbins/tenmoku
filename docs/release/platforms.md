# Downloads by platform

Each GitHub Release of Tenmoku carries one download per platform, each with a `.sha256` beside
it, built by `.github/workflows/release.yml` (one job per platform, then one publish job; see
[macos-signing.md](macos-signing.md), "Releasing"):

| Download | For | What it is |
| --- | --- | --- |
| `Tenmoku-X.Y.Z-macos-arm64.zip` | macOS on Apple Silicon | `Tenmoku.app` |
| `Tenmoku-X.Y.Z-windows-x64.zip` | Windows 10 (1903 or later) or 11 on Intel or AMD | a portable folder: `tenmoku.exe` and `Licenses\` |
| `Tenmoku-X.Y.Z-windows-arm64.zip` | Windows 11 on ARM (Snapdragon and other ARM64 PCs) | the same, built for ARM64 |
| `Tenmoku-X.Y.Z-linux-x86_64.tar.gz` | Linux on x86_64 with glibc (the release notes name the oldest glibc) | a portable folder: `tenmoku`, a desktop entry, the icon, `install.sh` and `Licenses/` |

None of them is an installer, and none needs administrator rights. Every build job runs its own
binary with `--version` before uploading it, and the Linux job also starts the app under Xvfb.

**These builds are not signed with a publisher certificate yet** (no Authenticode certificate,
and the macOS app is ad-hoc signed until the Developer ID secrets exist). The OS warns the first
time; the steps below get past that once.

## Check the download

```sh
shasum -a 256 -c Tenmoku-X.Y.Z-linux-x86_64.tar.gz.sha256   # macOS; sha256sum -c on Linux
```

On Windows, in PowerShell, compare `(Get-FileHash Tenmoku-X.Y.Z-windows-x64.zip).Hash` with the
first word of the `.sha256` file (case does not matter).

## macOS

Unzip, and drag `Tenmoku.app` to Applications. **Gatekeeper:** an app without a Developer ID
signature is refused with "Apple could not verify ...". Open it once, close the dialog, then
System Settings > Privacy & Security > **Open Anyway** (on macOS 14 and older, right-click the
app > Open also works). Or, from a terminal:

```sh
xattr -dr com.apple.quarantine /Applications/Tenmoku.app
```

Updates come through Sparkle when the release has an `appcast.xml`.

## Windows

1. Before extracting: right-click the zip > Properties > tick **Unblock** > OK. Otherwise every
   file extracted keeps the "downloaded from the internet" mark.
2. Extract it anywhere you can write, such as `%LOCALAPPDATA%\Programs\Tenmoku`, and run
   `tenmoku.exe`. To start it from a terminal as `tenmoku`, add that folder to your user PATH.
3. **SmartScreen:** "Windows protected your PC" appears for an unsigned download. Click **More
   info** > **Run anyway**. It asks once per new version.

Pick `windows-arm64` on an ARM PC: the x64 build runs there too, but emulated and slower. The
app needs a GPU with Direct3D 11, which every supported Windows PC has. To remove it, delete the
folder; settings are in `%USERPROFILE%\.config\tenmoku`.

## Linux

```sh
mkdir -p ~/.local/opt   # or anywhere
tar -xzf Tenmoku-X.Y.Z-linux-x86_64.tar.gz -C ~/.local/opt
~/.local/opt/Tenmoku-X.Y.Z-linux-x86_64/install.sh
```

`install.sh` links `~/.local/bin/tenmoku` to the binary and puts the desktop entry and icon under
`~/.local/share`, so Tenmoku shows in the app launcher; `install.sh --uninstall` removes those
three. Without it, run `./tenmoku` from the folder. Linux has no Gatekeeper or SmartScreen
step: the archive keeps the binary executable.

The binary links only glibc, `libxcb` and `libxkbcommon` (with `libxkbcommon-x11`), which
desktop installs have, and loads the Vulkan loader (`libvulkan1`) and a Vulkan driver when it
starts: Mesa's on most distributions (`mesa-vulkan-drivers` on Debian and Ubuntu,
`vulkan-radeon` or `vulkan-intel` on Arch). It runs on X11 and on Wayland. A distribution
whose glibc is older than the release notes name cannot run it (0.2.x is built on Ubuntu
24.04 and needs glibc 2.39: Ubuntu 24.04, Debian 13, Fedora 40 or later).
