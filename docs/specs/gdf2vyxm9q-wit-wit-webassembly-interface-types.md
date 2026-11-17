---
task_uid: "01m7knnphytp8wh1gdf2vyxm9q"
task: "T411"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extensions/grammars/wit/ holds the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form at a full 40-hex commit, the licence verbatim, extension.toml as the spec says; one appended THIRD_PARTY.md WebAssembly row and a NOT_RELEASED row in grammar_pack_recorded.rs; nothing else changes"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/build-grammar-wasm.sh wit passes on the lane's host: the wasm builds and loads within MAX_GRAMMAR_BYTES, and wasm_parity's ignored every_extension_highlights_its_sample passes for wit, the sample parsing with no ERROR or MISSING node"
    required: true
    manual: false
  - id: "C3"
    text: "The sample golden colours keywords, types, functions, identifiers, strings, numbers and comments; every_extension_is_well_formed and grammar_install_tests::every_first_party_manifest_parses pass; cargo nextest run -p fe-syntax -p fastedit passes, including grammar_pack_recorded"
    required: true
    manual: false
---

# WIT (.wit, WebAssembly Interface Types) syntax highlighting as a first-party WebAssembly grammar (extensions/grammars/wit), for our plugin API definitions (Mac T1921)

The user (2026-11-17), after "No syntax highlighting for ftl files? We can't even fully display our own source", agreed to cards for the other file types in this repo with no highlighting. This one: wit files.

A first-party WebAssembly grammar extension, extensions/grammars/wit/, exactly as T1629 (Nim), T1900 (Prisma) and T1920 (Fluent) do it: the file set every_extension_is_well_formed requires, grammar-source.toml pinned in the git form to https://github.com/bytecodealliance/tree-sitter-wit at a full 40-hex commit, its licence (Apache-2.0; confirm the repo's LICENSE file says so, and stop and report if it doesn't allow redistribution), extension.toml (extension(s) wit, comments as the language has them, no shebangs, packs ["systems"] unless a better existing pack fits; check registry.toml), one appended THIRD_PARTY.md WebAssembly row, and a NOT_RELEASED row in crates/fastedit/tests/grammar_pack_recorded.rs. Use the upstream highlights.scm if it has one (credit it), else write one. If the generated parser's ABI is outside what our tree-sitter loads and can't be regenerated without a fork, block for the user's fork decision.

Sample: a slice of this repo's own .wit files (packages, interfaces, worlds, records, variants, enums, flags, resources with methods, use/import/export, doc comments), parsing with no ERROR or MISSING node; the golden colours keywords, type and function names, identifiers, strings, numbers and comments.

## Implementation notes

- Source: bytecodealliance/tree-sitter-wit at `f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6` (its newest commit, 2026-10-05). Its LICENSE is Apache-2.0 with the LLVM exception (Cargo.toml and package.json say `Apache-2.0 WITH LLVM-exception`), which allows redistribution; extension.toml and the WebAssembly row say so, and deny.toml already accepts that licence. Its parser.c is ABI 15 and compiles as committed with its scanner.c, so nothing was regenerated. The module is about 49 KB.
- Pack: `systems`, as the spec suggests (proto, the nearest IDL, is in `web`, which is for web servers and templates).
- highlights.scm is upstream's queries/highlights.scm, credited in its header, with these changes: the names a `use` brings in (and their `as` aliases) are `@type`, not `@module`, since a `use` imports types; a fixed-size list's length (`uint`, the grammar's only number) is `@number`, not `@constant`; `@spell` is dropped (no such highlight); a gate's `version` and `feature` are `@variable.parameter`. Last pattern wins, as upstream's (written for Neovim) expects.
- injections.scm is comment-only: upstream's injects the `comment` language, which Tenmoku doesn't have.
- The sample is slices of kiln-plugin's world/types/events/net.wit and fe-plugin's buffer.wit, plus a final interface, marked as written for the sample, since our files have no resources, feature gates, string literals or numbers: a resource with a constructor, methods, an async and a static function, borrow, fixed-size lists, stream/future/map, `@since`/`@unstable`/`@deprecated`, `@external-id("…")` and a block doc comment. Every .wit in the repo (both crates, released/ included) parses with no ERROR or MISSING node.
