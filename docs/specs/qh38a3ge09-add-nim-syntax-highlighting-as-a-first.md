---
task_uid: "01m798j5bd5a9dvfqh38a3ge09"
task: "T366"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/nim/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form to d7408ccad04d1ae23d387ba6d1413391d76e95ab of https://github.com/tenmoku-editor/tree-sitter-nim (the case/of patch on metacraft-labs 67cd5540, recorded in grammar-source.toml's comment and highlights.scm's header) and the MIT licence file verbatim; nothing else changes except one appended THIRD_PARTY.md row naming the fork, the base commit and the patch, and the one sorted (\"nim\", \"added on argon; recorded by the Mac's next grammar release\") row in NOT_RELEASED of crates/fastedit/tests/grammar_pack_recorded.rs that the pack-record test needs for any new grammar directory"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh nim passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for nim, with test/sample.nim (holding both the same-column NEP-1 case/of form and the indented form) parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with nim present, its extension.toml giving the spec's file extensions, comment tokens and shebangs with no clash with a built-in; extension.toml's [[contributes.grammars]] lists packs = [\"systems\"], a pack extensions/grammars/registry.toml defines (every first-party grammar's extension.toml carries a packs line; the first pack is its main pack)"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.nim.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any); function captures are on the callee only (a field access such as item.name or s.w is a property, not a method, and the receiver of name.toUpperAscii() is not a function), user-defined command calls and unparenthesized block calls are highlighted as functions by patterns that match the pinned grammar's real hierarchy, and the sample covers a non-builtin command call and an unparenthesized block call. Known limit: a call with explicit generic arguments, such as initTable[string, int](), is still highlighted as a variable because the pinned grammar puts an index_suffix between the callee and the call suffix; that is follow-up T371"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit -p fe-registry passes (fe-registry's every_first_party_pack_is_defined included)"
    required: true
    manual: false
---

# Add Nim syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/nim) (Mac T1629)

# Nim syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/nim/`, following
`extensions/grammars/README.md` and the existing directories
(`extensions/grammars/hlsl/` and `objc/` are the latest; `ruby/` is the
plainest).

## This language

