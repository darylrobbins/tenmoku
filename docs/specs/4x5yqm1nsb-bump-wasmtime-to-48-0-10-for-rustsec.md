---
task_uid: "01m7tshbn72hstc64x5yqm1nsb"
task: "T431"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Cargo.lock resolves the whole wasmtime 48 family at 48.0.10, and nothing outside that family changes version unless the family's new requirements force it (listed in the notes)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo deny check advisories passes with deny.toml unchanged and reports none of RUSTSEC-2026-0566 to RUSTSEC-2026-0573"
    required: true
    manual: false
  - id: "C3"
    text: "cargo deny check bans licenses sources passes"
    required: true
    manual: false
  - id: "C4"
    text: "The implementation notes list each advisory (plus GHSA-f2pq-268w-886c), the version that fixes it, and whether Tenmoku reaches the affected code"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p kiln-plugin -p fe-plugin -p fe-syntax passes with no existing test edited, including the wasm grammar tests"
    required: true
    manual: false
  - id: "C6"
    text: "cargo check -p fe-workbench -p tenmoku-plugin --all-targets passes"
    required: true
    manual: false
---

# Bump wasmtime to 48.0.10 for RUSTSEC-2026-0566 to -0573 (CI deny) (Mac T2025)

{"mode": "inline", "body": "CI's `deny` job fails on main and on every PR (seen on tenmoku-editor/tenmoku#9, run 44606889127,\njob 135852841974): `advisories FAILED` with eight Wasmtime advisories published 2026-11-16, all\nagainst wasmtime 48.0.8 and all fixed in 48.0.9 (`Upgrade to ... >=48.0.9, <49.0.0`):\n\n- RUSTSEC-2026-0566 (GHSA-5m8g-xhq4-4h93): wasmtime-wasi, unbounded host memory via empty writes to `AsyncWriteStream`\n- RUSTSEC-2026-0567 (GHSA-2674-w29h-wqvj): abort in builds without the `threads` cargo feature\n- RUSTSEC-2026-0568 (GHSA-43j2-gcp4-m22g): stale GC heap base pointer lets a guest abort the host\n- RUSTSEC-2026-0569 (GHSA-hj27-mpw9-ff4c): panic in `Caller::get_module_export` outside a wasm call\n- RUSTSEC-2026-0570 (GHSA-rc4p-x33f-vpr2): abort in builds without the `gc` cargo feature\n- RUSTSEC-2026-0571 (GHSA-f8mm-ggr3-89wh): wasmtime-wasi-http, empty HTTP/2 body writes (wasmtime-wasi-http is not in our lock)\n- RUSTSEC-2026-0572 (GHSA-xrvr-8678-5wp8): panic when a component cancels a stream/future copy on the wrong end\n- RUSTSEC-2026-0573 (GHSA-wvgq-g5hf-6gm6): abort when a GC heap reservation isn't a page-size multiple\n\nGo to 48.0.10, not 48.0.9: 48.0.10 (2026-11-19) also fixes a WASI filesystem sandbox escape on\nWindows through a drive-prefixed path component (GHSA-f2pq-268w-886c, no RUSTSEC id yet). kiln-plugin\npreopens granted directories (`kiln/crates/kiln-plugin/src/fs.rs`) and Tenmoku ships on Windows, so\nthat one is likely reached; bumping now stops the next deny failure too.\n\nWhere wasmtime comes from: root `Cargo.toml` `[workspace.dependencies]` pins `wasmtime = \"48.0.8\"`\n(default-features off; component-model, cranelift, runtime, std) and `wasmtime-wasi = \"48.0.8\"`,\nused by kiln-plugin (+ `call-hook`), fe-plugin, fe-workbench and tenmoku-plugin. The vendored\ntree-sitter (`vendor/tree-sitter/Cargo.toml`) asks for `wasmtime-c-api-impl = \"48.0.1\"` (caret,\nfeatures cranelift, gc, gc-null), which resolves to the same 48 line; wasmtime-c-api-impl 48.0.10\nrequires `wasmtime ^48.0.10`, so one wasmtime stays in the tree. No vendor edit is needed.\n\nChange (precedent: T1301, which moved 48.0.7 to 48.0.8 the same way):\n1. Root `Cargo.toml`: raise the `wasmtime` and `wasmtime-wasi` floors to \"48.0.10\" (keep features\n   and the comment above them accurate).\n2. `Cargo.lock`: `cargo update -p wasmtime -p wasmtime-wasi -p wasmtime-c-api-impl` (stays <49 by\n   caret). The whole wasmtime 48 family (wasmtime-*, wasmtime-internal-*, cranelift-*, pulley-*,\n   wiggle if present) moves together; nothing outside it may change unless the family's new\n   requirements force it (list any such crate in the notes). kiln has no Cargo.lock of its own.\n3. `deny.toml` unchanged: no ignores; every advisory has a patched release in our major. If a\n   bump turns out impossible, stop and report instead of adding ignores.\n\nImplementation notes: a table of the eight advisories plus GHSA-f2pq-268w-886c: id, fixed-in, and\nwhether Tenmoku reaches the affected code, read from the advisory and our source. Starting points:\nkiln-plugin's linker adds only `wasmtime_wasi::p2` (`runner.rs`) and `WasiCtxBuilder` keeps default\nstdio (`fs.rs`) for 0566; `gc` is on in the app only through tree-sitter's `wasm` feature, so\nkiln-plugin built alone has no `gc` (0570) while the app has it (0568, 0573); no enabled feature\nlist names `threads` (0567) or component-model-async (0572); no crate calls `get_module_export`\n(0569). Note that compiled plugin components (and any cached grammar artifacts) from 48.0.8 stop\ndeserializing after the bump; confirm `kiln/crates/kiln-plugin/src/cache.rs` still treats that as a miss.\n\nLinux-buildable: lockfile and manifest only, no platform code.", "ref": "", "ref_blob": ""}

