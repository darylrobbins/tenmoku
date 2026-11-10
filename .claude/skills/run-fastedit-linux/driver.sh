#!/usr/bin/env bash
# Drives the Tenmoku app headless on Linux: Xvfb renders it (Mesa's
# software Vulkan), tmux holds it, xdotool types and clicks, and the
# framebuffer becomes a PNG. Run from the repo root. See SKILL.md.
#
#   driver.sh build                 cargo build -p fastedit
#   driver.sh start [path]          launch on a folder or file, wait for a frame
#   driver.sh ss <out.png>          screenshot the whole screen
#   driver.sh click <x> <y>         left click at screen pixels
#   driver.sh key <keys>...         xdotool key names: ctrl+p Return ctrl+shift+bracketleft
#   driver.sh type <text>           type text
#   driver.sh log [n]               last n lines of the app's log (default 30)
#   driver.sh a11y-tree <out.json> [path]
#                                   walk the app's AT-SPI tree in its own Xvfb
#                                   and D-Bus session; needs no `start`. Also
#                                   leaves <out>.dump.json, the app's own dump
#                                   (FASTEDIT_A11Y_DUMP)
#   driver.sh stop                  quit the app and Xvfb
#
# FE_WAYLAND=1 driver.sh start runs the app on Wayland instead: Weston,
# nested in Xvfb, with no server-side decorations, as GNOME. FE_BIN runs
# another binary the same way, such as the kiln gallery
# (target/debug/examples/gallery), which ignores the path.
set -euo pipefail

DISPLAY_NUM="${FE_DISPLAY:-:99}"
RUN_DIR="${FE_RUN_DIR:-/tmp/fastedit-run}"
SESSION="${FE_SESSION:-fastedit}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
BIN="${FE_BIN:-$REPO/target/debug/tenmoku}"
FB="$RUN_DIR/fb/Xvfb_screen0"
LOG="$RUN_DIR/app.log"
export PATH="$HOME/.cargo/bin:$PATH"

die() { echo "driver: $*" >&2; exit 1; }

# Xvfb, xdotool and the X framebuffer exist only on Linux; on macOS the app
# draws with Metal (see TESTING.md for running it there).
[[ "$(uname -s)" == Linux ]] || die "Linux only: on macOS run the app with cargo run"

shot() {
  [[ -f "$FB" ]] || die "no framebuffer: run 'driver.sh start' first"
  # Xvfb paints the X pointer into the framebuffer: park it in the empty
  # corner, outside the window, so it covers nothing.
  DISPLAY=$DISPLAY_NUM xdotool mousemove 1279 799
  sleep 0.2
  python3 "$HERE/xwd2png.py" "$FB" "$1"
}

