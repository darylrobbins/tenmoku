---
task_uid: "01m6z3y7z8kayaxvg8hstzdpsp"
task: "T543"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/heex/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except one appended THIRD_PARTY.md row (adapted queries are credited in the WebAssembly row, not a separate adapted-code row)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh heex passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for heex, with test/sample.heex parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with heex present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.heex.golden colours tags, components and slots, attributes and special attributes (:if, :for), directives, comments, and the injected Elixir's keywords, strings, atoms and numbers, plus CSS and JavaScript in \u003cstyle\u003e/\u003cscript\u003e"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add HEEx (Phoenix) syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/heex) (Mac T1639)

# HEEx syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/heex/`, following
`extensions/grammars/README.md` and the existing grammars
(`extensions/grammars/astro/` is the closest: a grammar that parses HTML
itself and injects a programming language).

## This language

- Grammar: https://github.com/phoenixframework/tree-sitter-heex, directory
  `src`. Pin: main (HEAD 5842537f734d7c12685bf27d6005313e3e5a47a0,
  2026-03-23).
- Licence: MIT (upstream `LICENSE.md`, copied verbatim to `LICENSE`).
  External scanner: none. Parser size: 0.13 MB. ABI 15. Export `heex`.
- File extensions: heex (`.html.heex` too, by its last extension). File
  names: -. Shebangs: -.
- Comments: block `<%!--` `--%>`.
- `parity_needs = ["elixir"]` (which brings sql transitively).
- Queries: upstream `queries/highlights.scm` (drop `(ERROR) @error`) and
  `queries/injections.scm`: elixir for directives and `{...}` expressions
  (keep `injection.include-children` and `injection.combined`); the
  `<style>`/`<script>` patterns use `#offset!`, which fe-syntax lacks, so
  rewrite them to inject the element's `(text)` child as css / javascript;
  keep `style=` -> css and `on*=` -> javascript.
- Notes: the grammar parses HTML itself, so there is no html injection.
  `partial_expression_value` pieces sit in different `directive` nodes, so
  `<%= if x do %>` and `<% end %>` are separate Elixir layers (see "How
  fe-syntax treats template injections"). Elixir's own `~H` sigil stays
  uninjected: `extensions/grammars/elixir/injections.scm` dropped heex, and
  changing it is out of scope (a follow-up card can add it).

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
- `extension.toml`: `id = "tenmoku.grammar_heex"`, `name = "HEEx syntax"`,
  `license` = the grammar's SPDX id, one `[[contributes.grammars]]` with
  `language = "heex"`, the extensions, file names, `line_comment` /
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
- `test/sample.heex`: 40-120 lines of idiomatic code touching the template
  delimiters, keywords, strings with escapes, numbers, comments, filters or
  helpers, and every injection; the host tree must parse with no ERROR or
  MISSING node (an injected layer may have errors: see the notes). Generate
  `test/sample.heex.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh heex`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one row for `heex` to `THIRD_PARTY.md`'s "Grammars
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
- Change nothing outside `extensions/grammars/heex/` and that appended
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

- Pinned main's head `5842537f734d7c12685bf27d6005313e3e5a47a0` (package
  version 0.9.0, the manifest's `version`); `LICENSE` is upstream's
  `LICENSE.md` verbatim. Upstream's queries are tree-sitter style, so
  `earlier_patterns_win = true`.
- The grammar has no raw text: inside `<style>`/`<script>` a `{`, `<`, `>` or
  `&` is parsed as a host expression, tag or entity, so only the `(text)`
  between them reaches the css/javascript layer (upstream's `#offset!`
  version takes the whole element). The sample's `<style>` and `<script>`
  therefore hold brace-free CSS (`@import`, `@layer`) and JavaScript.
- As the spec's notes say, `<% else %>` and `<% end %>` are Elixir layers of
  their own, so `else`/`end` there colour as `variable` (Elixir's error
  recovery); `if`/`do` in `<%= if ... do %>` keep the host's `keyword`.
- A `style="..."` value is parsed as a whole stylesheet; its last
  declaration needs its `;` to read as a property (the sample has one).
