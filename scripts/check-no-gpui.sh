#!/usr/bin/env bash
# Fail if any core crate depends on GPUI, directly or transitively.
# Core crates must stay headless so the AI backend and tests can use them.
# Every workspace crate is core unless listed in UI_CRATES.
set -euo pipefail

UI_CRATES=(fastedit fe-ui fe-gpui fe-workbench)

cd "$(dirname "$0")/.."

members=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print("\n".join(p["name"] for p in json.load(sys.stdin)["packages"]))')

failed=0
for crate in $members; do
  if printf '%s\n' "${UI_CRATES[@]}" | grep -qx "$crate"; then
    continue
  fi
  if cargo tree -p "$crate" -e normal --target all --prefix none --format '{p}' \
    | grep -qE '^gpui-pre(-[a-z-]+)? '; then
    echo "error: core crate '$crate' depends on GPUI:" >&2
    cargo tree -p "$crate" -e normal --target all -i gpui-pre >&2 || true
    failed=1
  else
    echo "ok: $crate"
  fi
done

exit "$failed"
