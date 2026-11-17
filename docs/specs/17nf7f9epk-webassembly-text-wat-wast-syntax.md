---
task_uid: "01m7knnpt420xjhj17nf7f9epk"
task: "T412"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/wat/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form at a full 40-hex commit, the licence verbatim, extension.toml as the spec says; one appended THIRD_PARTY.md WebAssembly row and a NOT_RELEASED row in grammar_pack_recorded.rs; nothing else changes"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh wat passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for wat, the sample parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "The sample golden colours keywords, types, functions, identifiers, strings, numbers and comments; every_extension_is_well_formed and grammar_install_tests::every_first_party_manifest_parses pass; cargo nextest run -p fe-syntax -p fastedit passes, including grammar_pack_recorded"
    required: true
    manual: false
---

# WebAssembly text (.wat, .wast) syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/wat) (Mac T1922)

The user (2026-11-17), after "No syntax highlighting for ftl files? We can't even fully display our own source", agreed to cards for the other file types in this repo with no highlighting. This one: wat, wast files.

A first-party WebAssembly grammar extension, extensions/grammars/wat/, exactly as T1629 (Nim), T1900 (Prisma) and T1920 (Fluent) do it: the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form to https://github.com/wasm-lsp/tree-sitter-wasm at a full 40-hex commit, its licence (Apache-2.0 WITH LLVM-exception (GitHub reports NOASSERTION); confirm the repo's LICENSE file says so, and stop and report if it doesn't allow redistribution), extension.toml (extension(s) wat, wast, comments as the language has them, no shebangs, packs ["systems"] unless a better existing pack fits; check registry.toml), one appended THIRD_PARTY.md WebAssembly row, and a NOT_RELEASED row in crates/fastedit/tests/grammar_pack_recorded.rs. Use the upstream highlights.scm if it has one (credit it), else write one. If the generated parser's ABI is outside what our tree-sitter loads and can't be regenerated without a fork, block for the user's fork decision.

Sample: one of this repo's .wat files plus a module with folded and flat instructions, types, imports/exports, memory, data and line/block comments, parsing with no ERROR or MISSING node; the golden colours keywords, type and function names, identifiers, strings, numbers and comments.

## Implementation notes

- Source: wasm-lsp/tree-sitter-wasm at `2ca28a9f9d709847bf7a3de0942a84e912f59088` (its newest commit, 2022-05-17). Licence confirmed at that commit: its `LICENSE` is the single line `SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception` (package.json says the same), a permissive licence that allows redistribution; deny.toml already accepts it. `LICENSE` is that file verbatim; since it names the licence without its text, `LICENSE-APACHE` holds the Apache-2.0 text with the LLVM exceptions (the same text as extensions/grammars/wit/LICENSE).
- ABI: its parser.c files are ABI 13, the oldest tree-sitter 0.27 loads (`TREE_SITTER_MIN_COMPATIBLE_LANGUAGE_VERSION` is 13), so nothing was regenerated or forked. There is no scanner. The module is about 238 KB.
- One grammar for both extensions: the repository holds two grammars, `wat` (one module, or bare module fields) and `wast` (a script of modules, registrations and assertions), which extends `wat` and whose root also takes bare module fields, so it reads every `.wat` the `wat` grammar does. The directory is `wat` (language `wat`, extensions `wat` and `wast`) with `grammar = "wast"`, `src = "wast/src"`, so the module is `tree-sitter-wast.wasm`.
- Pack: `systems`, beside wit and wgsl.
- highlights.scm is written for Tenmoku (upstream ships no queries). Structure words (`module`, `func`, `param`, `block`, `end`, the wast commands) are keywords, as are branches (`br`, `br_if`, `br_table`), `return` and `unreachable`; other instructions are `@function.builtin`; value types are `@type.builtin`; a `$name` is `@function` where it names or calls a function, `@type` for a type, `@module` for a module, `@label` for a block label or branch target, `@variable.parameter` for a param and `@variable` otherwise. Import and export names are `@string.special`; `offset=`/`align=` are parameters. Last pattern wins.
- Sample: `test/sample.wast`. Every `.wat` in this repo (the 16 under kiln/crates/kiln-plugin/tests/fixtures/) is a component-model file, `(component …)` with `core module`, `instance`, `canon lift`, `alias` and so on, which this 2022 grammar predates: none parses without errors (hostile/loop_call.wat is one ERROR node; commands.wat has 56, strings.wat 80), though error recovery still colours their comments, strings, `$names`, numbers and core instructions. So the sample takes the two core modules out of hostile/loop_call.wat, verbatim but for `core module` → `module` and one level less indent, then a module written for it (types, imports of a func, table and global, memory, data with escapes, globals, elem, flat and folded control flow, memory arguments, call_indirect, an annotation, start, exports, line and nested block comments) and a few wast commands. No tree-sitter grammar for the component-model text format exists that I could find; highlighting the repo's own files fully needs one (a fork of this grammar adding the component rules), a decision for the user.
- Component files: the user decided (2026-11-17, after this was reported) not to fork the grammar for the component model on this card; the finding goes to the Mac, with no follow-up card yet.
- Review round 1: in an element segment only the indices after the offset are functions (`(elem $segment (i32.const 0) $f)`), so a segment's own name stays a variable; a table's inline `elem` names functions; `(ref null $t)` and `ref.null $t` name types; a binary or quoted module, `script`, `input` and `output` name modules; annotation comments are comments; the wast `ref.func`/`ref.extern`/`ref.null` results are instructions. A lane instruction (`i8x16.extract_lane_s 0`) and a NaN result are coloured as whole nodes, since their `_s`/`_u` suffix is an anonymous regex token a query can't name; their numbers still win, and the lane opcode's span includes the space before its lane number. `f32.const nan:canonical` lexes as an ordinary `f32.const` with a NaN, so `result_const_nan` is captured but in practice never seen. The sample's `$lanes` module and the commands after it cover each.
