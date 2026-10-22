---
task_uid: "01m5fv84gmpdmebw0ynbfxf5jt"
task: "T263"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "hostile::unbounded_recursion_traps_and_unloads: a self-calling export exhausts the stack and the plugin is Unloaded(Trap) in under 2 s"
    required: true
    manual: false
  - id: "C2"
    text: "hostile::a_looping_init_is_interrupted: a loop in init makes Plugin::load fail with Timeout at the deadline"
    required: true
    manual: false
  - id: "C3"
    text: "hostile::a_looping_start_function_is_interrupted: a loop in a core module start function is interrupted at instantiation"
    required: true
    manual: false
  - id: "C4"
    text: "hostile::a_huge_memory_grow_is_refused_not_aborted: memory.grow of 4 GiB returns -1 and the host process keeps running"
    required: true
    manual: false
  - id: "C5"
    text: "hostile::a_huge_table_grow_is_refused: table.grow past the table limit returns -1"
    required: true
    manual: false
  - id: "C6"
    text: "hostile::garbage_and_invalid_components_are_load_errors: non-component bytes and a component with an invalid core module are LoadError values, not panics"
    required: true
    manual: false
  - id: "C7"
    text: "hostile::an_oversized_component_is_refused_unread: bytes over 16 MiB generated in the test are LoadError::TooLarge"
    required: true
    manual: false
  - id: "C8"
    text: "hostile::one_hostile_plugin_does_not_stall_another: with one plugin looping, a second plugin answers a call within 200 ms and stays loaded"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add a bounded hostile-plugin suite showing recursion, runaway loops, memory and table growth, and invalid or oversized components cannot hang or abort the host

## Problem

The runner (P3) is only as good as its worst case. Before any real host interface lands, a fixture suite of hostile
plugins must show that none of them can hang, exhaust, abort or poison the host, so regressions in the
limits are caught by tests rather than by users.

## Change

New `kiln/crates/kiln-plugin/tests/hostile.rs` and `.wat` fixtures under `tests/fixtures/hostile/`, one per case,
each test bounded to under 2 s with limits set small in the test: unbounded recursion (stack exhaustion), a loop in
`init`, a loop in a start function, `memory.grow` far beyond the limit, a table grown past its limit, a
component whose core module is invalid, bytes that are not a component, and a component over the byte cap
(generated in the test, not checked in). Two plugins side by side show isolation. If a case exposes a bug in
`runner.rs`, `engine.rs` or `limits.rs`, fix it in this card (those files are in scope).

## Notes

Keep it lean: no soak, no fuzzing, no more than the cases listed. Asserting on `PluginState::Unloaded(..).reason`
is enough; do not assert on wasmtime's message text.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.
