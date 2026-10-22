---
task_uid: "01m5gdj120pv92vrb0efx2wp3a"
task: "T271"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "python3 scripts/check_kiln_boundary.py exits 0 (prints 'ok: kiln/ stands alone'); the change is to doc comments in kiln/crates/kiln-plugin/src/engine.rs only; cargo fmt --all --check and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# kiln-plugin: reword engine.rs's module doc so it names no fastedit crate (kiln boundary check fails on main since T261)

## Problem

`python3 scripts/check_kiln_boundary.py` fails on helium `main` since T261 landed (Mac T1048): `kiln/crates/kiln-plugin/src/engine.rs:6: names fastedit` — the module doc says "Modelled on fe-syntax's grammar runtime", and kiln must not name fastedit's crates. The Linux gate does not run the boundary check, so it passed; the Mac's check will fail on it.

## Change

Reword that doc comment in `kiln/crates/kiln-plugin/src/engine.rs` so it names no fastedit crate (e.g. "Modelled on tree-sitter's Wasm grammar runtime in the editor"-style wording without any fastedit crate name; keep the meaning: the ticker sleeps while no call runs, so an idle plugin costs nothing). Doc-comment only; no code change. Run `python3 scripts/check_kiln_boundary.py` and `cargo fmt --all --check` before finishing.
