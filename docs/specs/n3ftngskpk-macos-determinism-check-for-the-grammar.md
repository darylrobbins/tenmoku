---
task_uid: "01m71qm2pb153ag0n3ftngskpk"
task: "T1681"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Evidence (macOS determinism): on this Mac, with macOS's BSD tar (tar --version names bsdtar), scripts/build-grammar-wasm.sh then scripts/build-grammar-registry.sh run twice write target/registry/ (artifact directories, index.json, the per-pack and all-languages tarballs) byte-identical across the two runs; the shasum transcript of both runs and the tar version are added to docs/decisions/xb65tqark9-grammar-registry-index.md beside T1481's Linux C3 transcript."
    required: true
    manual: false
  - id: "C2"
    text: "If the two runs differ, scripts/package-language-pack.sh (or build-grammar-registry.sh) is fixed so they match, the fix keeps the GNU tar path's output unchanged, and the decision file says what differed."
    required: true
    manual: false
---

# macOS determinism check for the grammar registry: build-grammar-wasm.sh then build-grammar-registry.sh run twice on the Mac (BSD tar) give byte-identical target/registry/ (T1481 C3 follow-up)

T1481 (argon T325, landed f3c7162f1 in stack 109) proved the grammar registry's output byte-identical across two runs on Linux with GNU tar (its C3). Its spec left the macOS run as a Mac follow-up: scripts/package-language-pack.sh has a separate BSD tar branch (line ~145: --format ustar --uid 0 --gid 0, times and modes set in the stage since BSD tar has no --mtime, names listed sorted since it has no --sort), and nothing has run it twice here.

Run the two builds on the Mac (wasi-sdk from WASI_SDK or ~/.cache/tree-sitter/wasi-sdk, as build-grammar-wasm.sh says), compare with shasum, and record the transcript. Code changes only if the runs differ.

Ask the lead before the wasm build (disk). The same_inputs_give_identical_tarballs test (crates/fastedit/tests/language_pack_per_pack.rs) already passes on macOS on a stub fixture; this card checks the real grammars.
