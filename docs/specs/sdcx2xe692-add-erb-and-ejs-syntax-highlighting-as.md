---
task_uid: "01m6z3t6wpa5pxmxsdcx2xe692"
task: "T1634"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/erb/ and extensions/grammars/ejs/ each hold the file set every_extension_is_well_formed requires, both grammar-source.toml pinning the same 40-hex commit of tree-sitter-embedded-template with identical source keys, and the grammar's licence verbatim; erb's injections.scm is upstream's injections-erb.scm (html + ruby) and ejs's is injections-ejs.scm (html + javascript); nothing else changes except two appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh erb ejs passes on the lane's host: the shared tree-sitter-embedded-template.wasm builds once and loads, and wasm_parity's ignored every_extension_highlights_its_sample passes for both, test/sample.erb and test/sample.ejs parsing with no ERROR or MISSING node in the host tree"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with erb and ejs present, claiming erb and ejs respectively with block comment \u003c%# %\u003e and no clash with a built-in or each other"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.erb.golden colours HTML tags and attributes, the \u003c% %\u003e delimiters, comment directives and Ruby keywords, strings and numbers; test/sample.ejs.golden the same with JavaScript in place of Ruby"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add ERB and EJS syntax highlighting as first-party WebAssembly grammars sharing tree-sitter-embedded-template (extensions/grammars/erb, extensions/grammars/ejs)

# ERB and EJS syntax as first-party WebAssembly grammars

Filler card: two new directories, `extensions/grammars/erb/` and
`extensions/grammars/ejs/`, following `extensions/grammars/README.md` and
the existing grammars (`extensions/grammars/twig/` is the closest: a
template language whose text is HTML). One upstream grammar serves both, as
its own `tree-sitter.json` does (one parser, per-language injection files):
the two directories differ only in name, extension, sample and
`injections.scm`.

Depends on the shared-export card: both directories build the same module,
`tree-sitter-embedded-template.wasm`, which before that card races in
`scripts/build-grammar-wasm.sh` (every background job shares the `$$` temp
name) and isn't checked to come from the same source.

## This language

- Grammar: https://github.com/tree-sitter/tree-sitter-embedded-template,
  directory `src`. Pin: master (HEAD 3499d85f0a0d937c507a4a65368f2f63772786e1,
  0.25.0, 2025-08-31). Both directories pin the same commit, with identical
  `git`/`rev`/`src`/`grammar`.
- Licence: MIT. External scanner: none. Parser size: 35 KB. ABI 15.
  Export `embedded_template`, so `wasm = "tree-sitter-embedded-template.wasm"`
  in both.
- erb: language `erb`, name "ERB". File extensions: erb (a path's last
  extension decides, so `.html.erb`, `.js.erb` and `.text.erb` are erb too).
  File names: -. Shebangs: -. Comments: block `<%#` `%>`.
  `parity_needs = ["ruby"]`.
- ejs: language `ejs`, name "EJS". File extensions: ejs. File names: -.
  Shebangs: -. Comments: block `<%#` `%>`. No `parity_needs` (JavaScript is
  built in).
- Queries: upstream `queries/highlights.scm` in both (MIT; it colours only
  the delimiters and `comment_directive`). `injections.scm`: erb takes
  upstream `queries/injections-erb.scm` (content -> html combined, code ->
  ruby combined), ejs takes `queries/injections-ejs.scm` (content -> html
  combined, code -> javascript combined). Skip `injections-etlua.scm`.
- Notes: nearly all colour comes from the injections, so the samples must
  exercise HTML (tags, attributes, a `<script>`), output tags `<%= %>`,
  trim forms `<%-`/`-%>`, comment directives and multi-line code. Every
  `<% %>` is its own layer (see "How fe-syntax treats template
  injections"): an `if` and its `end` in separate tags each parse with
  errors in their Ruby/JS layer; their keywords still colour. The golden
  files show that; it is not a defect to fix here.

Do everything in "Requirements" once for each directory: where it says
`@LANG@` read `erb` or `ejs`, `@NAME@` read `ERB` or `EJS`, `@EXT@` read
`erb` or `ejs`. Two THIRD_PARTY.md rows, one per language.

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
- `extension.toml`: `id = "tenmoku.grammar_@LANG@"`, `name = "@NAME@ syntax"`,
  `license` = the grammar's SPDX id, one `[[contributes.grammars]]` with
  `language = "@LANG@"`, the extensions, file names, `line_comment` /
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
- `test/sample.@EXT@`: 40-120 lines of idiomatic code touching the template
  delimiters, keywords, strings with escapes, numbers, comments, filters or
  helpers, and every injection; the host tree must parse with no ERROR or
  MISSING node (an injected layer may have errors: see the notes). Generate
  `test/sample.@EXT@.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh @LANG@`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one row for `@LANG@` to `THIRD_PARTY.md`'s "Grammars
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
- Change nothing outside `extensions/grammars/@LANG@/` and that appended
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

## Implementation notes

- Both directories pin 3499d85f0a0d937c507a4a65368f2f63772786e1, still
  upstream's HEAD on 2026-11-09. Queries are upstream's, unchanged apart from a
  header comment.
- As expected for one-layer-per-directive, ERB's lone `<% else %>` and
  `<% end %>` parse as errors in their Ruby layer and colour as `variable`
  (tree-sitter-ruby recovers them as identifiers); `if`, `unless` and `do`
  colour as keywords. EJS's `} else {` keeps its `else` keyword.
- The built-in JavaScript query doesn't colour escape sequences, so EJS's
  `\n` in a template string stays `string`.
- Respec: `crates/fastedit/tests/language_pack.rs` keyed its fake modules by
  language, so ERB's and EJS's stubs for the shared module overwrote each
  other and `pack_matches_the_installer_layout` failed; the stub is now keyed
  by the module's file name.
