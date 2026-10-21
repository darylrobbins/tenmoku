---
task_uid: "01m5fv8410w8n5kzyrrwxb9fx7"
task: "T261"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "limits::the_default_limits_are_the_named_constants: Limits::default() equals CALL_DEADLINE 100 ms, MEMORY_BYTES 64 MiB, EVENT_QUEUE 256 and MAX_READ_BYTES 16 MiB"
    required: true
    manual: false
  - id: "C2"
    text: "limits::an_infinite_loop_is_interrupted: a call into loop_forever.wat with a 20 ms deadline returns Timeout within 1 s and the plugin is Unloaded(Timeout)"
    required: true
    manual: false
  - id: "C3"
    text: "limits::memory_growth_past_the_limit_fails: grow_memory.wat asked for more than a 2 MiB test limit gets the grow refused (returns -1) and the plugin keeps running"
    required: true
    manual: false
  - id: "C4"
    text: "limits::a_trap_unloads_the_plugin_with_a_diagnostic: trap.wat unloads with Unloaded(Trap) naming the plugin id, and the thread has exited"
    required: true
    manual: false
  - id: "C5"
    text: "limits::an_unloaded_plugin_never_runs_again: every call after an unload returns Err(Unloaded) without running guest code"
    required: true
    manual: false
  - id: "C6"
    text: "limits::calls_run_on_the_plugin_thread: the guest-visible work runs on a thread other than the caller and dropping the Plugin stops that thread"
    required: true
    manual: false
  - id: "C7"
    text: "limits::wasi_starts_empty: the WASI context has no preopens, an empty environment and no inherited stdio"
    required: true
    manual: false
  - id: "C8"
    text: "limits::a_component_over_the_byte_cap_is_refused: bytes over 16 MiB are LoadError::TooLarge without compiling"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Run each plugin in its own Store on its own thread under a 100 ms epoch deadline and a 64 MB memory limit, and unload it with a diagnostic on a trap or overrun

## Problem

Plugins must be bounded by construction (§7 Sandbox): CPU by epoch interruption, memory by `StoreLimits`, each
plugin in its own `Store` off the main thread, and a call that traps or overruns unloads the plugin with a
diagnostic. `crates/fe-syntax/src/runtime.rs` does this for grammars on tree-sitter's engine; plugins need
the same on their own engine, for components.

## Change

- `kiln/crates/kiln-plugin/src/limits.rs`: `pub const CALL_DEADLINE: Duration = 100 ms`, `MEMORY_BYTES = 64 << 20`,
  `EVENT_QUEUE = 256`, `MAX_READ_BYTES = 16 << 20`, and `pub struct Limits { call_deadline, memory_bytes,
  event_queue, max_read_bytes }` whose `Default` is those constants (settings override them in a later card;
  the type is `Copy` and takes no settings dependency).
- `src/engine.rs`: `PluginEngine`, one process-wide `wasmtime::Engine` with `epoch_interruption(true)` and a
  ticker thread (`wasm-plugin-epoch`) advancing the epoch every few milliseconds only while a call is running,
  modelled on `fe-syntax/src/runtime.rs` (`EPOCH`, the in-flight counter and condvar).
- `src/runner.rs`: `Plugin::load(engine, component_bytes, id, grants, limits) -> Result<Plugin, LoadError>`
  starts one named OS thread owning the `Store<HostState>` (with `StoreLimits` for memory, tables, instances),
  instantiates the world and calls `init` under the deadline. `Plugin::call(...)` sends a closure-free typed
  request over a bounded channel and returns its result, never running guest code on the caller's thread.
  A trap, a deadline overrun, an out-of-memory refusal that traps, or an invalid response moves the plugin to
  `PluginState::Unloaded(Diagnostic { plugin, reason: Trap | Timeout | Memory | Protocol, message })`, drops the
  store and thread, and every later call returns `Err(Unloaded)` without running anything. Dropping a `Plugin`
  stops its thread. Component bytes over 16 MiB are `LoadError::TooLarge`.
- Hooks for later cards, called from `runner.rs` and defined as stubs in their own files: `fs::configure(&mut
  WasiCtxBuilder, &Grants, &Workspace)` (P13), `compat::link(&mut Linker)` (P16), and wasmtime-wasi added to the
  linker with an empty context here: no preopens, empty environment, no arguments, stdio not inherited.

## Notes

Fixtures are `.wat` under `kiln/crates/kiln-plugin/tests/fixtures/`; use short test limits (a few ms deadline)
so every test stays under 2 s. Do not add a compiled-component cache (P18) or any host import beyond
`log` and the P2 stubs.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Implementation notes

- The deadline is an `Instant` in the store data checked by `epoch_deadline_callback` on each tick (2 ms while
  any call runs), so a busy shared epoch never cuts a call short and an idle plugin costs nothing.
- Review round 1: epoch checks never reach a guest blocked in a host import (a WASI pollable). WASI is linked
  async and every export is called with `call_async` (wasmtime 48 mixes sync and async on one store, so the
  generated sync `call_*` are bypassed for typed funcs looked up by name), on wasmtime-wasi's ambient tokio
  runtime, raced against the deadline; the epoch ticker wakes waiting calls each tick. A call that returns
  after its deadline (a slow last host import) is a Timeout too. Scope widened for `tests/fixtures/block.wat`.
- The memory cap is enforced across all of a store's memories by `limits::StoreLimiter`, a `ResourceLimiter`
  wrapping `StoreLimits` (which caps each memory, tables and instances); a refusal is noted so a trap that
  follows it reads as `Reason::Memory`. A refused `memory.grow` returns -1 and is not itself a failure. A growth
  it allowed that then fails to allocate gives its charge back; wasmtime's type-limits failure, reported without
  asking the limiter, is told apart by its message so it never gives back an earlier, successful growth.
- `Plugin::load` logs under the plugin id; `Plugin::load_logging_to` takes a `LogSink`, which runs on the
  plugin thread (the tests use it to see the thread and to see the store dropped).
- `fs::Workspace` is a placeholder (the roots `fs.*:workspace` reach); `load` passes an empty one until P13.
- `runner::wasi_context` is public so tests can check the empty WASI context through wasmtime-wasi's host traits.
- Tests use a 250 ms deadline, not a few ms: at load ~38 on the gate, instantiating block.wat overran 20 ms.
  Every hostile call is still bounded at 2 s by the test itself.
