---
task_uid: "01m6z25fqjnpmq4nrz04n4f4zf"
task: "T312"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --test wasm_table_validation --test runtime_grammar --test wasm_empty_eof_state --test wasm_accepted_stack passes and leaves no fe-syntax-* folder in the temp dir afterwards"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench's palette_recent tests pass and leave no palette_recent_* folder in the temp dir afterwards"
    required: true
    manual: false
  - id: "C3"
    text: "Only test code changes"
    required: true
    manual: false
---

# Tests leak per-process scratch folders in the temp dir: fe-syntax wasm_table_validation, runtime_grammar, wasm_empty_eof_state, wasm_accepted_stack and fe-workbench palette_recent never remove theirs (Mac T1626)

Five test helpers write scratch files under `std::env::temp_dir()` in a folder named by the process id and never remove it. Under nextest every test is its own process, so each test run leaves one folder per test behind. On helium (a Linux host whose /tmp is a 32G tmpfs) they had piled up to 8131 `fe-syntax-table-validation-<pid>` folders (~218M), 2222 `fe-syntax-runtime-grammar-*`, 373 `fe-syntax-empty-eof-*`, 373 `fe-syntax-accepted-stack-*` and 621 `palette_recent_*`.

Where:
- crates/fe-syntax/tests/wasm_table_validation.rs `scratch()` (OnceLock, ~line 397)
- crates/fe-syntax/tests/runtime_grammar.rs `scratch()` (OnceLock, ~line 60)
- crates/fe-syntax/tests/wasm_empty_eof_state.rs (~line 123)
- crates/fe-syntax/tests/wasm_accepted_stack.rs (~line 155)
- crates/fe-workbench/src/tests/palette_recent.rs `window()` (~line 27)

Fix: make each helper's scratch folder go away when the test ends, with std only (no new dependency: fe-syntax's Cargo.toml is out of scope; fe-workbench already has tempfile as a dev-dependency). A `static` is never dropped, so cleanup behind a OnceLock never runs: remove the folder at the end of each test (a small drop guard the test holds works), or give each test its own folder it removes. Keep any file a registered grammar still reads (queries, the wasm module) alive for as long as the test uses that grammar. Test-only change: no production code.
