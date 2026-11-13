---
task_uid: "01m793m5hj1tbkvrq5r8rk4f1t"
task: "T365"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/verilog/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned as the spec says (a 40-hex commit of the named repository unless the notes say otherwise) and the grammar's licence files verbatim; nothing else changes except appended THIRD_PARTY.md rows and the cap raise in C2"
    required: true
    manual: false
  - id: "C2"
    text: "fe_syntax::MAX_GRAMMAR_BYTES is 20 MiB (20 \u003c\u003c 20 in crates/fe-syntax/src/runtime.rs) and crates/fe-syntax/tests/l10n.rs pins 20971520 in the refusal string; scripts/build-grammar-wasm.sh verilog passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES and the load deadline, and wasm_parity's ignored every_extension_highlights_its_sample passes for verilog, with test/sample.sv parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses pass with verilog present, its extension.toml giving the spec's file extensions, file names, comment tokens and shebangs with no clash with a built-in"
    required: true
    manual: false
  - id: "C4"
    text: "test/sample.sv.golden colours keywords, strings, comments, numbers, functions and types (and injected languages, if any)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add Verilog and SystemVerilog syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/verilog) (Mac T1476)

# Verilog and SystemVerilog syntax as a first-party WebAssembly grammar

Filler card: one new directory, `extensions/grammars/verilog/`, following
`extensions/grammars/README.md` (added by the discovery card this depends on)
and the existing eight (`extensions/grammars/ruby/` is the plainest).

## This language

- Grammar: https://github.com/tree-sitter/tree-sitter-verilog, directory `src`. Pin: master (HEAD 227d277b)
- Licence: MIT. External scanner: none. Parser size: 44 MB.
- File extensions: v, vh, sv, svh. File names: -. Shebangs: -.
- Comments: line `//`, block `/* */`.
- Queries: none upstream: adapt nvim-treesitter queries/verilog (Apache-2.0)
- Notes: the grammar parses SystemVerilog, so one card covers both; big parser: the wasm is 18,169,275 bytes, so this card raises `MAX_GRAMMAR_BYTES` to 20 MiB (below)

## Requirements

- `grammar-source.toml` in the git form (`git`, `rev` = a full 40-hex commit,
  `src`, `grammar`) unless the notes say otherwise. Pin the newest commit that
  builds and passes; take `grammar` from the `tree_sitter_<x>` export in
  `parser.c`, which also fixes `wasm = "tree-sitter-<x>.wasm"`.
- `LICENSE` (and any `LICENSE-*`/`NOTICE`) copied verbatim from the grammar at
  that commit. Queries from nvim-treesitter are Apache-2.0: add
  `LICENSE-APACHE` and say so as `kotlin/` does. Don't copy Helix or Zed
  queries (MPL-2.0 / mixed).
- `extension.toml`: `id = "tenmoku.grammar_verilog"`, `name = "Verilog and SystemVerilog syntax"`,
  one `[[contributes.grammars]]` with `language = "verilog"`, the extensions,
  file names, `line_comment`/`block_comment` and `shebangs` above (keys from
  the comment-tokens card), and `earlier_patterns_win` matching the query's
  convention (tree-sitter style: true; nvim style: false). No `packs` or
  other new keys: pack membership comes from a later card.
- `highlights.scm`: self-contained (inline anything a `; inherits:` line
  pulls in, adapted to this grammar's node names); captures in fe-syntax's
  vocabulary (`crates/fe-syntax/src/highlight.rs` NAMES/ALIASES). Drop
  patterns using predicates fe-syntax doesn't support rather than leaving
  them inert.
- `injections.scm`: only injections into languages that exist (built-ins, or
  first-party ones listed in `parity_needs`); a comment-only file if none.
  No locals, folds, indents or outline queries (no first-party extension
  ships them and the manifest has no keys for them).
- `test/sample.sv`: 40-120 lines of idiomatic code touching
  keywords, strings with escapes, numbers, comments, functions, types and any
  injection; it must parse with no ERROR or MISSING node. Generate
  `test/sample.sv.golden` with
  `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh verilog`, then read it: every
  token class above is coloured and nothing is obviously wrong.
- Append the rows to `THIRD_PARTY.md` ("Grammars shipped as WebAssembly"
  table, and the adapted-queries table if queries came from elsewhere). Append
  only; don't reorder or edit other rows.
- Change nothing outside `extensions/grammars/verilog/`, those appended
  rows and the cap raise below. If the module won't load (ABI, the table
  validator, a libc import the wasm stdlib lacks, over the new 20 MiB
  `MAX_GRAMMAR_BYTES`, the load deadline),
  or a built-in already claims an extension, block the card with the exact
  error: fixing fe-syntax, the vendored tree-sitter or the build script is
  out of scope.


## Raise the grammar size cap to 20 MiB (user decision, 2026-11-12)

tree-sitter-verilog at 227d277b builds (already `-Os`) to an 18,169,275-byte
wasm, 8 % over today's 16 MiB cap, and no older upstream commit is smaller
(every generated `parser.c` since 2021 is 46.0-46.4 MB). The user chose to
raise the cap rather than drop the card:

- `crates/fe-syntax/src/runtime.rs`: `MAX_GRAMMAR_BYTES` from `16 << 20` to
  `20 << 20` (20,971,520). Everything else reads the constant.
