---
task_uid: "01m6z5dmp90mnzcee3rvjrz5jx"
task: "T1649"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --test code_keys passes: users_tsx_snippet_keys_are_keys (the user's fromBook TSX snippet verbatim: keys isbn, title, authors, publishedYear are string.special.key; b.isbn13, b.isbn10, b.title, b.authors and b.publishedYear's member names stay property; join and toString stay function.method)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test code_keys passes: javascript_and_typescript_keys, rust_struct_keys, go_composite_literal_keys, c_designated_initializer_keys, lua_table_keys and python_has_no_identifier_keys (identifier keys in literals and JS/Rust pattern keys are string.special.key; shorthand, quoted, numeric and computed keys, function-valued pairs, interface/type-literal members, class fields, struct field declarations, JSX attributes, Go map/array literal keys and member/field access keep their highlights)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax passes, including highlights_match_golden_files; under crates/fe-syntax/tests/golden only the javascript.js, tsx.tsx, rust.rs, go.go and lua.lua goldens change (typescript.ts and c.c only if their samples hold a key), and every changed line turns a key into string.special.key"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench --lib semantic_tokens:: passes, including keys::a_property_token_on_a_struct_key_leaves_it_lapis (a server property token over Rust Book { title: … }'s key adds no Foreground decoration; the same token over b.title still paints property) and the existing semantic token tests; Syntax::is_key (or its equivalent) does not use the checked_highlights range cache"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-config passes, including yohen_hakuji_values every_design_role_has_the_design_value with docs/design/design-system.md's Syntax row renamed from 'Data-file key' to 'Key' (#8db1f2, #2457c5, Lapis, still syntax.string.special.key, row count unchanged from T1628's), plus a sentence under the table saying keys in data files and in object/struct/table literals are Lapis while member access and field declarations stay Property"
    required: true
    manual: false
  - id: "C6"
    text: "cargo run -p fe-gpui --example screenshots passes with rewritten goldens; only scenes that show the RUST sample's Self { entries: …, order: …, capacity, len: 0 } row change, and only the entries, order and len glyphs change colour; cargo run -p fe-workbench --example screenshots passes with no golden change"
    required: true
    manual: false
  - id: "C7"
    text: "No file under crates/fe-config/themes, crates/fe-syntax/src/highlight.rs, crates/fe-syntax/queries/go-injections.scm or extensions/ changes, and cargo clippy -p fe-syntax -p fe-workbench -p fe-config --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Keys written in code get Lapis too: object, struct, table and designated-initializer keys in JS/TS/TSX, Rust, Go, C and Lua become string.special.key, member access stays property, and a server's property token no longer paints over a key

## Decision (user, 2026-11-08, verbatim)
"Keys Lapis, access stays" — "Keys written in an object/struct (isbn:, Foo { title: … }, dict keys) get Lapis like data-file keys. Member access (b.title) keeps Shino."

This reverses T1628's "Code properties stay quiet Shino" for keys only. Member and field access stay `property`.

## Why
In the user's TSX screenshot of an object literal (below), the keys look like plain text: Yohen `property` is Shino #dccbb8 next to ink #ede4d6 (Hakuji #3d444d next to #1f2328). T1628 makes data-file keys `string.special.key` (Lapis #8db1f2 / #2457c5). This card puts keys written in code on the same token.

```tsx
function fromBook(b: Book): FormState {
  return {
    isbn: b.isbn13 || b.isbn10,
    title: b.title,
    authors: b.authors.join(", "),
    publishedYear: b.publishedYear?.toString() ?? "",
  };
}
```

## Token
Reuse `string.special.key`, as T1628 does. Add no new highlight name, do not edit crates/fe-syntax/src/highlight.rs, and change no theme file. Every bundled theme already sets `string.special.key`. In most of them it equals `property`, so those themes look the same. The four Catppuccin themes colour it their blue (their JSON key colour), so code keys turn blue there too. That is intended.

