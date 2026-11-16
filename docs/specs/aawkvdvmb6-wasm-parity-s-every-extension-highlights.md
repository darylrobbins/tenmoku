---
task_uid: "01m7hx3qs0g6mmtxaawkvdvmb6"
task: "T402"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "every_extension_highlights_its_sample registers and checks each language with a fresh WasmStore (its own thread or equivalent), and scripts/build-grammar-wasm.sh (all languages) passes for vim and zig"
    required: true
    manual: false
  - id: "C2"
    text: "The decision note in the hand-in says whether GRAMMAR_LANGUAGES=vim,zig alone passed before the fix (confirming the shared-store cause), and whether the app's long-lived parse threads can reach the same 128 MiB cap with many languages open"
    required: true
    manual: false
---

# wasm_parity's every_extension_highlights_its_sample checks each language on its own thread: one thread's WasmStore fills its 128 MiB memory after Verilog, so vim and zig get no tree (Mac T1892)

## Implementation notes

- `every_extension_highlights_its_sample` now runs each language's `check_samples` on its own scoped thread, so each gets a fresh thread-local `WasmStore`.
- Before the fix, `GRAMMAR_LANGUAGES=vim,zig` alone passed (measured), which confirms the shared-store cause: the full run's single store filled its 128 MiB after Verilog. After the fix, `scripts/build-grammar-wasm.sh` (all languages) passes, 126s.
- The app's parse threads cache one parser, and so one `WasmStore`, per thread (`runtime.rs` `PARSER`). A long-lived thread that parses many big wasm languages can therefore reach the same cap. Not changed here; not measured in the app.