- `crates/fe-syntax/tests/l10n.rs`: the pinned refusal string's
  `16777216` becomes `20971520`. Nothing else in fe-syntax changes.
- If the 18 MB module then misses the load deadline (`GrammarDeadlines`), block
  with the measured time: raising the deadline is not in this card.

## Resume from argon's work

Argon's lead did the directory already: cherry-pick `79116aeca` from argon's
`parked-T1476-verilog` branch (`ssh://argon/home/daryl/code/fastedit-lanes`,
head b639068e) onto this lane: the verilog directory (LICENSE, LICENSE-APACHE,
extension.toml, grammar-source.toml, highlights.scm, injections.scm,
test/sample.sv) and its THIRD_PARTY.md row. Skip that branch's two spec-note
commits. Still to do: the cap raise and `test/sample.sv.golden`.

## Implementation notes

- Cherry-picked argon's `79116aec` (`-x`); the `THIRD_PARTY.md` conflict kept
  every trunk row, with the verilog row appended after them. The spec-note
  commits `520ad175` and `b639068e` were skipped.
- `MAX_GRAMMAR_BYTES` is `20 << 20`; `tests/l10n.rs` pins `20971520`.
- `scripts/build-grammar-wasm.sh verilog` on this Linux host builds
  `tree-sitter-verilog.wasm` at 18,169,263 bytes (argon's 18,169,275 differs by
  the output path wasm-ld embeds), under the new cap.
- Load deadline, measured with a scratch test (not committed) that calls
  `register_language` once per fresh process under `GrammarDeadlines::DEFAULT`
  (10 s load), then parses `test/sample.sv`; no compile cache is involved, so
  each load is cold. Load average about 7 on 12 cores:
  - `--release`: 290 ms, 288 ms, 292 ms; parse 17-19 ms.
  - debug (test profile): 3.14 s, 3.52 s, 3.81 s; parse 0.23-0.29 s.
  Both are within the 10 s deadline, so it isn't raised. The sample parses
  with no ERROR or MISSING node.
- argon's `test/sample.sv` didn't parse at 227d277b: the grammar rejects a
  task-call statement with or without arguments (`write_byte(v);`), and any
  function call in a `parameter`/`localparam` initializer (`clog2(DEPTH)`,
  `$clog2(DEPTH)`). The testbench now calls a function inside an assignment
  (`din = pattern(i);`) and a variable initializer (`int aw = clog2(N);`),
  and `AW` is a module parameter.
- `highlights.scm` changes after the golden's first read, both in its header:
  compiler directives capture their keyword token (`` `define`` and the rest
  now coloured; the whole-directive captures had coloured the trailing space
  and line break and would colour an include's path as a keyword), and
  `(system_tf_identifier)` is `@function.builtin` wherever it appears, so
  `$display(...);` and `$finish;` statements are coloured, not only calls
  inside expressions.
- Known, from the grammar: string escapes (`\"`, `\n`, `\t`) stay `string`
  (`string_literal` has no escape child to capture); a lone identifier as a
  parameter value (`.DEPTH(N)`) parses as a `data_type` and is coloured as a
  type, since a parameter may be a type; `#` of `#(` in a module header is
  `@constructor`, as nvim-treesitter's query has it.
- Carried finding T276 F1 (`THIRD_PARTY.md:15`): the sentences it names
  ("deferred to the next card", "kiln/NOTICE … icon table") are no longer in
  trunk's `THIRD_PARTY.md`; the header now says kiln/ keeps its own copy in
  `kiln/THIRD_PARTY.md` with `kiln/NOTICE`. Nothing left to change.
- `fastedit::grammar_pack_recorded`'s `every_grammar_directory_has_a_recorded_pack_entry`
  failed with "pack.toml has no [languages.verilog]": the wasm has never been in a
  grammar release. With the scope widened (task revision 3), verilog is in its
  `NOT_RELEASED` list ("added on argon; recorded by the Mac's next grammar
  release"), in sorted order, as vps01's d595366f did for wgsl, vhdl, typst and
  scheme; the Mac's next grammar release replaces it with a recorded entry.
- Review round 1 (gg7n73):
  - F1: `real_number` and `fixed_point_number` are `@number` now. The sample
    adds an exponent form (`real tol = 2.5e-3;`) and a fractional time
    literal (`#12.5ns`), and the golden colours `0.5`, `2.5e-3` and `12.5`
    (`ns` stays `type.builtin`, as `time_unit` is).
  - F2: the query's provenance (nvim-treesitter `queries/verilog/highlights.scm`
    at `ca7993e6066077b1edcb71b82eb9bbaa61a7b90e`, Apache-2.0, text in
    `LICENSE-APACHE`) is in verilog's "Grammars shipped as WebAssembly" row,
    as every first-party grammar with borrowed queries does. A row in the
    "Code copied or adapted" table for an `extensions/` file fails the gate:
    `crates/fastedit/tests/third_party.rs` (`copied_files_and_rows_agree`
    reads only `crates/` and `kiln/`) and `tests/notice.rs` (each row's
    source must be in `NOTICE`, outside this card's scope). T305 (objc) and
    T326 (hlsl) hit this, and the convention since is the WebAssembly row
    only (Mac T1463). No row added.