# Runs inside dbus-run-session (see a11y-tree): the AT-SPI bus, an Xvfb of its
# own and the app, then a busctl walk of the tree (design doc, Spike 2).
a11y_probe() {
  local out="$1" target="$2" disp=":${FE_A11Y_DISPLAY:-93}" work="$RUN_DIR/a11y"
  rm -rf "$work"; mkdir -p "$work/config" "$work/state"
  local launcher=/usr/libexec/at-spi-bus-launcher
  [[ -x "$launcher" ]] || launcher="$(command -v at-spi-bus-launcher || true)"
  [[ -n "$launcher" ]] || die "no at-spi-bus-launcher: apt install at-spi2-core"
  "$launcher" --launch-immediately & pids="$!"
  trap 'kill $pids 2>/dev/null || true' EXIT
  sleep 1
  busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true
  local addr
  addr="$(busctl --user call org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress | cut -d'"' -f2)"
  local num="${disp#:}"
  [[ -e "/tmp/.X11-unix/X$num" || -e "/tmp/.X$num-lock" ]] \
    && die "display $disp is in use: set FE_A11Y_DISPLAY to a free number"
  Xvfb "$disp" -screen 0 1280x800x24 & local xpid="$!"; pids="$pids $xpid"
  for _ in $(seq 50); do
    kill -0 "$xpid" 2>/dev/null || die "Xvfb on $disp exited"
    DISPLAY=$disp xdotool getdisplaygeometry >/dev/null 2>&1 && break
    sleep 0.1
  done
  DISPLAY=$disp xdotool getdisplaygeometry >/dev/null 2>&1 || die "Xvfb on $disp isn't ready"
  # The setup wizard shows on a fresh config; the walk then reads its nodes.
  # FE_A11Y_KEYS (xdotool key names) is pressed first, e.g. "Escape" to skip it.
  # GPUI prefers WAYLAND_DISPLAY to DISPLAY, and AT_SPI_BUS_ADDRESS beats the
  # private session's bus discovery: clear the one, pin the other.
  env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET AT_SPI_BUS_ADDRESS="$addr" \
    DISPLAY=$disp XDG_CONFIG_HOME="$work/config" XDG_STATE_HOME="$work/state" \
    FASTEDIT_A11Y_DUMP="${out%.json}.dump.json" RUST_LOG=info "$BIN" "$target" > "$work/app.log" 2>&1 & pids="$pids $!"
  for _ in $(seq 60); do grep -q 'Accessibility activated' "$work/app.log" && break; sleep 0.5; done
  grep -q 'Accessibility activated' "$work/app.log" \
    || { tail -20 "$work/app.log" >&2; die "a11y never activated"; }
  sleep "${FE_A11Y_SETTLE:-4}"
  if [[ -n "${FE_A11Y_KEYS:-}" ]]; then
    DISPLAY=$disp xdotool mousemove 640 400 key --delay 80 $FE_A11Y_KEYS
    sleep 2
  fi
  ADDR="$addr" python3 - "$out" <<'PY'
import json, os, subprocess, sys

addr = os.environ["ADDR"]
ACC = "org.a11y.atspi.Accessible"


def busctl(*args):
    return subprocess.run(["busctl", f"--address={addr}", "--json=short", "--timeout=5", *args],
                          capture_output=True, text=True, check=True, timeout=10).stdout


def children(bus, path):
    return [(c[0], c[1]) for c in json.loads(busctl("call", bus, path, ACC, "GetChildren"))["data"][0]]


def attempt(fn, *args):
    # One node failing to answer is a finding, not the end of the walk.
    try:
        return fn(*args)
    except (subprocess.SubprocessError, ValueError, KeyError) as error:
        detail = getattr(error, "stderr", None) or str(error)
        return f"<error: {detail.strip()[:120]}>"


def prop(bus, path, name):
    return json.loads(busctl("get-property", bus, path, ACC, name))["data"]


def role_id(bus, path):
    # accesskit_unix answers GetRole (the AT-SPI Role number) and leaves the
    # name methods empty or unknown.
    return json.loads(busctl("call", bus, path, ACC, "GetRole"))["data"][0]


def walk(bus, path, depth=0):
    node = {"name": attempt(prop, bus, path, "Name"), "role": attempt(role_id, bus, path)}
    if depth < 40:
        kids = attempt(children, bus, path)
        node["children"] = kids if isinstance(kids, str) else [walk(b, p, depth + 1) for b, p in kids]
    return node


root = ("org.a11y.atspi.Registry", "/org/a11y/atspi/accessible/root")
tree = {"name": "root", "role": "application root",
        "children": [walk(b, p) for b, p in children(*root)]}
with open(sys.argv[1], "w") as f:
    json.dump(tree, f, indent=2)
    f.write("\n")
if not any(c.get("name") == "tenmoku" for c in tree["children"]):
    sys.exit("no tenmoku node under the AT-SPI root: " + json.dumps(tree["children"])[:300])
print(f"wrote {sys.argv[1]}")
PY
}

