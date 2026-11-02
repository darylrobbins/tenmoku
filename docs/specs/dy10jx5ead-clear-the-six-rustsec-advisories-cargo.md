---
task_uid: "01m6ca2gd85svn08dy10jx5ead"
task: "T1301"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo deny check advisories passes with deny.toml unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "The implementation notes list each advisory, the version that fixes it, and whether Tenmoku reached the affected code"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax -p fe-markdown-view passes with no existing test edited, including the wasm grammar tests, and cargo deny check bans licenses sources passes"
    required: true
    manual: false
---

# Clear the six RUSTSEC advisories cargo deny reports on main (wasmtime 48.0.7 x3, pulldown-cmark 0.13.4, ctutils 0.4.3, wasip3) by bumping to patched versions

`cargo deny check advisories` fails on main with six RUSTSEC advisories published recently (found by
T1280's worker on 2026-11-01): wasmtime 48.0.7 (RUSTSEC-2026-0422, -0423, -0424), pulldown-cmark
0.13.4 (RUSTSEC-2026-0414), ctutils 0.4.3 (RUSTSEC-2026-0385) and a wasip3 UDP advisory. wasmtime runs
extension wasm grammars (tree-sitter's wasm feature, vendored at vendor/tree-sitter) and plugins, so
its advisories matter most; read each advisory and say in the notes whether Tenmoku reaches the
affected code.

Do: bump each affected crate to its first patched version (directly, or through the crate that pulls it
in: `cargo tree -i <crate>`), adapting call sites and the vendored tree-sitter's wasmtime dependency as
needed with no behaviour change. Do not add ignores to deny.toml (a policy file): if an advisory has no
patched release, or a bump forces a large rewrite (e.g. a wasmtime major that tree-sitter 0.27 can't
use), stop and report which, with the options.

## Implementation notes

All six have patched releases in the versions Tenmoku already uses, so this is a lockfile bump with no code change.
`Cargo.lock` moves the whole wasmtime 48 family to 48.0.8 (cranelift 0.135.8), pulldown-cmark to 0.13.5 and ctutils to
0.4.4, the first patched version of each. `Cargo.toml` raises the floors of the crates it names (`wasmtime`,
`wasmtime-wasi` 48.0.8, `pulldown-cmark` 0.13.5). The vendored tree-sitter keeps `wasmtime-c-api-impl = "48.0.1"`:
it resolves to 48.0.8, and that crate pins `wasmtime` to its own exact version, so the workspace floor covers it.
`deny.toml` is unchanged.

| Advisory | Crate | Fixed in | Does Tenmoku reach the affected code? |
|---|---|---|---|
| RUSTSEC-2026-0422 (GHSA-3982-jqvv-277m): panic on `TypedFunc` with `I31` or `NoneRef` | wasmtime | 48.0.8 | No. The advisory excludes embedders that don't use those types and component-only embedders. No crate names them. kiln-plugin's `get_typed_func` is the component API (`runner.rs`), and `wasm_store.c` calls grammars through `wasmtime_func_call`/`_unchecked`. |
| RUSTSEC-2026-0423 (GHSA-qw58-7h65-crq2): cross-engine type confusion through `StructType::with_finality_and_supertype` | wasmtime | 48.0.8 | No. There are two engines (grammars and plugins), but the host must create the struct type and guests can't trigger it. No crate creates one, and `wasm_store.c` calls none of the C API's struct functions. |
| RUSTSEC-2026-0424 (GHSA-6fr9-xmw9-5352): unaccounted host memory for lent borrows of a component's own resources | wasmtime | 48.0.8 | Not in the shipped app today, but it would be once plugins load. kiln-plugin runs third-party components with the component model on, and nothing rejects a nested component, so one `.wasm` could carry the two components the upstream test uses. `StoreLimiter`'s memory cap doesn't count this memory; only the call deadline bounds it. No non-test code calls `Plugin::load` yet, though. Grammars are core modules, so they don't reach it. Read from the source and the fix (wasmtime PR 15075); no exploit was tried. |
| RUSTSEC-2026-0420 (GHSA-hmmr-87qq-258j): wasip3 UDP `send` raced against `connect` | wasmtime-wasi | 48.0.8 | No. The linker adds only `wasmtime_wasi::p2`, and the advisory says p2 sockets aren't affected. `fs::configure` also sets `allow_udp(false)` and a `socket_addr_check` that refuses every address. |
| RUSTSEC-2026-0414: exponential event growth with `ENABLE_WIKILINKS` | pulldown-cmark | 0.13.5 | No. `fe-markdown/src/parse.rs` enables only tables, task lists and strikethrough; parsing without the option isn't affected. It is the only crate using pulldown-cmark. |
| RUSTSEC-2026-0385: non-constant-time masks and `u128` comparisons | ctutils | 0.4.4 | No. It isn't built on macOS or Windows. On Linux and the BSDs it comes through `digest`'s `mac` module, where `CtEq` serves MAC verification. `secret-service` only calls `Hkdf::extract` and `expand`, and `hkdf` and `hmac` have no verify call. |

Compiled plugin components cached by 48.0.7 stop deserializing after the bump. `cache.rs` already treats that as a
miss and recompiles.

Checked: `cargo deny check advisories` and `cargo deny check bans licenses sources` pass, as do `cargo nextest run -p
fe-syntax -p fe-markdown-view` (387 tests, including `runtime_grammar` and `wasm_table_validation`) and `-p kiln-plugin
-p fe-plugin` (138 tests). No test was edited.
