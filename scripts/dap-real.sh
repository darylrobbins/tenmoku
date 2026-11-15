#!/usr/bin/env bash
# fe-dap's real-adapter tests (docs/decisions/r1b9dw3g0g-debugger.md §11 D16):
# runs the `#[ignore]`d tests in crates/fe-dap/tests/real_adapter.rs against
# debugpy and an LLDB adapter (lldb-dap, else codelldb).
#
#   scripts/dap-real.sh
#   FE_DAP_REQUIRE_ADAPTERS=1 scripts/dap-real.sh   # a missing adapter fails
#
# - debugpy comes from a wheel pinned by SHA-256 below, installed into a
#   scratch venv outside the repo. FE_DAP_PYTHON points a test at another
#   Python that has debugpy.
# - lldb-dap is looked up on PATH, then `xcrun -f lldb-dap`;
#   FE_DAP_LLDB_DAP and FE_DAP_CODELLDB name another binary.
# - Native tests skip where Linux's kernel.yama.ptrace_scope forbids them.
# - FE_DAP_REAL_RECORD=<file> appends the message orders the tests observe.
#
# .github/workflows/dap.yml runs this on ubuntu-latest and macos-latest.
set -euo pipefail

cd "$(dirname "$0")/.."

DEBUGPY_VERSION=1.8.22
DEBUGPY_WHEEL="debugpy-${DEBUGPY_VERSION}-py2.py3-none-any.whl"
DEBUGPY_URL="https://files.pythonhosted.org/packages/56/3d/c7dc9f35bc9e22cdb53679f844bee40fd5cff871862f60595869a433400d/${DEBUGPY_WHEEL}"
DEBUGPY_SHA256=a9e9d3550e15ca479c59333e90845029190531f0cacfedab3b815a57bd913947

scratch=$(mktemp -d "${TMPDIR:-/tmp}/fe-dap-real.XXXXXX")
trap 'rm -rf "$scratch"' EXIT

curl -fsSL --retry 3 -o "$scratch/$DEBUGPY_WHEEL" "$DEBUGPY_URL"
actual=$( (shasum -a 256 "$scratch/$DEBUGPY_WHEEL" 2>/dev/null || sha256sum "$scratch/$DEBUGPY_WHEEL") | awk '{print $1}')
if [ "$actual" != "$DEBUGPY_SHA256" ]; then
  echo "debugpy wheel hash mismatch: $actual" >&2
  exit 1
fi
python3 -I -m venv "$scratch/venv"
"$scratch/venv/bin/python" -I -m pip install --quiet --no-deps --no-index \
  "$scratch/$DEBUGPY_WHEEL"
export FE_DAP_PYTHON="$scratch/venv/bin/python"

cargo nextest run -p fe-dap --run-ignored only -E 'test(/_breakpoint_step_variables_evaluate/)' \
  --success-output final --no-fail-fast
