---
task_uid: "01m5ak9yjgnmbebsygdz7hf6rt"
task: "T230"
rev: 6
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "every static table (parse table, small parse table, parse actions, lex modes, alias sequences, field map slices and entries, supertype maps, reserved words, public symbol map, primary state ids, symbol and field names) is validated for bounds and cross-references at load, and each has a hostile fixture in crates/fe-syntax/tests/wasm_table_validation.rs that returns a GrammarError naming the file instead of reading out of bounds"
    required: true
    manual: false
  - id: "C2"
    text: "the extensions decision record's sandbox claim (§6) and vendor/tree-sitter/README.md are restored to: a malformed grammar can't crash the editor"
    required: true
    manual: false
  - id: "C3"
    text: "cargo deny check passes and the existing runtime_grammar tests still pass"
    required: true
    manual: false
  - id: "C4"
    text: "F6 is addressed by a capacity check: parser.c bounds the accepted root's production and structural child count against the alias sequences before rebuilding the root, with a fixture in wasm_table_validation.rs that really exercises the guard (a reduced root with a nonzero production and enough extra structural children to exceed its alias sequence, asserted with parse_fails, and failing when the guard is removed); the exactly-one-non-extra-tree rule and an external-token leaf fixture are follow-up cards (T235), and F7 is T234"
    required: true
    manual: false
  - id: "C5"
    text: "F8: a grammar whose keyword capture-token ID cannot be held by the inline-subtree fallback (\u003e= 256 with token_count \u003e 256) is rejected at load with a file-bearing GrammarError (the guard in wasm_store.c), shown by a hostile test in wasm_table_validation.rs that loads such a module; the end-to-end keyword-fallback fixture cannot be generated from tree-sitter 0.27.0's output and is a documented limit (see the spec's Known limit)"
    required: true
    manual: false
  - id: "C6"
    text: "F9: the external scanner state table's inferred size is checked against Wasm memory at load, with a hostile fixture (states near 0x7ffffff0) returning a GrammarError"
    required: true
    manual: false
---

# Validate a Wasm grammar's static tables before tree-sitter's parser reads them, so a corrupt grammar is an error, not an out-of-bounds read (T845 review F8) (Mac T1005)

## Problem

T845 loads a Wasm grammar's static tables (parse table, small parse table, parse actions, lex
modes, alias sequences, field map slices and entries, supertype maps, reserved words, public symbol
map, primary state ids, symbol and field names) out of Wasm memory in
`vendor/tree-sitter/src/wasm_store.c` with only an inside-memory check, and the native parser then
indexes them unchecked. A module with correct exports but corrupt tables can make the parser read
out of bounds, so a malformed grammar can crash the editor. T845 landed with the extensions
record's sandbox claim narrowed (T845 review F8, user decision 2026-10-19); this card restores it.

## Change

