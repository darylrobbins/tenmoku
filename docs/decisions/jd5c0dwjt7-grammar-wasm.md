# First-party WebAssembly grammars (T1304)

Card 3 of `docs/grammar-size-audit/README.md`, first half. No default feature
or built-in changes; that is card 4.

## Shared pieces, shaped for all eight languages

- **`grammar-source.toml`** has four strings: `crate`, `version`, `grammar` (the
  export `tree_sitter_<grammar>`, which fixes the file name) and `src`. The
  second card's languages fit: tree-sitter-sequel exports `tree_sitter_sql`,
  and tree-sitter-php's grammar is in `php/src`.
- **The script** finds its languages from `extensions/grammars/*/grammar-source.toml`,
  so a new language needs no script or workflow change. The workflow uploads
  `target/grammar-wasm/*.wasm`.
- **`wasm_parity.rs`** needs a test and a `LANGUAGES` row per language, and a
  `*_extension_is_as_built_in` test giving the built-in's query parts.

## Choices

- **Source lookup.** `cargo fetch --locked`, then the registry directory by name
  and version, so cargo has checked the crate's checksum against `Cargo.lock`.
  No `jq` or Python for `cargo metadata`. The script fails if
  `grammar-source.toml` and `Cargo.lock` disagree.
- **Flags.** As `tree-sitter-loader` 0.27 passes them, plus `--sysroot` as
  `hostile-grammars.md` does. `__wasm_call_ctors` isn't exported: these grammars
  have no constructors, and the CLI doesn't export it.
- **Registered as `<language>_wasm`, with no file extensions.** A built-in
  refuses its name and extensions to a registered language, and the built-ins
  stay on until card 4. The module, queries and precedence are the extension's.
- **`injections.scm` has only a comment for all four**, Swift too: the built-ins
  have `injections: None`, though tree-sitter-swift ships an injection query.
  Using it would break parity.
- **Drift tests that run in the gate.** The parity tests are ignored, because the
  gate has no wasi-sdk. Without a module, the other tests check that each
  extension's `highlights.scm` is byte for byte the built-in's, that its file
  extensions select the built-in language, that its pin is in `Cargo.lock`, and
  that a missing module's failure names the script.
- **Kotlin's licences.** Its `highlights.scm` includes `queries/kotlin.scm`,
  adapted from nvim-treesitter, and `THIRD_PARTY.md` says to ship the Apache-2.0
  text with that. So the extension also has `LICENSE-APACHE` and declares
  `MIT AND Apache-2.0`, with `license = "MIT"` on the grammar itself.
- **The workflow** runs on `macos-15`, as `ci.yml` and `lsp.yml` do, where the
  script was checked. wasi-sdk is pinned by version and SHA-256.
- **Manual runs.** From a tag, a run publishes to that tag's release. From a
  branch there's no release to attach to, so it uploads a workflow artifact, as
  `release.yml`'s manual run does.
- **Racing `release.yml`.** Both run on `v*` tags and either can create the
  release, so this one uploads, else creates with the same title and notes, else
  uploads again.

## The validator refused C#

`ts_wasm_language__validate` (`vendor/tree-sitter/src/wasm_store.c`, T1005)
required each supertype map entry to be below `symbol_count`. tree-sitter-c-sharp
0.23.5's map lists `alias_sym_element_binding_expression` (530, with
`SYMBOL_COUNT` 530), which is valid: a supertype's subtypes can be aliases. So the
real grammar failed to load with "invalid supertype map entries". C# is the only
one of the eight grammars with an alias in that table.

The check now compares with `name_count` (symbols plus aliases), as the alias
sequences' does; the lead widened this card's scope for it (revision 2). Entries
are only handed back as symbols to look up in `symbol_names` and `symbol_metadata`,
which span `name_count`, so this stays in bounds. An entry at or past `name_count`
is still an error: `a_supertype_map_entry_past_the_aliases_is_an_error` checks both
with an alias and without, and `a_supertype_map_entry_can_be_an_alias` the
positive case. Each fails if the comparison is put back or loosened by one.