## The rule
- A name written in the key position of a key–value pair inside a literal is a key: before `:` in JS/TS, Rust and Go, after `.` and before `=` in a C designated initializer, and before `=` in a Lua table.
- Only identifier keys count. Quoted keys (`{"a": 1}` in JS or Python), numeric keys and computed keys (`[k]: v`, Lua `["k"] = v`) keep their string, number or variable highlight. In code, a quoted key is a string expression. JSON is different: there every key is quoted, and colouring keys is the only way to tell them from values.
- Shorthand (`{ title }`, Rust `Foo { title }`, `let { title } = b`) is a name that is a key and a variable at once. It keeps its current highlight.
- Declarations stay `property`: TS interface and type-literal members (`property_signature`), class fields, and Rust, Go and C struct field declarations. They are a type's fields, not keys written in a value. T1628's `headers_and_code_fields_keep_their_highlights` already pins a Rust struct field definition as `property`.
- Access stays `property`: JS `member_expression` property, Rust and C `field_expression` field, Go `selector_expression` field, Lua `dot_index_expression` field, Python `attribute`.
- A pair whose value is a function keeps `function.method` (JS `{ f: () => 1 }`) or `function` (Lua `{ f = function() end }`), as the crates' queries have it now.

## Changes per built-in language
Each override is a new "Ours" query file in crates/fe-syntax/queries/, like json5.scm, added to the Def in crates/fe-syntax/src/language.rs.
- **JavaScript, TypeScript, TSX**: a new queries/javascript.scm, appended last to all three Defs (earlier_patterns_win is false, so it wins). `(pair key: (property_identifier) @string.special.key)` and `(pair_pattern key: (property_identifier) @string.special.key)`. After them, repeat the crate's `(pair key: (property_identifier) @function.method value: [(function_expression) (arrow_function)])` so function-valued pairs stay methods. JSX attributes (`jsx_attribute`) stay `attribute`.
- **Rust**: a new queries/rust.scm, appended after tree_sitter_rust::HIGHLIGHTS_QUERY. `(field_initializer field: (field_identifier) @string.special.key)` and `(field_pattern name: (field_identifier) @string.special.key)`. `shorthand_field_initializer`, `shorthand_field_identifier`, `field_declaration` and `field_expression` are unchanged.
- **Go**: a new queries/go.scm. Go's Def has earlier_patterns_win **true**, so this file goes **before** tree_sitter_go::HIGHLIGHTS_QUERY, and inside it the exception comes before the rule. A key in Go is an expression (`keyed_element key: (literal_element (identifier))`), and for a map or array it is a variable or constant, not a field name. So:
  1. `(composite_literal type: [(map_type) (slice_type) (array_type) (implicit_length_array_type)] body: (literal_value (keyed_element key: (literal_element (identifier) @variable))))`
  2. `(keyed_element key: (literal_element (identifier) @string.special.key))`

  This makes `Book{Title: x}` and the type-elided `[]Book{{Title: x}}` keys, and leaves `map[string]int{k: 1}` and `[...]string{A: "a"}` alone. A named map type (`type M map[string]int; M{k: 1}`) would read as a key. The grammar can't tell it apart, so this is accepted and noted in the file. Do not touch queries/go-injections.scm (T1642).
- **C**: a new queries/c.scm, appended after tree_sitter_c::HIGHLIGHT_QUERY on the `c` Def only: `(initializer_pair designator: (field_designator (field_identifier) @string.special.key))`. The `cpp` Def (not a default feature; C++ ships as a wasm extension) is unchanged.
- **Lua**: a new queries/lua.scm, appended after tree_sitter_lua::HIGHLIGHTS_QUERY. `(field name: (identifier) @string.special.key)`, followed by a repeat of the crate's `(table_constructor (field name: (identifier) @function value: (function_definition)))`.
- **Python**: no change. Dict keys are expressions, and keyword arguments are parameters, not keys. A test pins this.
- **Java, Bash, HTML, XML**: no change. Java has no object literals. HTML and XML attribute names stay as they are.
- **CSS**: no change. `color:` stays `property`. A declaration is not an object or struct the user named, the change would recolour every line of every stylesheet, and T1628's test pins CSS `color` as `property`. If the user wants it, it can be a follow-up.
- **Out of scope**: languages that ship as wasm extensions (extensions/grammars: ruby, php, cpp, csharp, swift, kotlin, scala, dart and the rest). Ruby `key:` hashes, PHP arrays, C++ and C# initializers and so on go to a follow-up card.

