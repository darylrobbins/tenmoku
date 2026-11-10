---
name: run-fastedit-linux
description: Linux only - build, run, and drive the Tenmoku editor app (this repo, fastedit) headless under Xvfb with xdotool. Use on Linux when asked to run or start Tenmoku or fastedit, launch the app, take a screenshot of its UI, check a change works in the real window, click or type into the editor, or run its tests. Not for macOS, where the app runs on Metal and AppKit: there, use `cargo run` and the screenshot examples in TESTING.md instead.
---

**Linux only.** Tenmoku is a GPUI desktop editor (`crates/fastedit`, binary `tenmoku`). On
Linux, agents drive it headless with
`.claude/skills/run-fastedit-linux/driver.sh`: Xvfb renders it
through Mesa's software Vulkan, tmux holds the process, xdotool sends
real keys and clicks, and the framebuffer is converted to a PNG. All paths
are relative to the repo root.

## Prerequisites

Ubuntu 26.04. GPUI's Linux libraries, Xvfb, xdotool and tmux:

```bash
sudo apt install -y libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libfontconfig-dev libfreetype-dev libssl-dev libx11-xcb-dev libxcb1-dev libxcb-xkb-dev libasound2-dev libzstd-dev clang cmake xvfb mesa-vulkan-drivers xdotool tmux weston
```

Rust via rustup; the repo's `rust-toolchain.toml` pins the version, which
the first `cargo` call installs:

```bash
curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain none --profile minimal
```

## Build

```bash
.claude/skills/run-fastedit-linux/driver.sh build
```

A cold build takes several minutes; later ones a few seconds.

## Run (agent path)

```bash
D=.claude/skills/run-fastedit-linux/driver.sh
mkdir -p /tmp/fastedit-demo/src && printf 'fn main() {\n    println!("hi");\n}\n' > /tmp/fastedit-demo/src/main.rs
$D start /tmp/fastedit-demo          # waits for the first frame
$D ss /tmp/fastedit-run/01-start.png # the trust prompt for a new folder
$D key Return                        # Trust Folder
$D key ctrl+p && $D type main && $D key Return   # quick open src/main.rs
$D key ctrl+End && $D type '// edited'
$D ss /tmp/fastedit-run/02-edited.png
$D click 1021 105                    # a row of the project tree, in screen pixels
$D key F10 Right Down Return         # the in-window menu bar: Edit > Redo
$D log 20
$D stop
```

Then **read the PNG** to see the result.

| command | what it does |
|---|---|
| `build` | `cargo build -p fastedit` |
| `start [path]` | launch on a folder or file (default: the repo) and wait for a frame |
| `ss <out.png>` | screenshot the 1280×800 screen; prints the colour count (1 = blank) |
| `click <x> <y>` | left click at screen pixels |
| `key <keys>...` | xdotool key names: `ctrl+p`, `Return`, `Escape`, `ctrl+shift+bracketleft`, `F10` |
| `type <text>` | type text into what has focus |
| `log [n]` | tail of the app's log (`/tmp/fastedit-run/app.log`) |
| `a11y-tree <out.json> [path]` | read the accessibility tree (see below); starts its own Xvfb and D-Bus, so no `start` |
| `stop` | kill the app and Xvfb; unsaved edits are dropped |

## Accessibility tree

`driver.sh a11y-tree <out.json> [path]` runs the app under Xvfb in a private
`dbus-run-session` with `at-spi-bus-launcher`, sets `org.a11y.Status IsEnabled`
true so accesskit_unix activates, then walks the live AT-SPI tree with `busctl`
(design doc `docs/decisions/qenw9xxdzk-accessibility-design.md`, Spike 2 and
section 9 shape 4). It needs `dbus-run-session`, `busctl`, `Xvfb`, `xdotool` and
`/usr/libexec/at-spi-bus-launcher` (`apt install at-spi2-core`), and a built
binary. It uses display `:93` (`FE_A11Y_DISPLAY`), not `start`'s, and a fresh
config, so the setup wizard shows: `FE_A11Y_KEYS=Escape` skips it before the walk.
`FE_A11Y_SETTLE` (seconds, default 4) waits for the first frames.

The output is JSON: `{name, role, children}` per node, where `role` is the
AT-SPI role number (43 push button, 23 frame). It also leaves
`<out>.dump.json`, what `FASTEDIT_A11Y_DUMP` wrote.

`FASTEDIT_A11Y_DUMP=<path> tenmoku` writes `Window::debug_a11y_tree_json()` to
`<path>` after each frame a window draws while the platform has a11y active
(VoiceOver or Accessibility Inspector on the Mac, an AT-SPI client on Linux),
at most once a second for the app; a frame inside that second is written when
it ends. The trigger is `fe_workbench::AfterFrame`, which `Workbench::render`
defers past the draw. An idle window draws no frames, so nothing is written.
Unset, nothing is installed.

