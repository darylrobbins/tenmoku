---
task_uid: "01m6z4ygqkfdsyqrqjkd44dekm"
task: "T1647"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/gomod/, gowork/ and gosum/ each hold the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository) and the grammar's licence verbatim; each extension.toml claims file names only (gomod go.mod; gowork go.work; gosum go.sum and go.work.sum) and no extension; nothing outside the card's scope changes except three appended THIRD_PARTY.md rows"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh gomod gowork gosum passes on the lane's host: each wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for all three, each test/sample.* parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with the three present, with no clash with a built-in or another directory; a directory claiming file names but no extensions is well formed while one claiming neither (and injected by none) is still a problem, each shown by a wasm_parity fixture case"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench grammar_prompt's offer_for offers a released, uninstalled catalog language for a file whose whole name, ignoring ASCII case, is one of its file_names (go.mod, GO.MOD; not x.mod); its Never choice reads 'Never for go.mod' and remembers the lowercase file name, so a later go.mod isn't offered; tests in grammar_prompt_tests.rs show each, and extension offers behave as before"
    required: true
    manual: false
  - id: "C5"
    text: "fastedit's grammar_install catalog() gives each first-party offer its manifest's file_names, the gomod offer having file_names [\"go.mod\"] and no extensions, and leaves out only languages claiming neither extensions nor file names; a test in grammar_install_tests.rs shows it"
    required: true
    manual: false
  - id: "C6"
    text: "test/sample.mod.golden colours the directive keywords (module, go, toolchain, godebug, require, replace, exclude, retract, tool, ignore), module paths, versions including a pseudo-version and +incompatible, the =\u003e operator, replace file paths, godebug keys and values, and comments including // indirect; test/sample.work.golden colours go, use, replace, paths, versions, =\u003e and comments; test/sample.sum.golden colours module paths, versions including pseudo-versions, the /go.mod suffix, h1 and the hashes"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-syntax -p fe-workbench -p fastedit passes"
    required: true
    manual: false
---

# Add go.mod, go.work and go.sum syntax highlighting as first-party WebAssembly grammars chosen by file name, and offer a grammar's install for a file it claims by name (extensions/grammars/gomod, gowork, gosum)

# go.mod, go.work and go.sum syntax as first-party WebAssembly grammars