Validate every table at load, before the language is returned, for bounds and cross-references
(state ids within the state count, symbol ids within the symbol count, action and alias indices
within their arrays, field map ranges within the entries, supertype and reserved-word ranges, name
tables NUL-terminated and in memory, and so on), in `wasm_store.c` and wherever the consumers in
`parser.c`, `language.c`, `subtree.c`, `query.c` and `node.c` index them. A failure is an error
from `ts_wasm_store_load_language`, which `crates/fe-syntax/src/runtime.rs` maps to a
file-bearing `GrammarError`. Each table gets a hostile fixture (built in-test by patching a valid
module's data segment, or checked in with its source noted) in
`crates/fe-syntax/tests/wasm_table_validation.rs`. Then restore the claim "a malformed grammar
can't crash the editor" in the extensions decision record (§6) and `vendor/tree-sitter/README.md`,
and update the README's patch list.

## Out of scope

Running grammars in a helper process; the epoch deadline and export checks (T845).

## Lent

Lent from Mac T1005. Depends on Mac T845 (commit 6acda579; adds vendor/tree-sitter and the runtime grammar code), which helium trunk lacks: ready only after the Mac lead syncs helium.

## F6 (user decision, 2026-10-20): a capacity check, not the one-root rule

After three review rounds and one extra round the HIGH finding F6 (`vendor/tree-sitter/src/parser.c` ~1099, the accepted stack's shape) stayed open. The user decided to ACCEPT the capacity check the worker built, which bounds the accepted root's production and structural child count against the grammar's alias sequences before the root is rebuilt, so `ts_subtree_summarize_children` cannot index past an alias sequence. It does not require "exactly one non-extra tree": the worker found that rule rejects valid parses.

Not done here, as follow-up cards: the exactly-one-non-extra-tree rule (with the false-rejection analysis) and a fixture with a real external-scanner token leaf, which needs a compiled grammar and a wasm toolchain this environment lacks. The fixtures in `wasm_table_validation.rs` cover the capacity check (a malformed accepted stack that exceeds the alias sequence fails to parse) and use an ordinary heap-token leaf in place of the external one.

## Known limit: F7

The medium F7 (`parser.c` ~1680, a null lookahead with an empty EOF entry) is a follow-up card (T234); do not fix it here.

Review findings that only ask for those follow-ups are out of scope for this card.

## Released 2026-10-21 (Mac y32): finish this card

The user released the hold. T236 is abandoned as redundant; this card lands the whole validator with the open findings below fixed (the earlier rounds' findings are carried). Start from lane/ygdz7hf6rt.


T230 (Mac T1005, validate a Wasm grammar's static tables) was held out unlanded after four review rounds that kept finding new problems. Its work is committed on `lane/ygdz7hf6rt`. Open findings at the stop:

- **F8 (high), `vendor/tree-sitter/src/wasm_store.c` ~1610:** the keyword capture token is checked only against `token_count`, but the keyword fallback writes it into an existing inline subtree. A grammar with `token_count` > 256 and `keyword_capture_token` = 256 passes validation; after a short keyword becomes invalid following a reduction, `parser.c` ~1827 calls `ts_subtree_set_symbol`, whose inline branch asserts `symbol < UINT8_MAX`: the editor can abort (without assertions the symbol is truncated). Fix: reject capture-token IDs the inline fallback cannot hold at load, or promote the subtree before assigning a larger symbol; add a hostile regression exercising keyword fallback with a large capture-token ID.
- **F6 (high), `crates/fe-syntax/tests/wasm_table_validation.rs` ~1136:** the capacity-check test discards every parse result and builds only empty stacks or childless token roots, which bypass the `child_count > 0` capacity branch, so it passes even with the guard removed. Fix: a fixture accepting a reduced root with a nonzero production and enough extra structural children to exceed its alias sequence, asserted with `parse_fails`.
- **F9 (medium), `wasm_store.c` ~2173:** the external scanner state table's inferred size is never checked against Wasm memory (a fieldless grammar can put states at 0x7ffffff0 with an unused field_names pointer at 0x7ffffff8; the inferred 8-byte capacity satisfies the lex-mode check although the range is outside memory).

## Change

Start from `lane/ygdz7hf6rt` (or the same diff against the current trunk), fix F8, add the F6 fixture, fix F9, then land T230's whole validator. Follow-ups T234 (F7) and T235 (accepted-stack rule, external-token fixture) are separate.
## Known limit (review round 1, F8): no faithful fallback fixture can be generated on this host

`tree-sitter generate` (0.27.0) always numbers the word token 1, so no grammar built with the host toolchain can reach the inline-subtree keyword fallback with a capture token of 256 or more (verified: with 280 named regex tokens declared before the word rule, `sym_ident = 1` and `keyword_capture_token = sym_ident`). A faithful end-to-end fixture would need a hand-renumbered `parser.c`, or a patched data segment whose lexer never returns that ID, which proves nothing more than the load-time check does. C5 is therefore the load-time rejection: the guard in `wasm_store.c` returns a file-bearing `GrammarError` for a keyword capture-token ID the inline fallback cannot hold, shown by the committed hostile test (957a8087); the end-to-end fallback is a documented limit, recorded here so a person can hand-renumber a generated grammar later if wanted.
