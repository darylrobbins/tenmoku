---
task_uid: "01m6hra1saz9ya6yrng9ep52bx"
task: "T1338"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "PERF.md's WebAssembly grammars section records two quiet-Mac runs of cargo bench -p fe-syntax --bench wasm (1-minute load below 4 at start, start and end loads stated) beside the under-load runs A and B"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md's WebAssembly grammars section has a Thresholds subsection: parse wasm/native at most 2x acceptable, 2x to 3x a warning, above 3x a regression; wasm load under 500 ms; no threshold for highlight or reparse; naming the runs it was set from and whether Kotlin meets each"
    required: true
    manual: false
  - id: "C3"
    text: "No file outside PERF.md changes"
    required: true
    manual: false
---

# Set the WebAssembly grammar thresholds in PERF.md from a quiet-Mac rerun of the Kotlin wasm bench: parse at most 2x native, over 3x a regression, load under 500 ms

Follow-up to T1306. The user approved these WebAssembly grammar thresholds (Nov 3), to be set from a quiet-Mac rerun of the Kotlin bench:

- **Parse:** wasm/native at most 2x is acceptable; above 3x is a regression. Between 2x and 3x is a warning: note it, don't block on it.
- **Load:** the one-off wasm load (compile and instantiate the module, read the files, compile the queries) under 500 ms.
- Highlight and the one-character reparse are recorded but carry no threshold (PERF.md already explains why: highlight is native in both, reparse is noise-bound).

Work:

1. Rerun `scripts/build-grammar-wasm.sh kotlin`, then `cargo bench -p fe-syntax --bench wasm` (as committed by T1306, unchanged) on a quiet Mac: before the run, ask the lead to hold the Mac's other lane, and start only when `uptime`'s 1-minute load is below 4. Record the load at start and end as T1306 did. Two runs (C and D), same as T1306's A/B layout. Ask the lead before any cargo build.
2. In PERF.md's "WebAssembly grammars" section: add the quiet runs to the table (or a second table) next to runs A and B, kept as the under-load figures, and add a "Thresholds" subsection stating the three rules above, which runs they were set from, and whether Kotlin meets them on the quiet runs. Replace "This states the ratios; it picks no threshold." accordingly.
3. If the quiet runs put Kotlin's parse ratio above 3x or its load above 500 ms, still record the numbers and the thresholds, say plainly in PERF.md that Kotlin fails them, and say so in the finish summary so the lead can raise it before T1322 drops the native Kotlin grammar. Don't change the bench or any grammar to make the numbers pass.

No new CI check or automated gate: the thresholds are a documented policy for choosing which grammars ship as wasm, read by people.