User report: go.mod opens as Plain Text. Go's module files have their own
small grammars, and each is chosen by its whole file name, not an extension
(`.mod` and `.sum` are other formats' too). Three new directories,
`extensions/grammars/gomod/`, `extensions/grammars/gowork/` and
`extensions/grammars/gosum/`, following `extensions/grammars/README.md` and
the existing grammars (`extensions/grammars/properties/` and
`extensions/grammars/graphql/` are the plainest; `graphql/`'s parser is ABI
13 like gowork's and loads). Plus the smallest change that lets a grammar
claiming only file names be checked and offered (below): today no
first-party grammar does.

File icons for these files already exist (kiln-ui's icon tables): no icon
work.

## What's missing for file-name-only grammars today

Verified at main 069b28b57:

- Detection works once a grammar is installed: `fe_syntax::language_name_for_path`
  (`crates/fe-syntax/src/language.rs`) and `runtime::name_for_file`
  (`crates/fe-syntax/src/runtime.rs`) match `file_names` whole, ignoring
  ASCII case, and `crates/fastedit/src/config.rs` passes the manifest's
  `file_names` into `GrammarSpec`. `go.work.sum` matches as a whole name.
  No built-in or directory claims the extensions `mod`, `sum` or `work`.
- The gate refuses the directory: `crates/fe-syntax/tests/wasm_parity.rs`
  `check_manifest` says "its grammar has no extensions, which the install
  offer is for". T1633 (a dependency) replaces that rule with "a directory
  may claim no extension *or file name* only if another directory injects
  it", which admits a file-name-only directory. If T1633 as landed still
  rejects one, change the rule so it does (claiming neither extensions nor
  file names stays a problem unless injected), with a fixture case.
- The install offer can't see file names: `crates/fe-workbench/src/grammar_prompt.rs`
  `offer_for` returns early unless `path.extension()` is one of the offer's
  `extensions`; `GrammarOffer` has no `file_names`, and
  `crates/fastedit/src/grammar_install.rs`' `OfferGrammar`/`Offered`/`catalog()`
  don't read them. So opening go.mod would never offer the install, and
  neither does a Gemfile today (ruby claims it, but only `.rb` is offered).

## Do: the offer by file name

- `GrammarOffer` gains `pub file_names: Vec<String>` (whole names, as the
  manifest gives them). `grammar_install.rs` reads `file_names` from the
  manifest (`OfferGrammar`, `Offered`) and fills it in `catalog()`. If T1633
  left extension-less languages out of the catalog, leave out only those
  claiming neither extensions nor file names (gomod and friends are
  offered; php_only stays out).
- `offer_for` (after T1632's compound-extension matching, which it keeps):
  a file whose whole name, ignoring ASCII case, is one of an offer's
  `file_names` gets that offer, whether or not the path has an extension,
  and a whole-name match beats an extension match, as in
  `language_name_for_path`. Its key (what `ask`, `pending` and Never use in
  place of the extension) is the lowercase file name.
- Never: the choice reads `Never for go.mod` (the file name, no leading
  dot) for a file-name match and stays `Never for .ext` otherwise; it
  writes the lowercase file name to the same `never` list in
  `grammar_prompt.json`, read and written as extensions are now. The label
  for a queued key is chosen the same way (whether the key is one of the
  offer's file names). Keep the toast's other text and the one-at-a-time
  rules as they are; the toast strings are plain `format!` as the file's
  others are.
- Update every `GrammarOffer` literal (`grammar_prompt_tests.rs`,
  `crates/fe-workbench/tests/grammar_prompt.rs`, `grammar_install.rs`).
- Tests: in `grammar_prompt_tests.rs`, a released, uninstalled offer
  claiming the file name `go.mod` and no extensions is offered for a file
  `go.mod` (and `GO.MOD`), not for `x.mod`, and its Never choice reads
  `Never for go.mod` and is remembered by name (a later `go.mod` isn't
  offered, a fresh read of the never file has `go.mod`). In
  `grammar_install_tests.rs`, the catalog's gomod offer carries
  `file_names == ["go.mod"]` and no extensions.
- `extensions/grammars/README.md`: one sentence under extension.toml: a
  grammar may claim only `file_names`; it's offered when a file of that
  name opens.
- T1485 (later) moves offer matching onto the grammar index and already
  plans file names; nothing here should make that harder. Say in the
  decision file that a Never entry may now be a file name.

## This language: gomod

- Grammar: https://github.com/grammar-orchard/tree-sitter-gomod-orchard,
  directory `src`. Pin: main, HEAD 97801af57aac43d45fb68dab0ac47f0d0ab18bd1
  (0.5.8, 2026-10-29). This is the maintained fork of
  camdencheek/tree-sitter-go-mod (its `tree-sitter.json` links it as
  upstream; same MIT licence and copyright). Prefer it over camdencheek's
  main (2e88687, 2025-10-23), which has no `godebug` directive (Go 1.23), so
  a go.mod using one would parse with an ERROR. The GitHub repository is the
  Codeberg project's mirror with generated files, one commit per release;
  the git form works as for other grammars. If it won't build or load, fall
  back to camdencheek at 2e886870578eeba1927a2dc4bd2e2b3f598c5f9a (ABI 15,
  export `gomod`), drop `godebug` from the sample, and say so in the
  decision file.
- Licence: MIT. External scanner: none. Parser: 138 KB. ABI 14. Export
  `gomod_orchard`, so `wasm = "tree-sitter-gomod-orchard.wasm"`.
- Language `gomod`, name "Go module". File extensions: none. File names:
  `go.mod`. Shebangs: -. Comments: line `//`.
- Directives the grammar knows: module, go, toolchain, godebug, require,
  replace, exclude, retract, tool, ignore.
- Queries: upstream `queries/highlights.scm` (MIT), adapted: keep its
  keyword list (it has every directive above but `tool`: add `tool`), `=>`
  as `@operator`, comments; then add `(module_path)` and `(tool)` as
  `@module`, `(version)`, `(go_version)` and `(toolchain_name)` as `@number`
  (a pseudo-version such as `v0.0.0-20240101120000-abcdef123456` is one
  `version` token, so it colours whole), `(file_path)` (replace targets,
  ignore paths) as `@string.special`, the godebug spec's key as
  `@property`, its value as `@constant` and its `=` as `@operator`,
  `escape_sequence` as `@escape`, brackets and `,` as punctuation. Skip
  `structure.scm`. `injections.scm` comment-only.
- Sample `test/sample.mod`: a realistic go.mod with every directive
  (single and parenthesised forms), a quoted module path, `// indirect` and
  other comments, a pseudo-version, a `+incompatible` version, a
  retract range `[v1.0.0, v1.0.5]`, and both replace forms (`=> ../local`
  and `=> example.com/fork v1.2.3`).

## This language: gowork

- Grammar: https://github.com/omertuc/tree-sitter-go-work, directory `src`.
  Pin: main, HEAD 949a8a470559543857a62102c84700d291fc984c (2022-10-04,
  0.1.0; unmaintained since). No newer fork adds anything.
- Licence: MIT (the LICENSE file; `package.json` still says ISC, ignore
  it). External scanner: none. Parser: 59 KB. ABI 13 (accepted: min 13;
  graphql's is 13 too). Export `gowork`, so `wasm = "tree-sitter-gowork.wasm"`.
- Language `gowork`, name "Go workspace". File extensions: none. File
  names: `go.work`. Shebangs: -. Comments: line `//`.
- Known gap: the grammar knows only `go`, `use` and `replace`. `toolchain`
  (Go 1.21) and `godebug` (Go 1.23) lines parse as ERROR and lose their
  colour; the rest of the file still colours. The gomod grammar isn't a
  substitute (it has no `use` directive). Note the gap in the query header
  and the decision file; don't patch or regenerate the grammar.
- Queries: upstream `queries/highlights.scm` (MIT), adapted to the same
  choices as gomod: go/use/replace `@keyword`, `=>` `@operator`, comments,
  `(module_path)` `@module`, `(version)`/`(go_version)` `@number`,
  `(file_path)` (use and replace paths) `@string.special`, `escape_sequence`
  `@escape`, brackets as punctuation. `injections.scm` comment-only.
- Sample `test/sample.work`: `go`, single and parenthesised `use`, both
  replace forms, comments; no `toolchain` or `godebug` (they'd be ERROR).

## This language: gosum

- Grammar: https://github.com/tree-sitter-grammars/tree-sitter-go-sum
  (amaanq/tree-sitter-go-sum redirects there), directory `src`. Pin:
  master, HEAD 27816eb6b7315746ae9fcf711e4e1396dc1cf237 (1.0.0,
  2025-07-10).
- Licence: MIT. External scanner: none. Parser: 33 KB. ABI 14. Export
  `gosum`, so `wasm = "tree-sitter-gosum.wasm"`.
- Language `gosum`, name "Go checksum". File extensions: none. File
  names: `go.sum`, `go.work.sum` (same format). Shebangs: -. Comments:
  none (no `line_comment`).
- Queries: upstream `queries/highlights.scm` (MIT), adapted: `@text.uri`
  and `@symbol` aren't fe-syntax names and `(module_path) @string @text.uri`
  double-captures. Use `(module_path)` `@module`; the version
  (`module_version`, the pre-release and pseudo-version numbers and hex,
  `alpha`/`beta`/`dev`/`pre`/`rc`, `+incompatible`) `@number` as one colour
  (capture `(version)` whole or its parts, whichever the golden shows
  cleanly); the `/go.mod` suffix `@string.special`; `(hash_version)` (`h1`)
  `@attribute`; `(hash)` `@string`; `:` and `/` `@punctuation.delimiter`.
  `injections.scm` comment-only.
- Sample `test/sample.sum`: a real-looking go.sum: both line kinds
  (`h1:` hash and `/go.mod h1:`), a pseudo-version, a `+incompatible`
  version, an `-rc.1` pre-release. (Every line must parse: the grammar
  needs a number after a pre-release word, so no bare `-alpha`.)

## Requirements (each directory)

Where it says `@LANG@` read `gomod`, `gowork` or `gosum`; `@NAME@` the
name above.

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`). Pin the commit above unless a newer one builds and
  passes; take `grammar` from the `tree_sitter_<x>` export in `parser.c`.
  No `parity_needs`.
- `LICENSE` copied verbatim from the grammar at that commit. Don't copy
  Helix or Zed queries (MPL-2.0 / mixed); nvim-treesitter's queries for
  these three add nothing the requirements above need, so don't take them
  (that would also need `LICENSE-APACHE`).
- `extension.toml`: `id = "tenmoku.grammar_@LANG@"`, `name = "@NAME@ syntax"`,
  `version` = the grammar's, `license = "MIT"`, one
  `[[contributes.grammars]]` with `language = "@LANG@"`, `file_names` as
  above and no `extensions` key (or an empty one), `line_comment` as above,
  and `earlier_patterns_win` matching the query's convention (tree-sitter
  style: true; nvim style: false). No `packs` or other new keys.
- `highlights.scm`: captures in fe-syntax's vocabulary
  (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES). Each query file's
  header says where it came from, at which commit, under what licence, and
  what changed (`extensions/grammars/astro/` is the model).
- `test/sample.<ext>` as above, parsing with no ERROR or MISSING node.
  Generate `test/sample.<ext>.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh @LANG@`, then read it:
  every token class named above is coloured and nothing is obviously wrong.
- Attribution: append one row per language to `THIRD_PARTY.md`'s "Grammars
  shipped as WebAssembly" table, giving the source as `grammar-source.toml`
  does and the licence, and naming in that licence cell the adapted query
  source with its commit and licence (as the astro, svelte and hlsl rows
  do). That row is where adapted queries are credited: add nothing to
  "Code copied or adapted into this repository" (that table and its checks,
  `crates/fastedit/tests/third_party.rs` and `tests/notice.rs`, cover
  `crates/` and `kiln/` only and want NOTICE entries; settled on T1463
  Astro), and don't touch NOTICE. Append only; don't reorder or edit other
  rows. `THIRD_PARTY.md` is in `regatta.toml`'s `shared_append_only`, which
  is why it isn't in this card's scope.
- If a module won't load (ABI, the table validator, a libc import the wasm
  stdlib lacks, `MAX_GRAMMAR_BYTES`, the load deadline), block the card with
  the exact error: fixing fe-syntax, the vendored tree-sitter or the build
  script is out of scope. (gomod has the camdencheek fallback above.)

## Out of scope

gopls for go.mod/go.work (fe-lsp), icons, regenerating or patching any
grammar, shebang or content detection.

Decision file `docs/decisions/<lane>-go-module-grammars.md`.

Platform: **Linux-OK**: the grammar script supports Linux x86_64/arm64, and
the prompt tests fake the offers.
