---
task_uid: "01m7c01dcpy66xzp49fmnqfbbf"
task: "T631"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --test wasm_parity every_extension_highlights_its_sample --run-ignored only passes for dart; test/sample.dart.golden shows property on a call's named-argument label and variable.parameter on a declared parameter"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test wasm_parity every_extension_is_well_formed passes"
    required: true
    manual: false
---

# Highlight Dart named arguments as property (extension query) (Mac T1820)

# Highlight Dart named arguments as properties

Decision 2026-11-13: a name that labels a value is `property`. Dart's extension query
`extensions/grammars/dart/highlights.scm` (line ~227) captures a call's `named_argument` label as
`@variable.parameter`, plain text in several themes. Change that pattern (`(named_argument (label (identifier)
@property))` or the equivalent the file has; keep it positioned so it wins over the `variable.parameter` capture on
the same node) so call-site `f(count: 3)` colours `count` as property. Declaration parameters (`void f({int count})`)
stay `variable.parameter`; check by reading the sample.

Shipping: this edits an extension under `extensions/grammars/`, delivered by the grammar registry's separate release,
not by an app build. The wasm is unchanged, so no `pack.toml` edit; the recorded hashes are of the wasm files.
Dart is wasm-only (no native feature), so there is no `wasm_parity` built-in arm.

Update `extensions/grammars/dart/test/sample.dart` only if it has no call with a named argument (add one), then
regenerate `test/sample.dart.golden` by the procedure `extensions/grammars/README.md` gives for goldens
(`wasm_parity`'s `every_extension_highlights_its_sample`, ignored by default: run it with `--run-ignored all`
filtered to dart, on a host that has the wasm, with `UPDATE_GOLDEN=1` if that is its blessing switch; read the test's
doc comment first). Review the diff: only named-argument labels move to `property`.

**Test.** `cargo nextest run -p fe-syntax --test wasm_parity every_extension_highlights_its_sample --run-ignored
only` passes for dart with the regenerated golden; the golden lists `property` for the named-argument label and
`variable.parameter` for a declared parameter. The golden itself is the regression test (it fails at base on the
unchanged `variable.parameter` label), so there is no separate `--regression-test` name; no new test file.

Linux-OK if the wasm test runs there (it does on lanes with the wasm toolchain per T1476); otherwise the lane host
decides.
