---
task_uid: "01m7hx3qd6n256aa0sppgjf3fe"
task: "T400"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "UPDATE_GOLDEN=1 GRAMMAR_LANGUAGES=astro,blade,ejs scripts/build-grammar-wasm.sh rewrites only string.special.key -\u003e property on injected-JS keys, and the run without UPDATE_GOLDEN passes"
    required: true
    manual: false
---

# Refresh the astro, blade and ejs sample goldens for T632's property on keys in injected JavaScript; the stale goldens fail grammars.yml (run 43731172070) (Mac T1890)