The window is 1200×800 centred on the screen: x 40–1240. The menu bar is
the top 28 px (File at x 64, Edit 103, View 147, Go 187, Window 236); the
project tree's rows start at about (1020, 105), 24 px apart, and the
editor's first line is at y 93. Settings, keymap and trust decisions go to
`/tmp/fastedit-run/{config,state}`, never the user's `~/.config` and
`~/.local/state`. Environment variables `FE_DISPLAY` (default `:99`),
`FE_RUN_DIR` and `FE_SESSION` move it, and `FE_BIN` runs another binary,
such as the kiln gallery:

```bash
cargo build -p kiln --example gallery
FE_BIN=target/debug/examples/gallery FE_DISPLAY=:97 FE_SESSION=gallery \
  FE_RUN_DIR=/tmp/gallery-run $D start
```

### On Wayland

```bash
FE_WAYLAND=1 $D start /tmp/fastedit-demo/src/main.rs   # Weston, nested in Xvfb
$D ss /tmp/fastedit-run/40-wayland.png
$D stop
```

Weston, like GNOME, offers no server-side decorations, so the app draws its
frame: the menu row is also the title bar, with the buttons at its right.
Weston's panel takes the top 32 px, and the window opens at about (24, 32),
so coordinates differ from X11's.

## Run (human path)

```bash
cargo run -- /path/to/folder   # a window on the desktop; quit with ctrl+q
```

## Test

```bash
cargo nextest run --workspace --profile ci   # what CI runs; ~35 s once built
cargo clippy --workspace --all-targets -- -D warnings
```

All 1232 tests pass on Linux. Under full load a fake-language-server test
(`fe-lsp tests::hover::*`, `fe-workbench tests::lsp_diagnostics::*`) now
and then times out after 10 s; it passes alone. Install nextest with
`curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C ~/.cargo/bin`.

## Gotchas

- **Keys go to the window under the pointer.** Xvfb has no window
  manager, so nothing holds focus: typed with the pointer off the window,
  keys are lost. `key` and `type` move the pointer to (640, 400) first.
- **Xvfb paints the X pointer into the framebuffer**, as a black "X" over
  whatever is beneath it. `ss` parks it in the bottom-right corner first.
- **Synthetic events (`XSendEvent`) are ignored**: GPUI reads the pointer
  through XInput2 and its keys through xkb. Only XTest input, which is
  what xdotool sends, reaches it.
- **Keys are the Linux keymap** (`crates/fe-keymap/keymaps/default-linux.toml`):
  ctrl where macOS has cmd. Shifted punctuation is named by what it types:
  fold is `ctrl-{` in the keymap, sent as `ctrl+shift+bracketleft`.
- **A new folder opens with the trust prompt**, which takes keys until
  answered (Enter trusts, Escape doesn't). A file launch roots the tree at
  its folder and doesn't ask. `stop` and `start` again: the trust decision
  persists in `/tmp/fastedit-run/state` and the prompt doesn't come back.
- **F1–F12 by name arrive with Alt held**: xdotool picks a level of Xvfb's
  `CTRL+ALT` key type for them, so `xdotool key F10` reaches the app as
  `alt-f10`. `driver.sh key` sends them as key codes instead; use it, not
  raw xdotool, for function keys.
- **`RUST_LOG` takes one level** (`debug`, `info`), not `env_logger`
  filters: `info,zbus=warn` doesn't parse and falls back to info.
- **The menu bar is drawn in the window** (GPUI has no native one on
  Linux). It takes focus while a menu is open: keys go to it until Escape
  or a chosen item gives focus back.
- **Weston's headless backend won't do**: it has no seat, and GPUI panics
  at startup without one (`wayland/client.rs` `seat.unwrap()`). The X11
  backend inside Xvfb gives it Xvfb's pointer and keyboard.
- **GPUI's test windows only report server-side decorations**, so tests of
  the client-side frame set `Workbench::client_tiling` (test-only) instead.
- **Open Folder… (ctrl+o) and Open File…** need xdg-desktop-portal, which
  Xvfb has none of: they show "The open panel isn't available". Pass the
  folder to `start` instead.
- **rust-analyzer restarts, then stops** ("closed its output (EOF)") unless
  `rustup component add rust-analyzer` has been run; the status bar shows it.
  Harmless for everything else.
- **Screenshots examples** (`cargo run -p fe-gpui --example screenshots`)
  render with Metal: on Linux they print an error and exit 1. Use this
  driver instead.

## Troubleshooting

- **`At least one of the "wayland" or "x11" features must be enabled`** at
  launch: an old build from before `crates/fastedit/Cargo.toml` enabled
  them for Linux. Rebuild.
- **`Found no xinput mouse pointers`** in the log: Xvfb has no input
  devices until xdotool's first event. Harmless; clicks work.
- **`start` says "no frame after 30 s"**: the log's tail is printed. A
  missing library shows there; check the apt line above.
- **`already running`**: a previous session is up; `driver.sh stop`.
