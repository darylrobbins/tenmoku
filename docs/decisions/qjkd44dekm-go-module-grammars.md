# go.mod, go.work and go.sum grammars (T1647)

Three first-party WebAssembly grammars, `gomod`, `gowork` and `gosum`, each
chosen by a whole file name and claiming no extension, and the change that lets
such a grammar be checked and offered.

1. **The pins are the spec's.** gomod is grammar-orchard/tree-sitter-gomod-orchard
   at `97801af` (0.5.8, ABI 14, export `gomod_orchard`), the maintained fork of
   camdencheek/tree-sitter-go-mod: it knows `godebug`, which camdencheek's main
   doesn't, so no fallback was needed. gowork is omertuc/tree-sitter-go-work at
   `949a8a4` (0.1.0, ABI 13, export `gowork`). gosum is
   tree-sitter-grammars/tree-sitter-go-sum at `27816eb` (1.0.0, ABI 14, export
   `gosum`). All three build with `scripts/build-grammar-wasm.sh` and load, and
   their samples parse with no ERROR or MISSING node.
2. **gowork's gap.** The grammar knows only `go`, `use` and `replace`, so a
   `toolchain` (Go 1.21) or `godebug` (Go 1.23) line in a go.work parses as an
   ERROR and goes uncoloured; the rest of the file still colours. The gomod
   grammar isn't a substitute (it has no `use`), and patching or regenerating a
   grammar is out of scope. The query's header says so, and the sample has
   neither line.
3. **Colours.** Directive keywords `@keyword`; module paths and tools `@module`;
   versions, go versions and toolchain names `@number`, a pseudo-version or a
   `+incompatible` version as one token; file paths (replace targets, `use`,
   `ignore`) `@string.special`; godebug keys `@property`, values `@constant`,
   `=` and `=>` `@operator`. In go.sum the version is captured whole, so its
   pre-release word, numbers and hex are one `@number`; `go.mod` of the
   `/go.mod` suffix is `@string.special`, `h1` `@attribute`, the hash `@string`,
   `:` and `/` delimiters. The queries have no overlapping patterns, so
   `earlier_patterns_win = true` (tree-sitter's convention) changes nothing.
4. **The gate's rule.** `wasm_parity`'s `check_manifest` now asks for an
   extension *or a file name*, unless another directory injects the language.
   T1633 had refused a directory claiming file names but no extensions, even
   when injected; that's now well formed, and claiming neither is still a
   problem unless injected (fixture `a_language_may_claim_only_file_names`).
5. **The catalog.** `grammar_install`'s `catalog()` leaves out only a language
   claiming neither extensions nor file names, and `with_injected` brings along
   only such a language: a file-name-only language is offered and installed on
   its own, as one with extensions is. Each `GrammarOffer` carries its
   manifest's `file_names`.
6. **The offer by name.** `offer_for` first looks for a released offer claiming
   the file's whole name, ignoring ASCII case, as fe-syntax gives a file to a
   language claiming its name before one claiming its extension; then the
   longest extension, as before. A whole-name match's key, which `ask`, its
   pending list and Never use in place of an extension, is the lowercase file
   name. An installed language, or the file being some other language's
   already, means no offer, as for an extension. No other matching (prefixes,
   path tails) was added: T1485 moves offer matching onto the grammar index,
   which can take that on.
7. **Never may be a file name.** The choice reads `Never for go.mod` when the
   key is one of the offer's file names and `Never for .ext` otherwise, and the
   key goes into the same `never` list of `grammar_prompt.json`. So that list
   may now hold a file name (`go.mod`) as well as extensions (`kt`,
   `blade.php`), neither with a leading dot. Nothing needs to tell them apart:
   a key is only ever looked up as the key it is. A reader of the file (T1485's
   index included) should treat an entry as either.
