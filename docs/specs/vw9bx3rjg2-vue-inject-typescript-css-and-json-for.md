---
task_uid: "01m6vd04a25pjnngvw9bx3rjg2"
task: "T515"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "vue_unquoted_lang_and_type_values_pick_the_injected_language (crates/fe-syntax/tests/vue_injection_selectors.rs, #![cfg(feature = \"html\")]): vue's injections.scm selector patterns, compiled against the native HTML grammar, give \u003cscript lang=ts\u003e typescript, lang=tsx tsx, lang=js and lang=jsx javascript, \u003cstyle lang=css\u003e css, \u003cscript type=module\u003e javascript, \u003cscript type=importmap\u003e json and \u003cstyle type=text/css\u003e css, exactly one language per block"
    required: true
    manual: false
  - id: "C2"
    text: "vue_quoted_lang_and_type_values_still_pick_the_injected_language: the same cases quoted, plus bare \u003cscript\u003e javascript and \u003cstyle\u003e css, each exactly one language"
    required: true
    manual: false
  - id: "C3"
    text: "every lang/type selector in extensions/grammars/vue/injections.scm (lines 41, 63, 75, 96, 109, 121, 133 at the base) takes a bare attribute_value or a quoted_attribute_value, and the header's Changes list says so"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.vue gains an unquoted \u003cscript lang=ts\u003e and \u003cstyle lang=css\u003e block; UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh vue regenerated test/sample.vue.golden, whose diff colours those blocks as TypeScript and CSS and changes nothing else; GRAMMAR_LANGUAGES=vue scripts/build-grammar-wasm.sh then passes (wasm_parity's ignored every_extension_highlights_its_sample, no ERROR or MISSING node)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets -- -D warnings, scripts/check-syntax-features.sh, and cargo nextest run -p fe-syntax -p fastedit pass"
    required: true
    manual: false
---

# Vue: inject TypeScript, CSS and JSON for unquoted lang= and type= values (<script lang=ts>), not only quoted ones (Mac T1446 F1)

Carried medium from T502 F1 (Mac T1446), filed at the Mac lead's request.

# Vue: inject TypeScript, CSS and JSON for unquoted lang= and type= values

## Problem

T502's review (F1, medium) found that every `lang`/`type` selector in
`extensions/grammars/vue/injections.scm` matches only a quoted value:

    (attribute
      (attribute_name) @_lang
      (quoted_attribute_value
        (attribute_value) @_ts))

at `injections.scm:41` (`<style type="text/css">`), `:63` (`<script type="importmap">`),
`:75` (`<script type="module">`), `:96` (`<style lang="css">`), `:109` (`<script lang="js|jsx">`),
`:121` (`<script lang="ts">`) and `:133` (`<script lang="tsx">`).

HTML (and tree-sitter-vue) also allows an unquoted value: `<script lang=ts>` parses as
`(attribute (attribute_name) (attribute_value))`, with no `quoted_attribute_value`. Such a block gets no
injection at all: the explicit patterns can't match it, and the default `<script>`/`<style>` patterns
(`injections.scm:30-35`, `:50-55`) refuse any start tag with ` lang=` or ` type=` in it. So
`<script lang=ts>const count: number = 1</script>` and `<style lang=css>.item { color: red; }</style>`
are shown as plain text. The sample (`extensions/grammars/vue/test/sample.vue:35`, `<script setup lang="ts">`)
only exercises the quoted form.

## Change

- In `extensions/grammars/vue/injections.scm`, let each of the seven selectors above take the value bare or
  quoted, with an alternation that captures the same name either way, e.g.

      (attribute
        (attribute_name) @_lang
        [
          (attribute_value) @_ts
          (quoted_attribute_value (attribute_value) @_ts)
        ])

  The directive patterns at the end of the file (`injections.scm:146-153`) already take both forms and show
  the shape. Keep each pattern's predicates and `injection.language` as they are, and keep the header's
  "Changes:" list accurate (add a line saying unquoted lang/type values are accepted, unlike upstream).
- Each `<script>`/`<style>` block must still get exactly one injection: an unquoted `lang=ts` must not also
  match the default JavaScript pattern (it doesn't today: the `#not-match?` on ` lang=` covers both forms;
  keep it that way).
- Add unquoted cases to `extensions/grammars/vue/test/sample.vue` (e.g. a second, non-setup
  `<script lang=ts>` block and a `<style lang=css>` block beside the existing quoted ones) and regenerate
  `test/sample.vue.golden` with `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh vue`. That script builds the
  wasm (and the sample's parity_needs) into `target/grammar-wasm/` and rewrites the golden; review the golden's
  diff by hand: the new blocks must show TypeScript and CSS colours, and nothing else may change. Don't run
  `tree-sitter parse` in a cloned grammar repo, and don't build in `/tmp`.

## Tests

The golden check (`crates/fe-syntax/tests/wasm_parity.rs:393`, `every_extension_highlights_its_sample`) is
`#[ignore]`d and needs the wasm built, so the gate's regression check (`cargo nextest ... -E test(=…)`, no
`--run-ignored`) can't run it. The regression test instead runs vue's own selector patterns on the native
HTML grammar, which has the same `script_element`/`style_element`/`start_tag`/`attribute`/`attribute_name`/
`attribute_value`/`quoted_attribute_value`/`raw_text` nodes:

New integration test file `crates/fe-syntax/tests/vue_injection_selectors.rs`, `#![cfg(feature = "html")]`
(scripts/check-syntax-features.sh builds the tests with each feature alone):

- Read `extensions/grammars/vue/injections.scm` (path from `CARGO_MANIFEST_DIR`, as wasm_parity does), split
  it into top-level patterns (balance parentheses, skipping `;` comments and string literals), keep those that
  name no vue-only node (`interpolation`, `directive_attribute`), and compile them with
  `tree_sitter::Query::new` against `tree_sitter_html::LANGUAGE`. Fail if none is kept or one doesn't compile.
- For each document, parse it with the HTML grammar, run `QueryCursor::matches` (which applies `#eq?`,
  `#match?`, `#not-match?` and `#any-of?`), and collect `injection.language` (from
  `query.property_settings(pattern_index)`) for each match whose `@injection.content` is the block's
  `raw_text`.
- `vue_unquoted_lang_and_type_values_pick_the_injected_language` (the regression test): `<script lang=ts>` →
  `typescript`, `<script lang=tsx>` → `tsx`, `<script lang=js>` and `<script lang=jsx>` → `javascript`,
  `<style lang=css>` → `css`, `<script type=module>` → `javascript`, `<script type=importmap>` → `json`,
  `<style type=text/css>` → `css`; each block exactly one language.
- `vue_quoted_lang_and_type_values_still_pick_the_injected_language`: the same cases quoted, plus bare
  `<script>` → `javascript` and `<style>` → `css`, each exactly one language (passes at the base too).

## Not in scope

- Unquoted `on*` event-handler attributes (`injections.scm:82-87`, `<button onclick=go()>`): not a lang/type
  selector and not part of the finding.
- Svelte's or any other grammar's queries (T503 is still in flight on `extensions/grammars/svelte/**`).
- Languages fe-syntax has no built-in for (less, scss, sass, pug): still dropped, as the header says.