- Grammar: https://github.com/tenmoku-editor/tree-sitter-nim (our fork of metacraft-labs/tree-sitter-nim), directory `src`. Pin: d7408ccad04d1ae23d387ba6d1413391d76e95ab (branch tenmoku/case-of-same-column = upstream 67cd55401207fa04c0a108fe8535dcddb297215e plus the case/of patch below)
- Licence: MIT (`LICENSE`). External scanner: scanner.c (stdlib.h/string.h only). Parser size: 42 MB.
- File extensions: nim, nims, nimble. File names: -. Shebangs: nim.
- Comments: line `#`, block `#[ ]#` (nests; `##` and `##[ ]##` are doc comments).
- Queries: upstream `queries/highlights.scm` (MIT, same repository): adapt it. No upstream injections.
- Version: `0.1.0` (upstream `tree-sitter.json`).
- Notes: big parser: check wasm size (landed Perl's 29 MB parser.c gave a
  4.7 MB module and F#'s 53 MB landed, so expect roughly 7 MB). Upstream is
  young (first commit 2025-12, 32 commits), chosen because it is MIT.

### Licence: MIT only, no MPL

The better-known grammar, alaviss/tree-sitter-nim (the one nvim-treesitter,
Helix and Zed use), is MPL-2.0, and so are its queries and nvim-treesitter's
`runtime/queries/nim/*` (SPDX MPL-2.0 alongside Apache-2.0, derived from
alaviss's). `deny.toml` allows MPL-2.0 only as an unmodified dependency after
review and forbids copying or modifying MPL code into our files. So: don't pin,
fetch, read for copying, or adapt anything from alaviss/tree-sitter-nim,
nvim-treesitter's `nim` queries, or Helix/Zed nim queries. Every query line is
either adapted from metacraft-labs' MIT `queries/highlights.scm` or written
here against its `src/node-types.json`.

If this grammar can't parse ordinary Nim (procs, objects, generics, templates,
macros, `case`/`of`, `for`, string literal kinds, pragmas `{. .}`) without
ERROR nodes, block the card with the failing snippet and the parse tree; don't
switch to the MPL grammar (that is a licence decision for a person).

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`). Pin the newest commit that builds and passes; take
  `grammar` from the `tree_sitter_<x>` export in `parser.c`, which also fixes
  `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` copied verbatim from the grammar at that commit.
- `extension.toml`: `id = "tenmoku.grammar_nim"`, `name = "Nim syntax"`,
  `license = "MIT"`, one `[[contributes.grammars]]` with `language = "nim"`,
  the extensions, `line_comment = "#"`, `block_comment = ["#[", "]#"]`,
  `shebangs = ["nim"]`, and `earlier_patterns_win` matching the query's
  convention (tree-sitter style: true; nvim style: false). No `packs` or
  other new keys: pack membership comes from a later card (T1487).
- `highlights.scm`: self-contained; a header comment says where it came from,
  at which commit, under what licence. Captures in fe-syntax's vocabulary
  (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES). Drop patterns using
  predicates fe-syntax doesn't support rather than leaving them inert.
- `injections.scm`: only injections into languages that exist (built-ins, or
  first-party ones listed in `parity_needs`); a comment-only file if none.
  No locals, folds, indents, tags or outline queries (no first-party
  extension ships them and the manifest has no keys for them).
- `test/sample.nim`: 40-120 lines of idiomatic code touching keywords,
  strings with escapes (and a raw and a triple-quoted string), numbers,
  comments (line, block, doc), procs, types and pragmas; it must parse with no
  ERROR or MISSING node. Generate `test/sample.nim.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh nim`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Attribution: append one `nim` row to `THIRD_PARTY.md`'s "Grammars shipped
  as WebAssembly" table, after the last row, in the `objc`/`hlsl` rows'
  format: the repository and full commit, and a licence cell naming the
  grammar's licence and the source of `highlights.scm` (upstream's
  `queries/highlights.scm` at that commit, MIT, adapted as its header says).
  That row plus the query's header comment is the whole attribution, the
  convention settled on T1463 (Astro): do NOT add rows to "Code copied or
  adapted into this repository" (that table covers `crates/` and `kiln/`
  only; an `extensions/` row fails `third_party::copied_files_and_rows_agree`)
  and do NOT touch `NOTICE` (`notice::sources_are_in_notice`). Reviewers: a
  missing copied-code row or NOTICE entry is this convention, not an omission.
  Append only; don't reorder or edit other rows. `THIRD_PARTY.md` is in
  `regatta.toml`'s `shared_append_only`, which is why it isn't in the scope.
- Change nothing outside `extensions/grammars/nim/` and that appended row. If
  the module won't load (ABI, the table validator, a libc import the wasm
  stdlib lacks, over `MAX_GRAMMAR_BYTES` = 16 MiB, the load deadline), or a
  built-in already claims an extension, block the card with the exact error
  (for size, the module's byte count): fixing fe-syntax, the vendored
  tree-sitter or the build script (optimisation flags included) is out of
  scope.


## Patched grammar (user decision, 2026-11-12)

Upstream's `case_stmt` requires every `of` indented past `case`, but the Nim
manual and NEP-1 put `of` at `case`'s column, so ordinary Nim code gave an
ERROR over the whole proc body. The user chose to carry a patched grammar in
the `tenmoku-editor/tree-sitter-nim` fork. Commit d7408cc (on 67cd5540,
generated with tree-sitter-cli 0.25.10, LANGUAGE_VERSION 15 unchanged) lets
`of`/`elif`/`else` branches sit at `case`'s column or one level in, adds an
`elif_branch` node, and adds test/corpus/case_branches.txt (upstream corpus
270/270). The parse table barely grows (19,792 to 19,799 states).

- Pin the fork commit above in the git form; the MIT `LICENSE` is unchanged.
- `test/sample.nim` must include the same-column form
  (`proc f(x: int) =\n  case x\n  of 1: echo 1\n  else: discard`) and the
  indented form, both with no ERROR or MISSING node. Use `elif_branch` in
  highlights.scm if `elif` needs a capture there.
- Record the patch: a comment in `grammar-source.toml`, the header comment of
  `highlights.scm`, and the THIRD_PARTY.md row ("tenmoku-editor/tree-sitter-nim,
  a fork of metacraft-labs/tree-sitter-nim at 67cd5540 with Tenmoku's case/of
  patch"). Append-only; no NOTICE.
- Known limit: a case *expression* (`let y = case x` with branches on the
  lines below) still mis-parses its `else` as a call, with no ERROR. The fork's
  branch tenmoku/case-expr-continuation (2a9e03c) fixes it but grows parser.c
  from 42 to 62 MB; it is not pinned here.

## Resume from argon's work

Cherry-pick `867574e18` from argon's `parked-T1629-nim` branch
(`ssh://argon/home/daryl/code/fastedit-lanes`): LICENSE, extension.toml,
grammar-source.toml, highlights.scm, injections.scm, test/sample.nim and its
THIRD_PARTY.md row. Skip the spec-note commits 39b5bedc4 and d6e5441a5. Then
repoint grammar-source.toml at the fork commit, add the same-column sample,
and generate the golden. Argon measured the base wasm at 8,330,587 bytes.

## Implementation notes

- Resumed from argon's `867574e18` (cherry-picked; the T333 spec-note commits
  skipped). `grammar-source.toml` now pins the fork,
  `tenmoku-editor/tree-sitter-nim` at `d7408ccad04d1ae23d387ba6d1413391d76e95ab`,
  with the patch described in its comment; `LICENSE` is byte-identical to the
  fork's at that commit.
- `test/sample.nim` has the NEP-1 same-column form (`describe`: `case` / `of` /
  `else` at one column) and the indented form with an `elif` branch (`grade`).
  It parses with no ERROR or MISSING node: `UPDATE_GOLDEN=1
  scripts/build-grammar-wasm.sh nim` passes `every_extension_highlights_its_sample`.
  The golden colours keywords (`of`/`elif`/`else` in both forms), the plain,
  raw and triple-quoted strings and char literals, numbers (`0xFF'u8`,
  `3.14e-2`, `1_000`), line, nested block and doc comments, procs, types and
  pragmas (`@attribute`).
- Size: `tree-sitter-nim.wasm` is 8,386,630 bytes (`MAX_GRAMMAR_BYTES` is
  16 MiB).
- Load under `GrammarDeadlines::DEFAULT` (10 s), a cold `register_language`
  (highlights and injections compiled) in a fresh test process, measured with a
  scratch test since deleted, two runs each: debug 1.02 s and 0.74 s (sample
  parse 0.17 s), release 0.15 s and 0.10 s (sample parse 0.02 s).
- `crates/fastedit/tests/grammar_pack_recorded.rs`: `nim` added to
  `NOT_RELEASED` (in sorted order); the Mac's next grammar release records it.
- Attribution: one appended row in THIRD_PARTY.md's WebAssembly table naming
  the fork, the base commit and the patch; per the spec, no adapted-code row
  and no NOTICE entry.
