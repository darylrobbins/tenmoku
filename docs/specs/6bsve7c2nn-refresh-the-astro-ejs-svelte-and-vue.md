---
task_uid: "01m70dtecfzvkg636bsve7c2nn"
task: "T1656"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Each of the four goldens changes only where an object key in injected JavaScript goes from property to string.special.key, shown by git diff"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test wasm_parity --run-ignored all every_extension_highlights_its_sample passes for all 51 languages with the wasm built by scripts/build-grammar-wasm.sh, transcript in the finish evidence"
    required: true
    manual: false
---

# Refresh the astro, ejs, svelte and vue sample goldens for T1649's string.special.key on keys in injected JavaScript; the stale goldens failed v0.1.2's grammars build, so the release has no language pack

The v0.1.2 release has no language pack: grammars.yml (run 42638019085, attempt 2) failed in "Build the grammars and check them against the golden files":
crates/fe-syntax/tests/wasm_parity.rs:415 every_extension_highlights_its_sample: "4 of 51 languages fail".
- astro: sample.astro.golden line 194: 34:32-38 property "hidden" -> string.special.key "hidden"
- ejs: sample.ejs.golden line 140: 19:38-42 property "user" -> string.special.key "user"
- svelte: sample.svelte.golden line 150: 28:18-20 property "id" -> string.special.key "id"
- vue: sample.vue.golden line 15: 4:35-39 property "done" -> string.special.key "done"
Cause: T1649 (14277d58a) made object keys in injected JavaScript string.special.key; these four grammars inject JS, and their goldens predate it. The test is #[ignore] (it needs the built wasm), so no gate ran it.

Do:
1. Build the wasm (scripts/build-grammar-wasm.sh for astro ejs svelte vue, or all) and rerun with UPDATE_GOLDEN=1 for those four only; review that each diff is only property -> string.special.key on object keys (no other capture changes). If anything else moved, stop and report.
2. Run the whole ignored test for every language to show nothing else is stale (51 of 51 pass).
3. Say in the finish notes how long the full run takes on this Mac, so the lead can decide on running it in a gate.

## Implementation notes

- Diffs: 19 lines across the four goldens, all `property "x"` -> `string.special.key "x"` on object keys in injected JS.
- Full `scripts/build-grammar-wasm.sh` (all 51 languages, cold build of the wasm plus the test) took 6m41s wall on this Mac; the test alone was 237 s (the first 4-language run's test took ~2 s).
- Full ignored test passed for 51 of 51.
