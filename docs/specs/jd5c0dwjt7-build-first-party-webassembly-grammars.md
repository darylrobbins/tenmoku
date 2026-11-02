---
task_uid: "01m6e54khqxmanddjd5c0dwjt7"
task: "T1304"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/{kotlin,csharp,scala,swift} each hold extension.toml, grammar-source.toml (Cargo.lock pin), highlights.scm, injections.scm and LICENSE; no .wasm is committed"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh builds the four wasm files from the Cargo.lock-pinned crate sources with wasi-sdk clang into target/grammar-wasm/, then runs wasm_parity's ignored tests"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-syntax/tests/wasm_parity.rs: kotlin, csharp, scala and swift each load their wasm through fe-syntax's wasm loader and highlight their golden sample identically to its .golden file; a missing wasm fails naming the script"
    required: true
    manual: false
  - id: "C4"
    text: ".github/workflows/grammars.yml builds the wasm on tags and workflow_dispatch and uploads them as release assets; THIRD_PARTY.md lists the four with pin and licence"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/build-grammar-wasm.sh passes on this Mac; cargo nextest run -p fe-syntax passes"
    required: true
    manual: false
  - id: "C6"
    text: "ts_wasm_language__validate checks supertype_map_entries against name_count (symbols plus aliases), as the alias checks do, so tree-sitter-c-sharp's alias entry loads; wasm_table_validation.rs gains a positive test that an alias entry loads, keeps rejecting entries \u003e= name_count, and vendor/tree-sitter/README.md's change list says so"
    required: true
    manual: false
---

# Build first-party WebAssembly grammars for Kotlin, C#, Scala and Swift, with a build script, a CI workflow and wasm-vs-native highlight parity tests

Card 3 of docs/grammar-size-audit/README.md ("Proposed follow-up cards"), first half: Kotlin, C#, Scala and Swift. A second card does C++, SQL, PHP and Ruby on top of this one, so build the shared pieces for all eight languages' shape, not just these four.

For each language, extensions/grammars/<lang>/ holds what the README lists: extension.toml, grammar-source.toml (the crate and version pinned in the root Cargo.toml [workspace.dependencies] / Cargo.lock; C# must name the wasm tree-sitter-c-sharp.wasm, after its tree_sitter_c_sharp export), highlights.scm and injections.scm (what fastedit uses natively for that language today, overrides included, so parity holds), and the grammar's LICENSE. The built .wasm is a CI asset and is never committed.

scripts/build-grammar-wasm.sh (new) builds each listed grammar's wasm from the Cargo.lock-pinned crate source (its src/parser.c and scanner, from the cargo registry) with wasi-sdk clang, the way crates/fe-syntax/tests/fixtures/hostile-grammars.md does (WASI_SDK, default ~/.cache/tree-sitter/wasi-sdk, which exists on this Mac), exporting tree_sitter_<name> as `tree-sitter build --wasm` does. It writes into target/grammar-wasm/ (not the source tree), takes language names as arguments (all when none), and then runs the parity tests: cargo nextest run -p fe-syntax --test wasm_parity --run-ignored only, like scripts/lsp-real.sh.

crates/fe-syntax/tests/wasm_parity.rs (new): one #[ignore] test per language that loads target/grammar-wasm/<file>.wasm through fe-syntax's wasm grammar loader (the same path user grammars take, table validation included) with the extension's queries, highlights the existing golden sample (tests/golden/<lang>.<ext>) and asserts the result equals the sample's .golden file. The goldens are read, never changed. A missing wasm fails the test with a message naming the script, rather than passing.

.github/workflows/grammars.yml (new): on tags and workflow_dispatch, installs wasi-sdk, runs the script, and uploads the eight wasm files as release assets (the four this card adds now; the second card adds its four).

THIRD_PARTY.md: list each grammar shipped as a wasm asset, with pin and licence.

No change to the default features or the built-in set (that is card 4). Record design decisions in docs/decisions/<lane>-*.md.
