---
task_uid: "01m6e54xfcnw2g3bvkgx7c7far"
task: "T1305"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/{cpp,sql,php,ruby} each hold extension.toml, grammar-source.toml (Cargo.lock pin), highlights.scm, injections.scm and LICENSE; no .wasm is committed"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh builds all eight wasm files and wasm_parity's cpp, sql, php and ruby tests highlight their golden samples identically to their .golden files"
    required: true
    manual: false
  - id: "C3"
    text: ".github/workflows/grammars.yml uploads all eight wasm assets; THIRD_PARTY.md lists the four with pin and licence"
    required: true
    manual: false
  - id: "C4"
    text: "scripts/build-grammar-wasm.sh passes on this Mac; cargo nextest run -p fe-syntax passes"
    required: true
    manual: false
---

# Build first-party WebAssembly grammars for C++, SQL, PHP and Ruby, extending the build script, CI workflow and parity tests

Card 3 of docs/grammar-size-audit/README.md ("Proposed follow-up cards"), second half: C++, SQL, PHP and Ruby, on top of the first half (Kotlin, C#, Scala, Swift), which built scripts/build-grammar-wasm.sh, crates/fe-syntax/tests/wasm_parity.rs and .github/workflows/grammars.yml. Follow that card's shape and decision file exactly.

For each language, extensions/grammars/<lang>/ holds extension.toml, grammar-source.toml (the Cargo.lock-pinned crate: tree-sitter-cpp, tree-sitter-sequel (wasm name tree-sitter-sql.wasm as the README says), tree-sitter-php, tree-sitter-ruby), highlights.scm and injections.scm (what fastedit uses natively today, overrides included: see crates/fe-syntax/src/tests/kotlin_sql_overrides.rs for SQL), and the grammar's LICENSE. The built .wasm is never committed.

Add the four languages to scripts/build-grammar-wasm.sh, one #[ignore] parity test each to wasm_parity.rs against tests/golden/{cpp.cpp,sql.sql,php.php,ruby.rb} and their .golden files (read, never changed), the four assets to grammars.yml's upload, and the four grammars to THIRD_PARTY.md. PHP's crate has two grammars (php and php_only): use the one fastedit uses natively.

No change to the default features or the built-in set (that is card 4).
