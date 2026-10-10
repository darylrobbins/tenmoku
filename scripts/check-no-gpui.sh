#!/usr/bin/env bash
# Fail if any core crate depends on GPUI, directly or transitively.
# Core crates must stay headless so the AI backend and tests can use them.
# Every workspace crate is core unless listed in UI_CRATES. Then checks that
# kiln/, the app framework, stands alone (scripts/check_kiln_boundary.py).
set -euo pipefail

UI_CRATES=(fastedit fe-ui fe-gpui fe-workbench kiln kiln-platform kiln-shell)

cd "$(dirname "$0")/.."

members=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print("\n".join(p["name"] for p in json.load(sys.stdin)["packages"]))')

failed=0
for crate in $members; do
  if printf '%s\n' "${UI_CRATES[@]}" | grep -qx "$crate"; then
    continue
  fi
  # GPUI itself and its platform crates. Zed's Apache utility crates that ship
  # under the same prefix (gpui-pre-sum-tree, -collections, -util, …) are fine.
  if cargo tree -p "$crate" -e normal --target all --prefix none --format '{p}' \
    | grep -qE '^gpui-pre(-(platform|macos|apple|linux|windows|wgpu|web|macros))? '; then
    echo "error: core crate '$crate' depends on GPUI:" >&2
    cargo tree -p "$crate" -e normal --target all -i gpui-pre >&2 || true
    failed=1
  else
    echo "ok: $crate"
  fi
done

python3 scripts/check_kiln_boundary.py || failed=1

exit "$failed"
