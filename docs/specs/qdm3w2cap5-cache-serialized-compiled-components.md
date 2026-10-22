---
task_uid: "01m5j50txj7db0gdqdm3w2cap5"
task: "T281"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "C1 cache::the_second_load_uses_the_cache: loading the same bytes twice compiles once (counter) and writes one entry"
    required: true
    manual: false
  - id: "C2"
    text: "C2 cache::changed_bytes_get_a_new_entry: different component bytes do not hit the old entry"
    required: true
    manual: false
  - id: "C3"
    text: "C3 cache::a_corrupt_entry_is_recompiled_and_replaced: truncating the entry file still loads and rewrites it"
    required: true
    manual: false
  - id: "C4"
    text: "C4 cache::an_entry_for_another_engine_config_is_ignored: an entry written with a different engine config is recompiled, not deserialized"
    required: true
    manual: false
  - id: "C5"
    text: "C5 cache::a_group_writable_directory_or_file_is_not_trusted: after chmod 777 on the dir or file the entry is recompiled"
    required: true
    manual: false
  - id: "C6"
    text: "C6 cache::the_cache_is_pruned_to_its_cap: the 65th distinct entry removes the oldest"
    required: true
    manual: false
  - id: "C7"
    text: "C7 cache::an_unwritable_cache_directory_still_loads: loading works with the cache directory read-only"
    required: true
    manual: false
  - id: "C8"
    text: "C8 cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Cache serialized compiled components under the state cache directory, keyed by content and wasmtime version, and recompile any entry that is corrupt or not ours (Mac T1057)

## Problem

Compiling a component on every launch costs startup time; wasmtime can serialize a compiled component and load it back.
The cache must not be a way to run something the user never granted.

## Change

`kiln/crates/kiln-plugin/src/cache.rs`: `ComponentCache::new(dir)` keyed by SHA-256 of the component bytes plus
wasmtime's version and the engine's config fingerprint; `load(engine, bytes) -> Component` deserializes the entry only if
the directory and file are owned by the user and not group or world writable and `Engine::detect_precompiled` accepts
it, otherwise compiles and replaces it (an `unsafe` deserialize is justified by those checks and its comment).
A corrupt or foreign entry is recompiled, never an error. The cache is pruned to a named cap (64 entries) by age. An
unwritable cache directory still loads. `runner.rs` (P3's `Plugin::load`) takes an optional cache.

## Notes

The grant check stays before any load; the cache only skips compilation. macOS and Linux file modes.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.
