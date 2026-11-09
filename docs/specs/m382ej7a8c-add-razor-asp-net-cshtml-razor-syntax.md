---
task_uid: "01m6z3yafarenw6zm382ej7a8c"
task: "T544"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/razor/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except one appended THIRD_PARTY.md row (adapted queries are credited in the WebAssembly row, not a separate adapted-code row)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh razor passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for razor, with test/sample.cshtml parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with razor present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.cshtml.golden colours Razor directives (@page, @model, @code, @if, @foreach), C# keywords, types, strings, numbers and comments, @* *@ comments, and the injected HTML's tags and attributes"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Razor (ASP.NET .cshtml/.razor) syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/razor) (Mac T1641)

# Razor syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/razor/`, following
`extensions/grammars/README.md` and the existing grammars
(`extensions/grammars/csharp/` holds the C# query this one builds on).

## This language

- Grammar: https://github.com/tris203/tree-sitter-razor, directory `src`.
  Pin: main (HEAD d4664e409caaea12f73c9525484e3cf88b1cf718, 0.1.1,
  2026-07-28).
- Licence: MIT. External scanner: scanner.c. Parser size: 52.5 MB, the
  largest yet (C#'s 29.7 MB built to 5.8 MB, so expect about 10-11 MB:
  under `MAX_GRAMMAR_BYTES` = 16 MiB but check, and say the size and load
  time). ABI 15. Export `razor`.
- File extensions: cshtml, razor. File names: -. Shebangs: -.
- Comments: block `@*` `*@`.
- The grammar is tree-sitter-c-sharp ^0.23.5 extended (`grammar(CSHARP,
  ...)`), so C# is parsed by this grammar itself, not injected: no
  `parity_needs`.
- Queries: upstream `queries/highlights.scm` starts `; inherits: c_sharp`:
  inline `extensions/grammars/csharp/highlights.scm` (tree-sitter-c-sharp
  0.23.5's own query, MIT, the same grammar version Razor extends) and then
  Razor's patterns, adapted to fe-syntax's captures; `csharp/` is
  `earlier_patterns_win = false` (last pattern wins), so keep that and put
  Razor's patterns after C#'s. Upstream
  `queries/injections.scm`: keep `(element)` -> html, combined; drop the
  comment injections (no `comment` language).
- Notes: elements nested in C# blocks have different parents, so each
  block's markup is its own HTML layer. The sample should touch `@page`,
  `@model`, `@using`, `@inject`, `@code { }`, `@if`/`@foreach`, implicit
  `@Model.Name` and explicit `@(a + b)` expressions, a `@* *@` comment and
  a tag helper attribute.

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
- `extension.toml`: `id = "tenmoku.grammar_razor"`, `name = "Razor syntax"`,
  `license` = the grammar's SPDX id, one `[[contributes.grammars]]` with
  `language = "razor"`, the extensions, file names, `line_comment` /
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
- `test/sample.cshtml`: 40-120 lines of idiomatic code touching the template
  delimiters, keywords, strings with escapes, numbers, comments, filters or
  helpers, and every injection; the host tree must parse with no ERROR or
  MISSING node (an injected layer may have errors: see the notes). Generate
  `test/sample.cshtml.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh razor`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one row for `razor` to `THIRD_PARTY.md`'s "Grammars
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
- Change nothing outside `extensions/grammars/razor/` and that appended
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

- Pinned main's head, d4664e409caaea12f73c9525484e3cf88b1cf718 (0.1.1).
  `tree-sitter-razor.wasm` builds to 10,723,552 bytes (10.2 MiB, under
  `MAX_GRAMMAR_BYTES`), and loads within the deadline: the whole
  `every_extension_highlights_its_sample` run for razor took 15-36 s at a
  load average near 100.
- The grammar's HTML is minimal: `<!DOCTYPE html>`, character references
  (`&copy;`) and a `@* *@` comment inside an element's content each leave an
  ERROR in the host tree (the last one breaks the later `@code` block), so
  the sample has none of them and its `@* *@` comment sits at the top level,
  before `@code`.
- First-party queries may not capture `@none`, so an element attribute's
  `=` (the element's own token, which the injected HTML leaves uncaptured)
  is `@punctuation.delimiter` rather than C#'s `@operator`.
