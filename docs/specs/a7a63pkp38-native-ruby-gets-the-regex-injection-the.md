---
task_uid: "01m7hx3qk713y5g0a7a63pkp38"
task: "T401"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The native Ruby LanguageDef (language.rs, ruby feature) carries the regex injection that extensions/grammars/ruby/injections.scm has, wired the way PHP's is; an injection test shows a Ruby regex literal's inside highlighted as regex"
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-syntax/tests/golden/ruby.rb.golden is regenerated for it, and GRAMMAR_LANGUAGES=ruby scripts/build-grammar-wasm.sh passes (ruby_extension_is_as_built_in and wasm_parity's ruby check)"
    required: true
    manual: false
---

# Native Ruby gets the regex injection the ruby extension has (T637), so wasm_parity's ruby check matches ruby.rb.golden again (Mac T1891)
