---
task_uid: "01m6z3y52kk86gspzaxqpfgt0a"
task: "T541"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/jinja/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except one appended THIRD_PARTY.md row (adapted queries are credited in the WebAssembly row, not a separate adapted-code row)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh jinja passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for jinja, with test/sample.jinja parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with jinja present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.jinja.golden colours the {% %} and {{ }} delimiters, statement keywords (for, if, block, macro, set), filters and calls, strings, numbers, comments, and the injected HTML's tags and attributes"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Jinja syntax highlighting (Jinja2, Nunjucks) as a first-party WebAssembly grammar (extensions/grammars/jinja) (Mac T1636)

# Jinja syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/jinja/`, following
`extensions/grammars/README.md` and the existing grammars
(`extensions/grammars/twig/` is the closest: Twig is Jinja's PHP cousin).

## This language

- Grammar: https://github.com/cathaysia/tree-sitter-jinja, directory
  `tree-sitter-jinja/src`. Pin: f46e979278123e3b6949381896f3917f378af068
  (release 0.13.1, 2026-11-02, master's head). The repository was archived
  the same day: it is read-only but fetches as usual; pin that commit.
- Licence: Apache-2.0 (the repository's root `LICENSE`, copied verbatim as
  `LICENSE`; `extension.toml` `license = "Apache-2.0"`). External scanner:
  scanner.c (includes only `tree_sitter/` headers). Parser size: 0.6 MB.
  ABI 15. Export `jinja`.
- File extensions: jinja, jinja2, j2, njk. File names: -. Shebangs: -.
- Comments: block `{# #}`.
- Queries: upstream `tree-sitter-jinja/queries/highlights.scm` (Apache-2.0,
  nvim style: `@spell` and the like renamed or dropped). Upstream
  `injections.scm` only injects line statements into `jinja_inline`, the
  repository's second grammar, which doesn't exist here: drop it. Add
  `(content)` -> html, combined, as Twig does (a `.j2` config file's text
  then stays plain, as HTML text is).
- Notes: picked as the best-maintained full Jinja grammar (nvim-treesitter's
  choice): https://github.com/dbt-labs/tree-sitter-jinja2 (Apache-2.0)
  lexes a whole `{% ... %}` as one regex token, so no keywords or
  expressions; https://github.com/geigerzaehler/tree-sitter-jinja2 has no
  licence; https://github.com/interdependence/tree-sitter-htmldjango (MIT)
  is Django's dialect. Nunjucks (`.njk`) is Jinja-compatible for what a
  sample uses. Django templates aren't claimed: they are `.html` files,
  which the built-in HTML owns, and fe-syntax decides by path before any
  content check (`crates/fe-syntax/src/detect.rs`), so detecting them would
  need a content-sniffing rule for `.html`: out of scope. Line statements
  (`# for x in y`) parse as `inline` and stay plain. `content` inside a
  block is that block's child, so each block body is its own HTML layer.

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`. Add
  `parity_needs` exactly as "This language" says.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit; a `LICENSE.md` upstream is copied to `LICENSE` (the well-formed
  check admits only `LICENSE`, `LICENSE-*` and `NOTICE`). Queries from
  nvim-treesitter are Apache-2.0: add `LICENSE-APACHE` and say so as `kotlin/`
  does. Don't copy Helix or Zed queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_jinja"`, `name = "Jinja syntax"`,
  `license` = the grammar's SPDX id, one `[[contributes.grammars]]` with
  `language = "jinja"`, the extensions, file names, `line_comment` /
  `block_comment` and `shebangs` above, and `earlier_patterns_win` matching
  the query's convention (tree-sitter style: true; nvim style: false). No
  `packs` or other new keys.
- `highlights.scm`: self-contained (inline anything a `; inherits:` line
  pulls in, adapted to this grammar's node names); captures in fe-syntax's
  vocabulary (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES). Drop
  patterns using predicates or directives fe-syntax doesn't support
  (`#offset!`, `#gsub!`, `#has-ancestor?`, `#set! priority`, `#lua-match?`
  unless rewritten as `#match?`) rather than leaving them inert. Each query
  file's header says where it came from, at which commit, under what
  licence, and what changed (`extensions/grammars/astro/` is the model).
- `injections.scm`: only injections into languages that exist (built-ins,
  or first-party directories listed in `parity_needs`); a comment-only file
  if none. No locals, folds, indents or outline queries.
- `test/sample.jinja`: 40-120 lines of idiomatic code touching the template
  delimiters, keywords, strings with escapes, numbers, comments, filters or
  helpers, and every injection; the host tree must parse with no ERROR or
  MISSING node (an injected layer may have errors: see the notes). Generate
  `test/sample.jinja.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh jinja`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one row for `jinja` to `THIRD_PARTY.md`'s "Grammars
  shipped as WebAssembly" table, giving the source as `grammar-source.toml`
  does and the licence, and naming in that licence cell every adapted query
  source with its commit and licence (as the astro, svelte and hlsl rows do).
  That row is where adapted queries are credited: add nothing to "Code copied
  or adapted into this repository" (that table and its checks,
  `crates/fastedit/tests/third_party.rs` and `tests/notice.rs`, cover
  `crates/` and `kiln/` only and want NOTICE entries; settled on T1463
  Astro), and don't touch NOTICE. Append only; don't reorder or edit other
  rows. `THIRD_PARTY.md` is in `regatta.toml`'s `shared_append_only`, which
  is why it isn't in this card's scope.
- Change nothing outside `extensions/grammars/jinja/` and that appended
  row. If the module won't load (ABI, the table validator, a libc import the
  wasm stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.

## How fe-syntax treats template injections (read before writing queries)

- `injection.combined` joins a content node only with its siblings of the
  same kind under the same parent (`crates/fe-syntax/src/injection.rs`,
  module docs), not every match in the file. Text nodes that are all
  children of the root form one HTML layer; code pieces that each sit in
  their own directive node are each their own layer, so `<% if x %>` alone
  is a Ruby layer with an error. That is expected: keep `injection.combined`
  where upstream has it, don't work around it, and don't change fe-syntax.
- Layers nest up to `MAX_DEPTH` = 3 (template -> HTML -> `<script>` JS is
  fine).
- `html`, `css`, `javascript`, `typescript`, `json`, `yaml` and `bash` are
  built in; `ruby`, `php`, `csharp`, `elixir` and `sql` are first-party
  directories (name them in `parity_needs` when the sample injects them).
