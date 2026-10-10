---
name: run-fastedit-linux
description: Linux only - build, run, and drive the fastedit editor app headless under Xvfb with xdotool. Use on Linux when asked to run or start fastedit, launch the app, take a screenshot of its UI, check a change works in the real window, click or type into the editor, or run its tests. Not for macOS, where the app runs on Metal and AppKit: there, use `cargo run` and the screenshot examples in TESTING.md instead.
---

**Linux only.** fastedit is a GPUI desktop editor (`crates/fastedit`). On
Linux, agents drive it headless with
`.claude/skills/run-fastedit-linux/driver.sh`: Xvfb renders it
through Mesa's software Vulkan, tmux holds the process, xdotool sends
real keys and clicks, and the framebuffer is converted to a PNG. All paths
are relative to the repo root.

## Prerequisites

Ubuntu 26.04. GPUI's Linux libraries, Xvfb, xdotool and tmux:

```bash
sudo apt install -y libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libfontconfig-dev libfreetype-dev libssl-dev libx11-xcb-dev libxcb1-dev libxcb-xkb-dev libasound2-dev libzstd-dev clang cmake xvfb mesa-vulkan-drivers xdotool tmux
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
$D click 1021 76                     # a row of the project tree, in screen pixels
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
| `key <keys>...` | xdotool key names: `ctrl+p`, `Return`, `Escape`, `ctrl+shift+bracketleft` |
| `type <text>` | type text into what has focus |
| `log [n]` | tail of the app's log (`/tmp/fastedit-run/app.log`) |
| `stop` | kill the app and Xvfb; unsaved edits are dropped |

The window is 1200×800 centred on the screen: x 40–1240. The project
tree's rows start at about (1020, 76), 24 px apart; the editor's first line
is at y 64. Settings, keymap and trust decisions go to
`/tmp/fastedit-run/{config,state}`, never the user's `~/.config` and
`~/.local/state`. Environment variables `FE_DISPLAY` (default `:99`),
`FE_RUN_DIR` and `FE_SESSION` move it.

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
- **There's no menu bar on Linux**, so menu-only commands (Open File…,
  Untrust This Folder) can't be reached; open files with ctrl+p or the tree.
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
