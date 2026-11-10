---
task_uid: "01m730n76ebnn61174s8e0hj77"
task: "T1721"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "sample.liquid.golden changes only where a key goes from property to string.special.key (line 2 \"layout\" and any others of that kind)"
    required: true
    manual: false
  - id: "C2"
    text: "After scripts/build-grammar-wasm.sh, cargo nextest run -p fe-syntax --test wasm_parity --run-ignored all every_extension_highlights_its_sample passes for every language"
    required: true
    manual: false
---

# Refresh the liquid sample golden for T1649's string.special.key on its front-matter key; the stale golden failed the first registry publish (grammars.yml run 42839814918 on 5e5de9ace)

The first grammar registry publish has failed, so nothing is published at index_url and T1484 C3 is blocked. grammars.yml ran on main at 5e5de9ace (T1483), run 42839814918, and failed in "Build the grammars and check them against the golden files":

crates/fe-syntax/tests/wasm_parity.rs:415 every_extension_highlights_its_sample: only liquid fails.
  liquid: differs from extensions/grammars/liquid/test/sample.liquid.golden at line 2
  - 2:1-7 property "layout"
  + 2:1-7 string.special.key "layout"

This is the same failure T1656 fixed for astro, ejs, svelte and vue. Liquid (T1638) was recorded on a host without a fresh wasm build. Since T1649 the front-matter key gets string.special.key, so the golden is stale. The same test is #[ignore]d in local gates because it needs scripts/build-grammar-wasm.sh, so only CI caught it.

Rebuild the wasm modules (scripts/build-grammar-wasm.sh; wasi-sdk is on this Mac, see T1681). Then rerun with UPDATE_GOLDEN=1 for liquid only and review the diff. Every changed line must be a key that goes from property to string.special.key. If anything else moves, stop and report it instead of updating. Run the whole test over every language afterwards, so that no other grammar added since v0.1.3 (wgsl vhdl typst scheme glsl handlebars jinja gotmpl razor heex) is also stale.
