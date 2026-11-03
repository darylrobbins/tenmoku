---
task_uid: "01m6e54xyjgyx5vw6mn0tyw01b"
task: "T1306"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-syntax/benches/wasm.rs (with a [[bench]] entry) measures native and wasm Kotlin full parse and highlight of the golden sample and a ~5,000-line generated corpus, one incremental reparse, and the wasm load cost"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md's new WebAssembly grammars section records each number native and wasm, the wasm/native ratios, the load cost, machine, commit and how to rerun, without picking a threshold"
    required: true
    manual: false
  - id: "C3"
    text: "cargo clippy -p fe-syntax --benches passes"
    required: true
    manual: false
---

# Measure WebAssembly against native parse and highlight for Kotlin, and record the ratios in PERF.md

Card 1 of docs/grammar-size-audit/README.md ("Proposed follow-up cards"): measure WebAssembly against native parse and highlight on Kotlin, one of the grammars the audit moves out of the binary, before the day-one set is committed.

crates/fe-syntax/benches/wasm.rs (new criterion bench, with a [[bench]] entry in crates/fe-syntax/Cargo.toml) runs, for the native Kotlin grammar and for target/grammar-wasm/tree-sitter-kotlin.wasm (built by scripts/build-grammar-wasm.sh kotlin, from the card this depends on) loaded through fe-syntax's wasm loader:
- full parse and full highlight of the golden sample tests/golden/kotlin.kt and of a larger corpus (the sample repeated to about 5,000 lines, generated in the bench, not committed);
- one incremental reparse after a single-character edit in the middle of the large corpus;
- the one-off cost of loading the wasm grammar (wasmtime compile and instantiate).
A missing wasm makes the bench exit with a message naming the script.

Keep the run short: criterion's quick settings (small sample size and measurement time), one machine, not a soak. Ask the lead before running it.

PERF.md gets a "WebAssembly grammars" section: each measurement native and wasm, the wasm/native ratio, the load cost, the machine and commit, and how to rerun. Do not decide the threshold: state the ratios plainly; the user picks the acceptable factor for card 4.
