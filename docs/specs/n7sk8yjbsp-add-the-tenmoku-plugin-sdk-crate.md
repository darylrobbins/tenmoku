---
task_uid: "01m6j07w64se2sqbn7sk8yjbsp"
task: "T431"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "the_template_manifest_is_a_plugin: the template extension.toml passes the kiln extension check and PluginManifest::from_extension with ExtensionOrigin::User gives id example.word_count, its wasm inside the directory and the capabilities buffer.read, status and commands (crates/tenmoku-plugin/tests/template.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "the_template_loads_and_counts_words: the prebuilt word_count.wasm loads through the fe-plugin linker with those grants, registers example.word_count.count, shows a status item 3 words after a FocusChanged event for a buffer reading one two three, and its count command logs 3 (crates/tenmoku-plugin/tests/template.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p tenmoku-plugin -p fe-plugin and cargo clippy -p tenmoku-plugin --all-targets -- -D warnings pass; cargo deny check bans licenses sources passes"
    required: true
    manual: false
---

# Add the tenmoku-plugin SDK crate (bindings for kiln:extension and fastedit:editor, a small Plugin trait, export and log macros), mark the WIT a preview, and a word-count plugin template built on it (Mac T1351)

## Problem

A plugin author today has to copy the test guest's `wit_bindgen::generate!` setup (two WIT paths and
`generate_all`, `crates/fe-plugin/tests/guests/editor/src/lib.rs`) and implement every export. The
WIT is unstable until language-feature and UI imports exist (user, 2026-11-03).

## Change

- **SDK crate** `crates/tenmoku-plugin/` (a workspace member through `crates/*`; `publish = false`).
  It names the `fastedit:editor` world, so it lives under `crates/`, not `kiln/` (kiln may not name
  the app; `scripts/check_kiln_boundary.py`).
  - `Cargo.toml`: `wit-bindgen` 0.57 with its macros, the version written in this manifest as the
    guest crates do, keeping the root manifest untouched; dev-dependencies `fe-plugin`,
    `kiln-plugin`, `wasmtime` for the template test.
  - `src/lib.rs`: `generate!` for `fastedit:editor/editor-extension` from
    `../../kiln/crates/kiln-plugin/wit` and `../fe-plugin/wit` (their one copy, no `wit/deps`),
    `generate_all`, `pub_export_macro: true`, `default_bindings_module: "tenmoku_plugin"`.
    Re-exports under short paths (`buffer`, `decorations`, `status`, `commands`, `events`, `log`,
    `fs`, `net`, `process`, `secrets`, `types`).
  - `src/plugin.rs`: a `Plugin` trait with default methods (`init`, `shutdown`, `run_command`,
    `handle_events`) and `export_plugin!(MyPlugin)`, which implements the generated `Guest` traits by
    delegating, so a plugin writes only what it uses; `info!`, `warn!` and `error!` macros over the
    `log` import; constructors for the WIT `error` cases.
  - Crate docs open with the preview notice: the API can change incompatibly in any release; rebuild
    plugins against each release.
  - It builds for the host target in the workspace (wit-bindgen's non-wasm stubs), so CI's clippy
    and tests cover it. If it can't, make it standalone with its own `[workspace]`, like the test
    guest, and say so in the hand-in.
- **Unstable WIT, in code.** `crates/fe-plugin/wit/world.wit` gets a doc comment: this world and the
  `kiln:extension` world it includes are a preview; incompatible changes may come in any release
  until they are declared stable; the package versions stay @1.0.0 meanwhile, and the adapters in
  `kiln_plugin::compat` apply from stabilization on. No version rename: a move to 0.x would churn
  every WIT file, both bindgen sites, `compat::HOST_API` and every checked-in guest for no runtime
  difference, and adding imports (the coming diagnostics and formatting cards) already leaves old
  guests loading.
- **Template**, a plain directory (no cargo-generate, no new tool): `crates/tenmoku-plugin/template/`
  with `Cargo.toml` (its own `[workspace]`, a path dependency on `..`, the test guest's release
  profile), `Cargo.lock`, `extension.toml` (`id = "example.word_count"`,
  `[engine] tenmoku = "1.1"`, `[contributes.plugin] wasm = "word_count.wasm"`,
  `capabilities = ["buffer.read", "status", "commands"]`), `src/lib.rs` (on focus change and save, a
  status item "N words" for the focused buffer; a `count` command that logs the count), `build.zsh`
  (as the test guest's: wasm32-wasip2, release, copies the component) and the prebuilt
  `word_count.wasm`.
- `crates/tenmoku-plugin/tests/template.rs` loads the prebuilt template through fe-plugin's linker
  with a fake `BufferAccess`. It is the drift guard: CI never builds guests, so a WIT change that
  breaks the template fails here.

The test guest `crates/fe-plugin/tests/guests/editor` stays as it is: its word protocol is for
tests. The template is written fresh on the SDK, borrowing the guest's build setup.

## Notes

Building the template needs the `wasm32-wasip2` target (`rustup target add wasm32-wasip2`); the Mac
doesn't have it, helium does (it built `editor.wasm` for T1058). Needs nothing from the app wiring;
it can run now. Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- The crate builds for the host in the workspace, so CI's clippy covers it; no standalone
  `[workspace]`. `template/` keeps its own.
- `generate!` sits in a private `bindings` module, glob re-exported: with `pub_export_macro` the
  world's export macro is `#[macro_export]`ed and also `pub use`d where it is generated, which is
  a duplicate at a crate root (E0255). The paths a plugin sees are unchanged.
- `Plugin` methods take `&mut self` on one `Default` value that `export_plugin!` keeps, since the
  template has to remember the focused buffer for its saves and its command. The value lives in a
  `thread_local` with a `RefCell`; the component model refuses to re-enter a guest, so the borrow
  can't clash.
- The host's `events.subscribe` is still a stub answering `unimplemented` (`kiln_plugin::events`).
  A plugin that failed `init` on it wouldn't load, so the template accepts that one error and
  propagates the others; it still subscribes, for when the import lands.
- Dev-dependencies beyond the spec's three: `fe-buffer`, `kiln-command` and `kiln-core`, for the
  test's `BufferAccess`, grants and `load_manifest`.
- A debug build warns `function signature mismatch: shutdown` at link: the world's `shutdown`
  export shares its symbol with libc's in std. The test guests' world has the same export, and
  `build.zsh`'s LTO release build links without it. Fixing it means renaming the WIT export, which
  is a matter for the preview's next incompatible change, not this card.
