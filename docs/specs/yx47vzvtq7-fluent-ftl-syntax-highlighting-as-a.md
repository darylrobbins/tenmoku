---
task_uid: "01m7knjb6tmee1t0yx47vzvtq7"
task: "T410"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/fluent/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form to https://github.com/mozsearch/tree-sitter-fluent at the full commit e4ff53883eb6eddc832a73452fdc3d9c46fe698d (Mozilla searchfox's Fluent Syntax 1.0 rewrite of projectfluent/tree-sitter-fluent, whose 2018 grammar cannot parse placeables and whose parser.c is ABI 8; Apache-2.0, LICENSE verbatim), the WebAssembly row crediting it as a fork of projectfluent/tree-sitter-fluent, the Apache-2.0 LICENSE verbatim, extension.toml with extension ftl and line_comment #; one appended THIRD_PARTY.md WebAssembly row and a NOT_RELEASED row in grammar_pack_recorded.rs; nothing else changes"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh fluent passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for fluent, with test/sample.ftl (taken from our own catalogues: messages, terms, attributes, placeables, variables, functions with named arguments, select expressions with a default variant, all three comment levels) parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "test/sample.ftl.golden colours message and term identifiers, attributes, variables, references, functions, variant keys, strings, numbers and comments; wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-syntax -p fastedit passes, including grammar_pack_recorded"
    required: true
    manual: false
---

# Fluent (.ftl) syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/fluent), so our own l10n catalogues display properly (Mac T1920)

The user (2026-11-17): "No syntax highlighting for ftl files? We can't even fully display our own source" — the repo has 81 Fluent (.ftl) catalogues (crates/*/l10n/en-US/*.ftl) and no grammar maps them. Priority.

A first-party WebAssembly grammar extension, extensions/grammars/fluent/, exactly as T1629 (Nim) and T1900 (Prisma) did it: the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form to https://github.com/projectfluent/tree-sitter-fluent (Apache-2.0, the Project Fluent org's own grammar) at a full 40-hex commit (9f0d60a542f210ce9c03ae8872f93e73b5a31740 or newer), its LICENSE verbatim, extension.toml (extension ftl, line_comment "#", no shebangs, packs ["docs"] unless a better existing pack fits; check registry.toml), one appended THIRD_PARTY.md WebAssembly row, and a NOT_RELEASED row in crates/fastedit/tests/grammar_pack_recorded.rs.

Upstream ships no queries/highlights.scm and its last commit is 2024-08: write highlights.scm here (credit nothing copied; if you adapt another editor's Fluent queries, credit them in the WebAssembly row). If the committed src/parser.c's ABI is outside what our tree-sitter loads, regenerate with the pinned tree-sitter CLI as the build script allows, and say so; if that's not possible without a fork, block for a fork decision (the user decides forks, as for Nim).

Highlights should cover: message and term identifiers (-term), attributes (.attr), = , placeables { }, variables ($var), term and message references, function calls (NUMBER(...), DATETIME(...)) and their named arguments, select expressions (->, variant keys [one] and the default *[other]), string and number literals, comments (#, ##, ###), and junk shown as an error.

The sample (test/sample.ftl) should be a real slice of our own catalogues: a few messages from crates/fe-workbench/l10n/en-US/*.ftl with plurals/select expressions, terms, attributes, functions and all three comment levels, parsing with no ERROR or MISSING node; the golden colours each class above.

## Implementation notes

- Source: mozsearch/tree-sitter-fluent at `e4ff53883eb6eddc832a73452fdc3d9c46fe698d` (rev 2's C1). Its parser.c is ABI 15 and compiles as committed with its C scanner.c, so nothing was regenerated. The module is about 21 KB.
- Pack: `docs` (no better existing pack fits; `config` is for configuration formats).
- highlights.scm is written here; nothing is copied. Message ids are `@property` (as a .properties key), term ids `@constant`, attributes `@attribute`, variables `@variable.parameter` with `$` as `@punctuation.special`, functions `@function.builtin`, named arguments `@variable.parameter`, identifier variant keys `@string.special.key`, placeable braces and the default `*` `@punctuation.special`, `=` and `->` `@operator`, pattern text and string literals `@string`, `#` comments `@comment`, and `##`/`###` comments `@comment.documentation`.
- Junk isn't coloured as an error: fe-syntax's highlight vocabulary has no error name (other first-party grammars drop `(ERROR) @error` for the same reason), and fe-syntax is outside this card's scope. The highlights.scm header says so.
- The sample: our catalogues have no terms and no function calls, so test/sample.ftl is real slices (fastedit app.ftl's `###` header, fe-workbench diff/inbox/git/extensions, fe-editor's `.description` attribute) plus a final group, marked as written for the sample, with a term and its attribute, term and attribute references, term call arguments, and NUMBER/DATETIME with named arguments.