cmd="${1:-}"; shift || true
case "$cmd" in
  build)
    cd "$REPO" && cargo build -p fastedit
    ;;
  start)
    target="${1:-$REPO}"
    [[ -x "$BIN" ]] || die "$BIN missing: run 'driver.sh build'"
    tmux has-session -t "$SESSION" 2>/dev/null && die "already running: 'driver.sh stop' first"
    mkdir -p "$RUN_DIR/fb" "$RUN_DIR/config" "$RUN_DIR/state"
    rm -f "$FB" "$LOG"
    tmux new-session -d -s "$SESSION" -n xvfb \
      "Xvfb $DISPLAY_NUM -screen 0 1280x800x24 -fbdir $RUN_DIR/fb"
    for _ in $(seq 50); do [[ -f "$FB" ]] && break; sleep 0.1; done
    [[ -f "$FB" ]] || die "Xvfb didn't start (another server on $DISPLAY_NUM?)"
    # Settings, keymap and trust decisions go to $RUN_DIR, not the user's.
    display="DISPLAY=$DISPLAY_NUM"
    if [[ -n "${FE_WAYLAND:-}" ]]; then
      command -v weston >/dev/null || die "FE_WAYLAND needs weston: apt install weston"
      # Weston's X11 backend gives Wayland clients the X pointer and
      # keyboard; its headless one has no seat, which GPUI needs.
      runtime="$RUN_DIR/runtime"
      mkdir -p "$runtime" && chmod 700 "$runtime"
      tmux new-window -t "$SESSION" -n weston \
        "DISPLAY=$DISPLAY_NUM XDG_RUNTIME_DIR='$runtime' weston --backend=x11 \
         --renderer=pixman --width=1280 --height=800 --socket=wayland-fe \
         > '$RUN_DIR/weston.log' 2>&1"
      for _ in $(seq 50); do [[ -S "$runtime/wayland-fe" ]] && break; sleep 0.1; done
      [[ -S "$runtime/wayland-fe" ]] || die "weston didn't start: $RUN_DIR/weston.log"
      display="XDG_RUNTIME_DIR='$runtime' WAYLAND_DISPLAY=wayland-fe"
    fi
    tmux new-window -t "$SESSION" -n app \
      "cd '$REPO' && $display XDG_CONFIG_HOME='$RUN_DIR/config' \
       XDG_STATE_HOME='$RUN_DIR/state' RUST_LOG=info \
       '$BIN' '$target' 2>&1 | tee '$LOG'"
    # Ready once the window has drawn: the blank screen is one colour, and
    # under Weston the app has logged its renderer.
    for _ in $(seq 60); do
      sleep 0.5
      if [[ -n "${FE_WAYLAND:-}" ]]; then
        grep -q 'Selected GPU adapter' "$LOG" 2>/dev/null || continue
        sleep 1
      fi
      out="$(shot "$RUN_DIR/ready.png")"
      [[ "${out##*colours~}" -gt 2 ]] && { echo "ready: $out"; exit 0; }
    done
    tail -20 "$LOG" >&2 || true
    die "no frame after 30 s"
    ;;
  ss)
    [[ $# -eq 1 ]] || die "usage: ss <out.png>"
    shot "$1"
    ;;
  click)
    [[ $# -eq 2 ]] || die "usage: click <x> <y>"
    DISPLAY=$DISPLAY_NUM xdotool mousemove "$1" "$2" click 1
    sleep 0.3
    ;;
  key)
    [[ $# -ge 1 ]] || die "usage: key <keys>..."
    # F1-F12 go as key codes: by name, xdotool picks a level of Xvfb's
    # CTRL+ALT key type and sends them with Alt held (F10 arrives alt-f10).
    keys=()
    for k in "$@"; do
      mods="${k%F*}"; n="${k##*F}"
      if [[ "$k" =~ (^|\+)F([1-9]|1[0-2])$ ]]; then
        if (( n <= 10 )); then code=$((66 + n)); else code=$((84 + n)); fi
        k="${mods}${code}"
      fi
      keys+=("$k")
    done
    # The pointer must be over the window for keys to reach it: nothing
    # gives it focus otherwise, with no window manager.
    DISPLAY=$DISPLAY_NUM xdotool mousemove 640 400 key --delay 80 "${keys[@]}"
    sleep 0.3
    ;;
  type)
    [[ $# -eq 1 ]] || die "usage: type <text>"
    DISPLAY=$DISPLAY_NUM xdotool mousemove 640 400 type --delay 30 "$1"
    sleep 0.3
    ;;
  log)
    tail -n "${1:-30}" "$LOG"
    ;;
  a11y-tree)
    [[ $# -ge 1 ]] || die "usage: a11y-tree <out.json> [path]"
    [[ -x "$BIN" ]] || die "$BIN missing: run 'driver.sh build'"
    for tool in dbus-run-session busctl Xvfb xdotool; do
      command -v "$tool" >/dev/null || die "needs $tool"
    done
    out="$(realpath -m "$1")"
    # A private session bus keeps the probe off the user's; re-enter this
    # script inside it.
    exec dbus-run-session -- "$0" a11y-probe "$out" "${2:-$REPO}"
    ;;
  a11y-probe)
    a11y_probe "$1" "$2"
    ;;
  stop)
    # Killing the session ends Xvfb and the app with it, unsaved edits and all.
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    pkill -f "Xvfb $DISPLAY_NUM " 2>/dev/null || true
    # Xvfb takes a moment to exit; a quick restart would find it on the display.
    for _ in $(seq 50); do
      pgrep -f "Xvfb $DISPLAY_NUM " >/dev/null || { echo "stopped"; exit 0; }
      sleep 0.1
    done
    die "Xvfb on $DISPLAY_NUM is still running"
    ;;
  *)
    sed -n '2,16p' "$0" >&2
    exit 2
    ;;
esac