## Semantic tokens must not undo it
`editor.semantic_highlighting` is on by default. crates/fe-workbench/src/lsp_semantic_tokens.rs maps a server's `property` token to a `property` Foreground decoration, which paints over the grammar. typescript-language-server, rust-analyzer and clangd send `property` for object, struct and designated keys, so the user's TSX file would still show Shino.

The fix: in `semantic_tokens_landed`, a token whose highlight name is `property`, over a range the grammar highlights exactly as `string.special.key`, gets no Foreground decoration. Its `mutable` underline, if any, is unchanged. Every other token works as today.

The grammar check is a new uncached method in crates/fe-syntax/src/syntax.rs, for example `Syntax::is_key(&self, range) -> bool`. It runs the host language's highlight query with a cursor limited to `range`, and is true when the winning capture covering exactly `range` is `string.special.key`. It is false without a tree parsed at the current version, and false for injected layers. It must not go through `checked_highlights` or its 8-entry range cache: a call per token would push out the visible rows' entries.

## Design doc
docs/design/design-system.md Syntax table: rename T1628's row `| Data-file key | … | Lapis | |` to `| Key | \`#8db1f2\` | \`#2457c5\` | Lapis | |`. Under the table, add one sentence saying that keys in data files and the keys written in object, struct and table literals are Lapis, while member access and field declarations stay Property. In crates/fe-config/tests/yohen_hakuji_values.rs, rename the ROLES entry from "Data-file key" to "Key" (still syntax.string.special.key). Do **not** change the row count: T1628 already added the row.

## Tests
A new crates/fe-syntax/tests/code_keys.rs, its own test binary like data_formats.rs. It uses the public API only: `language_by_name`, `Syntax::new`, `parse_job`, `finish` and `highlights`. Gate it with `#![cfg(all(feature = "javascript", feature = "typescript", feature = "rust", feature = "go", feature = "c", feature = "lua", feature = "python"))]`. Write one test per language with each key next to its access:
- `users_tsx_snippet_keys_are_keys`: the snippet above, verbatim, in `tsx`. `isbn`, `title` (the key), `authors` (the key) and `publishedYear` (the key) are `string.special.key`. The member names `isbn13`, `isbn10`, `title` (in `b.title`), `authors` (in `b.authors`) and `publishedYear` (in `b.publishedYear`) are `property`. `join` and `toString` keep `function.method`.
- `javascript_and_typescript_keys`: in javascript, typescript and tsx:
  - pair keys and `const { title: t } = b` pattern keys are keys;
  - shorthand `{ isbn }`, `"quoted": 1`, `1: x` and `[k]: v` are not;
  - `{ f: () => 1 }` and `{ g() {} }` are `function.method`;
  - `interface Book { title: string }` and `class A { title = 1 }` member names stay `property`;
  - a JSX attribute stays `attribute`.
