# tree-sitter-json-words.wasm

A second healthy grammar module, beside `crates/fe-syntax/tests/fixtures/tree-sitter-json.wasm`,
whose highlights differ. It is named `json` (the loader takes the grammar name
from the file name, `tree-sitter-json.wasm`, so a swapped-in module must export
`tree_sitter_json` too) but its `string` is a run of lowercase letters and
`other` is any other single character, so `{"a": 12}` has one `string` at bytes
2..3 where JSON has `"a"` at 1..4. `extension_grammar_swap.rs` copies it over
the JSON module under the same `(string) @string` query.

Built once with the tree-sitter CLI 0.27.0 (which fetched wasi-sdk itself):

```sh
tree-sitter generate && tree-sitter build --wasm -o tree-sitter-json.wasm .
```

from this `grammar.js`:

```js
module.exports = grammar({
  name: "json",
  rules: {
    source_file: ($) => repeat(choice($.string, $.other)),
    string: (_) => /[a-z]+/,
    other: (_) => /[^a-z]/,
  },
});
```

SHA-256 `c91080816bc432716838414a15fa69682d8cddf11c38280207c9219ecd8f77d6`.
