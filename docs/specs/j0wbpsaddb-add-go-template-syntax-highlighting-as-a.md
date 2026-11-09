---
task_uid: "01m6z3y0bff42b8zj0wbpsaddb"
task: "T539"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "extensions/grammars/gotmpl/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except one appended THIRD_PARTY.md row (adapted queries are credited in the WebAssembly row, not a separate adapted-code row)"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh gotmpl passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for gotmpl, with test/sample.tmpl parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with gotmpl present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.tmpl.golden colours the {{ }} delimiters, keywords (if, range, with, define, template, end), functions and pipelines, fields and variables, strings, numbers and comments"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Go template syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/gotmpl) (Mac T1637)

# Go template syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/gotmpl/`, following
`extensions/grammars/README.md` and the existing grammars
(`extensions/grammars/twig/` is the closest).

## This language

- Grammar: https://github.com/ngalaiko/tree-sitter-go-template, directory
  `src`. Pin: master (HEAD 06b2fd2ac32a7be71cdcf925eeeb868a68e2acde, 0.19.5,
  2026-10-02).
- Licence: MIT. External scanner: none. Parser size: 0.3 MB. ABI 15.
  Export `gotmpl`.
- File extensions: gotmpl, tmpl. File names: -. Shebangs: -.
- Comments: block `{{/*` `*/}}`.
- Queries: upstream `queries/highlights.scm` (MIT). No upstream injections,
  and the text between actions may be HTML, YAML or anything: a comment-only
  `injections.scm` saying so.
- Notes: Helm is left out. Helm templates are `templates/*.yaml` (plus
  `_helpers.tpl`): `.yaml` is the built-in YAML's, which a manifest may not
  claim, and fe-syntax picks a language by fixed path tails, file names and
  the last extension only (`crates/fe-syntax/src/language.rs`,
  `PATH_SUFFIXES`), with no "a `templates/` directory beside a `Chart.yaml`"
  rule; it would also need the repository's second grammar,
  `dialects/helm` (export `helm`), as its own directory. `.tpl` is shared
  with Smarty and other engines, so it isn't claimed either. A later card
  can add Helm with a path-pattern detection rule.

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
- `extension.toml`: `id = "tenmoku.grammar_gotmpl"`, `name = "Go template syntax"`,
  `license` = the grammar's SPDX id, one `[[contributes.grammars]]` with
  `language = "gotmpl"`, the extensions, file names, `line_comment` /
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
- `test/sample.tmpl`: 40-120 lines of idiomatic code touching the template
  delimiters, keywords, strings with escapes, numbers, comments, filters or
  helpers, and every injection; the host tree must parse with no ERROR or
  MISSING node (an injected layer may have errors: see the notes). Generate
  `test/sample.tmpl.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh gotmpl`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one row for `gotmpl` to `THIRD_PARTY.md`'s "Grammars
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
- Change nothing outside `extensions/grammars/gotmpl/` and that appended
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