- `rust_struct_keys`: in `Book { title: b.title, isbn }` and `let Book { title: t, .. } = b;`, the key `title` is a key. `b.title`'s `title` and `struct Book { title: String }`'s `title` stay `property`. Shorthand `isbn` is not a key.
- `go_composite_literal_keys`: `Book{Title: b.Title}` and `[]Book{{Title: "x"}}` keys are keys. `b.Title`'s `Title` and `type Book struct { Title string }`'s `Title` stay `property`. The key in `map[string]int{k: 1}` and `[...]string{A: "a"}` stays `variable` (or whatever it has today, as long as it is not a key).
- `c_designated_initializer_keys`: in `struct p q = { .x = 1, .y = b.y };`, `x` and `y` before `=` are keys. `b.y`'s `y`, `b->y`'s `y` and `struct p { int x; }`'s `x` stay `property`.
- `lua_table_keys`: in `local t = { title = b.title, f = function() end, ["k"] = 1 }`, `title` is a key, `b.title`'s `title` stays `property`, `f` stays `function` and `"k"` stays `string`.
- `python_has_no_identifier_keys`: in `{"a": 1, k: 2}` and `f(a=1)`, nothing is `string.special.key`, and `b.title`'s `title` stays `property`.

A new crates/fe-workbench/src/tests/lsp_semantic_keys.rs, declared as `mod keys` from crates/fe-workbench/src/tests/lsp_semantic_tokens.rs next to `format_specifier`, built like lsp_semantic_format_specifier.rs. It adds `property` to the fake server's legend. Test `a_property_token_on_a_struct_key_leaves_it_lapis`: src/main.rs holds `Book { title: b.title }`, and the server sends `property` tokens on both `title`s. Only the second `title` (the access) is painted `property`; the first gets no Foreground decoration.

Goldens: `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden` rewrites crates/fe-syntax/tests/golden/*.golden. Expected changes:
- javascript.js: `uses`, `detail`, `score`.
- tsx.tsx: `insert`, `normal`, `visual`, `hour`, `minute`, `flex`.
- rust.rs: `bytes` and `gap` in `Self { … }`.
- go.go: `pending`, `delay`, `sink` in `&Debouncer{…}`.
- lua.lua: `api`.
- typescript.ts and c.c: none expected.

Review the diff: every changed line must be a key switching to `string.special.key`.

Screenshots: Yohen is the default palette, and fe-gpui's RUST sample has `Self { entries: HashMap::new(), order: Vec::new(), capacity, len: 0 }`. `UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots` rewrites the scenes that show that row (highlighted_rust, the RUST-based scenes, and the minimap scenes, which draw every row). Look at each changed golden: only `entries`, `order` and `len` may change colour. The fe-workbench scenes (main.rs has no struct literal; the setup wizard sample has only a field declaration and an access), fe-markdown-view and fe-chat-view must not change. Goldens are rendered with Metal, so this card runs on macOS.

## Stays as is
- highlight.rs and its NAMES.
- Every theme file.
- T1628's data_file_keys.rs.
- queries/go-injections.scm.
- extensions/grammars.
- CSS property names, declarations and member access (see the rule above).

## Implementation notes

- typescript.ts's golden changes too: its sample holds two keys (`hits:` in
  `{ ...entry, hits: entry.hits + 1 }` and `{ value, hits: 0 }`), as C3
  allows. c.c's sample has no designated initializer and doesn't change.
- `Syntax::is_key` queries only the token's range with a fresh cursor, under
  the same per-query budget and panic catch as `highlights`, and is false if
  the query didn't finish. The workbench reads the editor's syntax once per
  answer; a tree not yet parsed at the current version leaves `property`
  tokens painting as before.
- Review round 1: tokens that land before the grammar has parsed their
  version (language still loading, background parse, an undrawn tab) mark
  the slot `keys_unchecked`; the post-draw `sync_semantic_tokens` shows them
  again once the editor's syntax is parsed at that version, with no new ask.
  `Syntax::is_key` now judges the raw captures before flattening, so only a
  key capture of exactly the range counts (not a prefix, suffix or interior
  of one). Lua's bracketed `[k] = v` keeps the crate's `field` (property).
- Review round 2: the key check uses any editor of the buffer whose grammar
  has parsed the tokens' version (a split hidden behind another tab isn't
  drawn and stays behind), and waits only while none has. Tokens waiting for
  that are kept with their version apart from the delta baseline, so a
  refresh that times out (which clears the baseline) no longer drops them.
