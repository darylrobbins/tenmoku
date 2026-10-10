#!/usr/bin/env bash
# Drives the fastedit app headless on Linux: Xvfb renders it (Mesa's
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
#   driver.sh stop                  quit the app and Xvfb
set -euo pipefail

DISPLAY_NUM="${FE_DISPLAY:-:99}"
RUN_DIR="${FE_RUN_DIR:-/tmp/fastedit-run}"
SESSION="${FE_SESSION:-fastedit}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
BIN="$REPO/target/debug/fastedit"
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
    tmux new-window -t "$SESSION" -n app \
      "cd '$REPO' && DISPLAY=$DISPLAY_NUM XDG_CONFIG_HOME='$RUN_DIR/config' \
       XDG_STATE_HOME='$RUN_DIR/state' RUST_LOG=info \
       '$BIN' '$target' 2>&1 | tee '$LOG'"
    # Ready once the window has drawn: the blank screen is one colour.
    for _ in $(seq 60); do
      sleep 0.5
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
