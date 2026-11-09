---
task_uid: "01m6z3thvdgnae57xzeb43vyeh"
task: "T1640"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/blade/ and extensions/grammars/php_only/ each hold the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (blade: a 40-hex commit of the named repository; php_only: tree-sitter-php 0.24.2 in the crates.io form, no extensions, php's queries credited, parity_needs sql) and the grammars' licence files verbatim; nothing else changes except two appended THIRD_PARTY.md rows (adapted queries are credited in the WebAssembly rows, not a separate adapted-code row)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh php_only blade passes on the lane's host: the wasms build and load within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for both, with test/sample.blade.php and php_only's sample parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed (php_only allowed because blade's injections.scm names it) and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with blade and php_only present, blade's extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.blade.php.golden colours HTML tags and attributes, Blade directives and their parameters, {{ }}/{!! !!} delimiters, comments, and the injected PHP's (php_only) variables, strings, numbers and keywords plus JavaScript in Alpine attributes and \u003cscript\u003e"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Blade (Laravel) syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/blade)

# Blade syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/blade/`, following
`extensions/grammars/README.md` and the existing grammars
(`extensions/grammars/astro/` is the closest: a grammar that extends
tree-sitter-html and injects code).

Depends on two cards: the compound-extension card (so `blade.php` can be
claimed and wins over `php` for `view.blade.php`) and the php_only card
(the first-party `php_only` language Blade's code regions are injected
into: the first-party `php` grammar treats text before `<?php` as HTML, so
`{{ $user->name }}` would stay plain under it).

## This language

- Grammar: https://github.com/EmranMR/tree-sitter-blade, directory `src`.
  Pin: main (HEAD b5291d1ba207a8ebb8383b2ecb8a8a6535210a50, after 0.12.3,
  2026-08-31).
- Licence: MIT. External scanner: scanner.c (with `tag.h`). Parser size:
  23 MB (HLSL's 20 MB built to 4.2 MB, so expect about 5 MB: well under
  `MAX_GRAMMAR_BYTES`, but say the size). ABI 15. Export `blade`.
- File extensions: blade.php (the compound form the compound-extension card
  adds). File names: -. Shebangs: -. Sample: `test/sample.blade.php`.
- Comments: block `{{--` `--}}`.
- `parity_needs = ["php_only"]` (which brings sql transitively).
- Queries: both upstream files start `; inherits: html`: inline
  tree-sitter-html 0.23.2's `queries/highlights.scm` and
  `queries/injections.scm` (MIT; `<script>` -> javascript, `<style>` ->
  css) ahead of Blade's own, as `astro/` does, and credit both sources in
  the headers and the THIRD_PARTY.md row. Blade's injections: `php_only`
  and `parameter` -> php_only, Livewire/Alpine attributes -> javascript,
  `<x-*>` `:attr` -> php_only. The Envoy `text` -> bash pattern uses
  `#has-ancestor?`, which fe-syntax lacks: drop it.
- Notes: the sample should touch `{{ }}`, `{!! !!}`, `@if/@foreach/@endif`
  with parameters, `@php ... @endphp`, a component `<x-alert :type="$t">`,
  an Alpine `x-data`, a comment and a `<script>`.

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
- `extension.toml`: `id = "tenmoku.grammar_blade"`, `name = "Blade syntax"`,
  `license` = the grammar's SPDX id, one `[[contributes.grammars]]` with
  `language = "blade"`, the extensions, file names, `line_comment` /
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
- `test/sample.blade.php`: 40-120 lines of idiomatic code touching the template
  delimiters, keywords, strings with escapes, numbers, comments, filters or
  helpers, and every injection; the host tree must parse with no ERROR or
  MISSING node (an injected layer may have errors: see the notes). Generate
  `test/sample.blade.php.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh blade`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one row for `blade` to `THIRD_PARTY.md`'s "Grammars
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
- Change nothing outside `extensions/grammars/blade/` and that appended
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

## Rev 2 (lead, 11-09): this card also adds php_only

T1633 (rev 2) ships the rule that lets an injection-only directory exist and
the install plumbing, but not the directory, since nothing injected it yet.
Add it here, alongside Blade, which injects it:

- `extensions/grammars/php_only/`, a normal grammar directory: source
  tree-sitter-php 0.24.2 (the version `php/` pins), `src = "php_only/src"`,
  `grammar = "php_only"`, in the crates.io form (`registry`, `crate`,
  `version`, `sha256`, `src`, `grammar`) since php_only isn't built in;
  `extensions = []`; `highlights.scm` and `injections.scm` from the crate's
  php queries (php_only shares php's node names; credit them as `php/`
  does), `parity_needs = ["sql"]`; licence; a sample of tagless PHP and its
  golden file; its own appended THIRD_PARTY.md WebAssembly row.
- `scripts/build-grammar-wasm.sh php_only blade` must pass. "Change nothing
  outside `extensions/grammars/blade/`" above now reads "outside
  `extensions/grammars/blade/`, `extensions/grammars/php_only/` and the two
  appended rows".
