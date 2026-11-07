---
task_uid: "01m6sp8c3mgncsp15nrt5s1whp"
task: "T304"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/perl/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh perl passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for perl, with test/sample.pl parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with perl present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.pl.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Perl syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/perl) (Mac T1440)

# Perl syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/perl/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter-perl/tree-sitter-perl, directory `src`. Pin: a commit on the `release` branch (master has no generated parser.c)
- Licence: MIT. External scanner: scanner.c. Parser size: 29 MB.
- File extensions: pl, pm, t, psgi. File names: -. Shebangs: perl.
- Comments: line `#`, block `-`.
- Queries: upstream queries/highlights.scm (+ injections.scm into built-ins only)
- Notes: big parser: check wasm size; POD is not a block comment

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_perl"`, `name = "Perl syntax"`,
  one `[[contributes.grammars]]` with `language = "perl"`, the extensions,
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
- `test/sample.pl`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.pl.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh perl`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/perl/` and those appended
  rows. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## Implementation notes

- Pinned the release branch's head, `4e56e286bbe60b1da410cf5cb3ebef75e134f30e`
  (deploying master's `80781e3`, version 2.0.4). The module is 4.7 MB, well
  under `MAX_GRAMMAR_BYTES`.
- No `block_comment`: POD (`=head1` … `=cut`) is not a comment token pair. It
  highlights as `comment.documentation`.
- `injections.scm` keeps upstream's heredoc injection only for terminators
  naming a language the app has (case-insensitive, so `<<SQL` is sql, hence
  `parity_needs = ["sql"]`); upstream's `comment`, `pod` and self-injected
  `s///e` patterns are dropped.
- The shebang `@preproc` pattern is dropped: it used `#lua-match?`, and the
  later `(comment) @comment` overrode it anyway.
