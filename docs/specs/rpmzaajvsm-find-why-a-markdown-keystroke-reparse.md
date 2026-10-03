---
task_uid: "01m417z3p3t7paqwrpmzaajvsm"
task: "T4"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Manual: the decision file compares fastedit's Markdown keystroke reparse on the bench's 10k-line corpus with plain tree-sitter-md on the same edits, and gives a profile split between the block scanner, lexing and parsing"
    required: true
    manual: false
  - id: "C2"
    text: "Manual: PERF.md's Syntax section states the cause and either a cargo bench -p fe-syntax -- markdown reparse under 5 ms or the documented limit with its follow-up card or upstream issue"
    required: true
    manual: false
---

# Find why a Markdown keystroke reparse takes 6 ms on 10k lines, then fix or document it

Source: Paddock T-27 (`118a01f7-d281-4517-b66c-5327f87eb6f6`), P2, draft (needs the user's approval).
Split from T-27: this is the Markdown half; skipping doomed sync reparses is T-27a.

## Problem

PERF.md (`cargo bench -p fe-syntax`, 2026-10-02): a keystroke reparse of 10k lines of Markdown takes
**6.1 ms**, over the plan's 5 ms target; Rust takes 0.87 ms and JSON 0.25 ms. At the old 16 MB limit
it took 640 ms. Markdown uses `tree_sitter_md::LANGUAGE` (the block grammar) with
`HIGHLIGHT_QUERY_BLOCK` (`crates/fe-syntax/src/language.rs`). We don't know yet whether the cost is the
block grammar's external scanner, the grammar's structure (edits re-lexing to the end of the
enclosing block), or how we drive it.

## Change

An investigation, ending in a measured fix or a documented limit:

1. Reparse the bench's 10k-line Markdown corpus with plain tree-sitter (a scratch program, not
   committed) and compare with `cargo bench -p fe-syntax -- "markdown/reparse keystroke"`, to rule our
   wrapper in or out.
2. Profile the reparse (samply or Instruments) and report the share spent in the block scanner
   (`scanner.c` in tree-sitter-md, MIT), in lexing and in the parse table; note whether the cost
   tracks distance to the end of the enclosing block.
3. Don't change code here. If the cause is ours, describe the fix in the decision file for a
   follow-up card. A grammar patch or fork is a dependency change (`Cargo.lock` is a human path, and
   CLEAN_ROOM.md requires a licence audit), so propose it, don't do it; an upstream issue is fine.
4. Update PERF.md's Syntax section with the finding: either the new number under 5 ms or the limit
   and its cause.

A lower parse limit for Markdown belongs to the memory-budget decision (T-26), not here.

## Implementation notes

Decision: document the limit, no code change.

**Wrapper ruled out (C1).** Scratch program (not committed): plain `tree_sitter::Parser` with
`tree_sitter_md::LANGUAGE`, a contiguous `String`, the bench's corpus (10k lines, 186,926 bytes),
one letter inserted inside a word then removed untimed, `tree.edit` + `parse(old_tree)` timed.

| | mean per keystroke |
|---|---|
| fastedit `cargo bench -p fe-syntax -- markdown/reparse keystroke/10k` | 6.12 ms (6.02–6.31) |
| plain tree-sitter, 400 random in-word positions | 7.55 ms |
| plain tree-sitter, full parse | 28.7 ms (bench: 29.4 ms) |

By position in the file (plain tree-sitter, 40 keystrokes each): 1% 6.5 ms, 10% 5.9, 25% 5.8,
50% 4.2, 75% 3.5, 90% 2.9. Cost falls with distance *to the end*: the reparse re-walks the tail.
`Tree::changed_ranges` is empty, so the result tree has the same shape. The tree is
`document` with 666 `section` children (about 5 children each).

**Profile (C1).** macOS `sample`, 20,000 reparses at 10% of the file, 5,463 samples. Lexing and
scanning are classified from the call tree (inclusive), so the generated `ts_lex`, the md `scan`
helper and `ts_lexer__*` all count; the rest is by top of stack. About 7% (402 samples) is the
scratch program's own offset arithmetic, outside tree-sitter.

| where | samples | share |
|---|---|---|
| block scanner (`scan`, `tree_sitter_markdown_external_scanner_*`) | 15 | 0.3% |
| lexing (`ts_lex`, `ts_lexer__*`) | 8 | 0.1% |
| `ts_parser_parse` self (reuse checks, state machine) | 1,425 | 26% |
| `ts_subtree_summarize_children` | 1,072 | 20% |
| `ts_stack_push`, `stack__iter` | 546 | 10% |
| `reusable_node_advance` | 293 | 5% |
| `ts_subtree_release`, `ts_subtree_compress`, malloc/free, memmove | ~950 | ~17% |
| `ts_language_next_state`, `ts_parser__reduce`, other tree-sitter | ~750 | ~14% |
| scratch program | 402 | 7% |

Scanner plus lexing is about 0.4%, so neither is the cost; it is tree-sitter's parser walking and
re-reducing the unchanged tail (and re-summarising the wide `document` node) on every edit.
`samply` isn't installed, so `sample` stood in for it; the split is by function, not by line.

**Cause is not ours.** The wrapper is faster than plain tree-sitter here, so there is no fix on
our side. Remaining options, all dependency changes (propose, don't do): an upstream
tree-sitter-md/tree-sitter issue about the tail being re-reduced after an edit; a grammar
patch/fork that lets section nodes be reused whole (needs a licence audit per CLEAN_ROOM.md and a
`Cargo.lock` change). Neither is done here. Follow-up card: Regatta T33, "Markdown keystroke reparse: upstream tree-sitter-md issue or grammar fork". A lower Markdown parse limit is T-26's.
