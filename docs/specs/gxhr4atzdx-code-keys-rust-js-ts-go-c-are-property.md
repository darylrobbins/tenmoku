---
task_uid: "01m7c01dv6wdpcc5gxhr4atzdx"
task: "T632"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --test code_property_keys passes: code_literal_keys_are_properties, data_file_keys_stay_string_special_key"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test code_keys --test data_file_keys --test data_formats passes (code_keys.rs updated to property; the data-file pins unchanged)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax highlights_match_golden_files passes with the code-key goldens regenerated; json, json5, toml, yaml, ini goldens are byte-identical"
    required: true
    manual: false
---

# Code keys (Rust, JS/TS, Go, C) are property; string.special.key stays for data files (Mac T1821)

# Code keys are `property`, not `string.special.key`

Decision 2026-11-13: `string.special.key` stays for data files (TOML, JSON, JSON5, YAML, INI). A name that labels
a value in code is `property`: Rust, JavaScript/TypeScript/TSX, Go and C. Today those four overrides capture
literal keys as `string.special.key`, which T1649's `tests/code_keys.rs` pins. Change them:

- `crates/fe-syntax/queries/rust.scm`: `(field_initializer field: (field_identifier) @string.special.key)` and
  `(field_pattern name: (field_identifier) @string.special.key)` become `@property`.
- `crates/fe-syntax/queries/javascript.scm` (lines 8 and 11, object `pair` keys and the like; TypeScript/TSX reuse it).
- `crates/fe-syntax/queries/go.scm` line 18 (`literal_element` keys of composite literals).
- `crates/fe-syntax/queries/c.scm` line 6 (designated initializer `.x = 1`).

Rewrite each header comment to say "property" and why ("a key written in code labels a value; data files keep
`string.special.key`"). Edit the doc comment at `crates/fe-syntax/src/syntax.rs:546` ("a key in a data file, or one
written in an ...") to say data files only. Do NOT change lua.scm, ini.scm, toml.scm, yaml.scm or json5.scm
(Lua table keys are data-like and are not in this decision; say so in the finish notes if you think they should move).
Quoted/computed keys (`"a": 1`, `[k]: 1`) and shorthand keep what they have today, as `code_keys.rs`'s header says.

Existing pins to update: `crates/fe-syntax/tests/code_keys.rs` (every `KEY` assertion for code becomes `property`;
rename its const and its header). `tests/data_file_keys.rs`, `tests/data_formats.rs`,
`src/tests/query_overrides.rs` and `src/tests.rs` pin data-file keys only: they must pass unchanged.
`src/injection/tests.rs` lines ~212-216 pin yaml keys in a markdown fence: unchanged.

Regenerate goldens (`UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden`, all features that apply): rust.rs,
go.go, javascript.js, typescript.ts, tsx.tsx, plus markdown.md and dockerfile.dockerfile (embedded code). Review: only
`string.special.key` spans of code keys become `property`; data-file goldens (json, json5, toml, yaml, ini) must
not change at all.

**Test.** New file `crates/fe-syntax/tests/code_property_keys.rs`, `#![cfg(all(feature = "javascript",
feature = "typescript", feature = "rust", feature = "go", feature = "c"))]`, `spans` helper copied from
`tests/code_keys.rs`. `code_literal_keys_are_properties`: `{a: 1}` in js and ts, `Point { x: 1 }` and
`let P { y, .. } = p;` field pattern in rust, `Point{X: 1}` in go, `{ .x = 1 }` in c: each key is `property`.
`data_file_keys_stay_string_special_key`: `a = 1` toml and `{"a": 1}` json keys are still `string.special.key`.
Base fails on the first.

Themes need nothing: `property` is already coloured in tenmoku-*, vscode-2026/dark-modern; Hakuji, Yohen and
Catppuccin get their colour in card B (depends on none of this; ordering only affects how people see the result).

Conflicts: shares goldens with F (javascript/tsx/ts) and G1 (rust/go), which depend on this card; no open task lists
these queries. Linux-OK.
## Lead decision on the blocked gate

The four tests in `crates/fe-workbench/src/tests/lsp_semantic_keys.rs` use rust-analyzer's property token over a Rust `Book { title: .. }` key and expect `Syntax::is_key` to take it off; with Rust struct keys now `property` that mechanism has no Rust case. Scope now includes `crates/fe-workbench/src/tests/lsp_semantic_keys.rs` (and the `is_key` use in `crates/fe-workbench/src/lsp_semantic_tokens.rs` if it needs a touch). Preferred: rewrite the four tests around a data-file key (TOML/JSON, which keeps `string.special.key`) if the test fake server can serve one; if that needs more than a small test helper, delete them as moot and say so in the finish note. `let P { y, .. }` shorthand staying unhighlighted and your `y: z` choice are fine. Then rgt finish once.

## Implementation notes

- The spec's Rust field-pattern example `let P { y, .. } = p;` is a shorthand field (`shorthand_field_identifier`), which keeps its highlight, so the new test uses `let P { y: z, .. } = p;`.
- `code_keys.rs::is_key_needs_exactly_a_keys_range` pinned a Rust struct key through `Syntax::is_key`; it now uses a TOML key (is_key means `string.special.key`).
- markdown/dockerfile goldens needed no change (no code keys in them). Lua table keys are unchanged; they are data-like, so I think they can stay.
- Lead decision: deleted the four `crates/fe-workbench/src/tests/lsp_semantic_keys.rs` tests as moot. The fake LSP servers are only rust-analyzer and pyright, so a data-file key (TOML/JSON) can't be served without new server wiring. Rust struct keys are `property` now, so the `is_key` take-off has no Rust case. `is_key` itself is unchanged.

## Implementation notes

- Lua table keys stay as they are (not in this decision); they may deserve a look later.
- The four `lsp_semantic_keys.rs` tests are kept, ported to a TOML file: the fake rust-analyzer is made to serve `toml` (settings reload as in `lsp_aliases_reroute.rs`), a dotted key `f.b.title` keeps `string.special.key`, so a `property` token over it is still taken off, while the same token over the string value stays. Parse-timing, refresh-timeout and split-view assertions are unchanged.
