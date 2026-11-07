---
task_uid: "01m6tjw275djxbemf4atjgw7nc"
task: "T317"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/fsharp/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh fsharp passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for fsharp, with test/sample.fs parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with fsharp present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.fs.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add F# syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/fsharp) (Mac T1458)

# F# syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/fsharp/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/ionide/tree-sitter-fsharp, directory `fsharp/src`. Pin: main (HEAD aefd0c87)
- Licence: MIT. External scanner: scanner.c, includes ../../common/scanner.h. Parser size: 53 MB.
- File extensions: fs, fsi, fsx. File names: -. Shebangs: -.
- Comments: line `//`, block `(* *)`.
- Queries: upstream queries/highlights.scm
- Notes: largest parser here: block if the wasm exceeds 16 MiB or misses the load deadline. `.fsi` signatures parse with the same grammar? check; drop fsi if not

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_fsharp"`, `name = "F# syntax"`,
  one `[[contributes.grammars]]` with `language = "fsharp"`, the extensions,
  file names, `line_comment`/`block_comment` and `shebangs` above (keys from
  the comment-tokens card), and `earlier_patterns_win` matching the query's
  convention (tree-sitter style: true; nvim style: false). No `packs` or
  other new keys: pack membership comes from a later card.
- `highlights.scm`: self-contained (inline anything a `; inherits:` line
  pulls in, adapted to this grammar's node names); captures in fe-syntax's
  vocabulary (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES). Drop
  patterns using predicates fe-syntax doesn't support rather than leaving
  them inert.
- `injections.scm`: only injections into languages that exist (built-ins, or
  first-party ones listed in `parity_needs`); a comment-only file if none.
  No locals, folds, indents or outline queries (no first-party extension
  ships them and the manifest has no keys for them).
- `test/sample.fs`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.fs.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh fsharp`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/fsharp/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned main's head, `aefd0c8741bdf3aeb827a228aa4a996a6536697e` (tagged
  0.3.12). The wasm is 11.5 MB (under `MAX_GRAMMAR_BYTES`) and loads and
  highlights the sample in about 4 s on the Linux lane host.
- `.fsi` dropped: upstream gives it the separate `fsharp_signature` grammar.
  A signature file (`val area : shape: Shape -> float`) parses under
  `fsharp` with no ERROR node but as an expression: `val` comes out a
  function and `shape` a type.
- `highlights.scm` is upstream's, later patterns winning
  (`earlier_patterns_win = false`); `@spell` dropped, `@character.special`
  (which fe-syntax would read as `string`) made `@punctuation.special` or
  dropped, `@variable.parameter.builtin` made `@variable.builtin`.
- `injections.scm` is comments only: upstream's patterns inject into
  `comment` (absent) or into markdown/xml through `#offset!`, which
  fe-syntax doesn't support.
- Review round 1: interpolations' code took the string's colour (upstream
  captures the whole `format_string` as `@string`); `(format_string_eval)
  @embedded`, after that pattern, marks it as code (as Elixir's
  interpolations are; a non-built-in query may not use `@none`), keeping
  the braces' and inner tokens' own colours. The sample covers `$"…"`, `$@"…"` and `$"""…"""`.
  The grammar reads `\{` in a `$@"…"` string as one character (its
  `_verbatim_string_char` includes `_non_escape_char`), so
  `$@"C:\{name}"` misplaces the hole; that's upstream's parser, out of
  scope here, and the sample avoids it.