## Implementation notes

Root `Cargo.toml` floors raised to 48.0.10; `cargo update -p wasmtime -p wasmtime-wasi -p wasmtime-c-api-impl` moved 34 crates, all of the family: wasmtime, wasmtime-wasi(-io), wasmtime-c-api-impl, wasmtime-environ, wasmtime-internal-*, cranelift-* (0.135.8 -> 0.135.10), pulley-* and wiggle* (48.0.8 -> 48.0.10). Nothing outside the family changed; `deny.toml` is unchanged. `cargo deny check advisories` and `bans licenses sources` pass.

| Id | Fixed in | Reached by Tenmoku? |
|---|---|---|
| RUSTSEC-2026-0566 (wasmtime-wasi, empty writes to `AsyncWriteStream`) | 48.0.9 | Possibly: kiln-plugin links `wasmtime_wasi::p2` and keeps default stdio, so a plugin can write to stdout/stderr. Patched. |
| RUSTSEC-2026-0567 (abort without `threads` feature) | 48.0.9 | Build configuration matches (no feature list names `threads`); patched. |
| RUSTSEC-2026-0568 (stale GC heap base, guest aborts host) | 48.0.9 | App only: `gc` comes on through tree-sitter's `wasm` feature, so grammar modules run on it. |
| RUSTSEC-2026-0569 (panic in `Caller::get_module_export`) | 48.0.9 | No: no crate calls it. |
| RUSTSEC-2026-0570 (abort without `gc` feature) | 48.0.9 | kiln-plugin built alone has no `gc`; the app has it via tree-sitter. |
| RUSTSEC-2026-0571 (wasmtime-wasi-http, empty HTTP/2 writes) | 48.0.9 | No: wasmtime-wasi-http is not in Cargo.lock. |
| RUSTSEC-2026-0572 (component cancels stream/future copy on wrong end) | 48.0.9 | No: component-model-async is not enabled. |
| RUSTSEC-2026-0573 (GC heap reservation not page-size multiple) | 48.0.9 | App only, through tree-sitter's `gc`. |
| GHSA-f2pq-268w-886c (WASI fs sandbox escape on Windows via drive-prefixed path) | 48.0.10 | Yes on Windows: kiln-plugin preopens granted directories (`fs.rs`). |

Cache: `kiln-plugin/src/cache.rs` keys entries on wasmtime's version, and `Component::deserialize(...).ok()` turns a failure on an entry from another version into a miss, so 48.0.8 artifacts recompile.

Checks: `cargo check -p fe-workbench -p tenmoku-plugin --all-targets` passes; `cargo nextest run -p kiln-plugin -p fe-plugin -p fe-syntax` 770 passed.
