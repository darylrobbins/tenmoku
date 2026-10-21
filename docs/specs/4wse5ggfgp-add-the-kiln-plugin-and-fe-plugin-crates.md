---
task_uid: "01m5fkjnaav76f9r4wse5ggfgp"
task: "T1045"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-plugin single_wasmtime::a_wat_component_runs_on_the_plugin_engine: a component written in WAT (compiled through wasmtime wat feature) with one exported function returning 42 is instantiated on a new wasmtime::Engine and returns 42"
    required: true
    manual: false
  - id: "C2"
    text: "fe-plugin single_wasmtime::the_plugin_engine_is_tree_sitters_wasmtime: a plugin Engine is accepted where tree_sitter::wasmtime::Engine is expected (one crate, compile-time proof)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo tree -d -p kiln-plugin -p fe-plugin shows no second wasmtime version, and cargo tree -i wasmtime lists both tree-sitter and kiln-plugin at one version"
    required: true
    manual: false
  - id: "C4"
    text: "every module the series fills exists as a documented stub, and neither crate is a dependency of crates/fastedit; the finish notes give the release fastedit binary size and cold release build time before and after temporarily linking fe-plugin (link reverted)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p kiln-plugin -p fe-plugin, cargo clippy -p kiln-plugin -p fe-plugin --all-targets -- -D warnings and cargo deny check pass"
    required: true
    manual: false
---

# Add the kiln-plugin and fe-plugin crates with wasmtime components and WASI on the single wasmtime 48 tree-sitter already links, and record the binary cost

## Problem

Stage 2 (extensions record §7) runs plugins as WebAssembly components on wasmtime. Nothing in the
workspace can load a component yet. tree-sitter's `wasm` feature (T845) already links wasmtime 48
through `wasmtime-c-api-impl`, but its engine is inside the C API and cannot run components. Plugins need
their own `wasmtime::Engine`, and the workspace must still contain exactly one wasmtime version.

## Change

- Root `Cargo.toml` `[workspace.dependencies]`: `wasmtime = { version = "48", default-features = false,
  features = ["component-model", "cranelift", "runtime", "std"] }` (adjust to what the lockfile's
  wasmtime 48 accepts so it is the same crate tree-sitter uses), `wasmtime-wasi = "48"`, `sha2` (already in
  `Cargo.lock`), and a dev-only `wat` feature use through each crate's `[dev-dependencies] wasmtime = {
  workspace = true, features = ["wat"] }`. `Cargo.lock` follows; `cargo deny check` passes (adjust
  `deny.toml` skips only if new duplicates appear; no new licence is expected).
- New crate `kiln/crates/kiln-plugin` (kiln-owned, headless, must not depend on GPUI or any fastedit crate):
  `Cargo.toml` (dependencies: wasmtime, wasmtime-wasi, kiln-command, kiln-core, serde, serde_json, toml,
  thiserror, log, sha2) and `src/lib.rs` declaring every module the series will fill, each as a stub
  file holding only its `//!` doc comment saying what lands there: `engine`, `limits`, `runner`, `bindings`,
  `manifest`, `grants`, `warnings`, `commands`, `events`, `fs`, `process`, `net`, `secrets`, `compat`,
  `cache`. Declaring all of them now means later cards edit only their own module files, never `lib.rs`.
- New crate `crates/fe-plugin` (fastedit's binding of the plugin host to the editor): `Cargo.toml`
  (dependencies: kiln-plugin, kiln-command, fe-buffer, fe-editor, fe-net, fe-text as needed, tree-sitter only as
  a dev-dependency) and `src/lib.rs` with stub modules `editor`, `decorations`, `status`, `net`.
  Neither crate is added to `crates/fastedit/Cargo.toml` here (the app wiring card does that).
- Binary cost, for the record's Q8: measure the release `fastedit` binary size and a cold `cargo build
  --release -p fastedit` time before and after linking `fe-plugin` temporarily into
  `crates/fastedit` (revert that link before finishing), and put both numbers in the finish notes.
- `THIRD_PARTY.md`: add wasmtime-wasi's and any new crates' licences if that file lists crates one by one.

## Notes

Single version proof: `cargo tree -d -p kiln-plugin` shows no second `wasmtime` and `cargo tree -i wasmtime`
shows `tree-sitter` and `kiln-plugin` both reach the same version. The epoch-ticker precedent is
`crates/fe-syntax/src/runtime.rs`; P3 writes the plugin one. `Cargo.lock` overlaps T1037 (which this card
depends on for that reason) and the inbox cards T410 and T970; the overlap is mechanical.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Implementation notes

Binary cost (Q8), release `fastedit`, cold `cargo build --release -p fastedit` into a fresh target dir, Apple Silicon:

| | binary | cold build |
|---|---|---|
| before | 95,448,032 B | 537 s |
| with `fe-plugin` linked (`use fe_plugin as _;`) | 95,569,376 B | 599 s |

About +121 KB and +62 s: tree-sitter already links wasmtime 48, and nothing in the stubs calls into the component runtime, so the linker drops most of it. The cost of real use will be larger. The link was reverted.

`Cargo.lock` was moved to wasmtime 48.0.7 (the whole wasmtime/wiggle family), rustls-webpki 0.103.16 and webpki-root-certs 1.0.10 so the full `cargo deny check` passes (RUSTSEC-2026-0378/0379/0382/0383). The binary-cost table above was measured on 48.0.6.
