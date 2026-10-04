# Markdown: the slow keystroke reparse

Status: **decided: accept ~6–8 ms for now, and post the upstream issue drafted
below.** No fork, no dependency change. Decided 2026-10-03. Source card T33
(follow-up to T4).

## Context

A Markdown reparse after one typed letter costs about 6 ms on 10k lines against
the plan's 5 ms target. Rust (0.87 ms) and JSON (0.25 ms) meet it. T4 (PERF.md,
*Syntax*) found the cost isn't ours: plain tree-sitter driving
`tree_sitter_md::LANGUAGE` is slower still (7.5 ms), the block scanner and lexer
take under 0.4%, and the time is in tree-sitter's parser (reuse walk, subtree
summarising, stack pushes, freeing), growing with the distance from the edit to
the end of the file. The block grammar's tail is walked and reduced node by node
where Rust and JSON reuse it whole.

## Numbers

`cargo bench -p fe-syntax --bench syntax -- "markdown/reparse keystroke"` at
b9602bc (current main's tip when run), tree-sitter 0.27.0, tree-sitter-md 0.5.3,
Apple M3 Max, 2026-10-03. The machine was loaded (load average ~13 on the
machine's cores while other lanes ran), so these are upper bounds:

| Corpus | Criterion time (low / mid / high) |
|---|---|
| 10k lines, run 1 | 9.48 / 9.72 / 10.21 ms |
| 10k lines, run 2 | 7.35 / 7.62 / 7.89 ms |
| 8 MB (442k lines) | 376 / 416 / 457 ms |

PERF.md's quiet-machine figures are 6.1 ms at 10k lines and 640 ms at 16 MB.
The 8 MB case is under the 8 MB `MAX_PARSE_BYTES` limit, so it is the worst case
a user can reach. Treat the table as "no better than before, no clear
regression"; rerun on an idle machine before quoting a figure externally.

## Options

### 1. Upstream issue — chosen, to be posted by the user

The cost sits in tree-sitter's core, which owns the reuse walk, but the trigger
is the grammar's shape: `document` holds every top-level `section` as a direct
child, and each `section` is a long `repeat` of blocks. An edit near the top
leaves a long unchanged tail that the parser re-reduces rather than reusing.
Which side should change is the maintainers' call, so the issue goes to
`tree-sitter-grammars/tree-sitter-markdown` and asks them to move it to
`tree-sitter/tree-sitter` if the walk is the right place. It costs us nothing
and has no dependency impact.

Draft (not posted):

> **Title:** Incremental reparse of a large document is O(distance to end of
> file), not O(edit)
>
> **Version:** tree-sitter 0.27.0, tree-sitter-md 0.5.3 (block grammar only).
>
> **Reproduction.** Generate a Markdown file of ~10,000 lines (about 187 KB):
> repeated sections, each a `##` heading, a paragraph, a bullet list, a block
> quote and a fenced code block. Parse it once. Then repeatedly insert one
> letter inside a word, call `Tree::edit`, reparse with the old tree, and
> delete the letter again outside the timed region. Pick the edit position at
> random. Compare the same loop on tree-sitter-rust and tree-sitter-json
> corpora of the same size.
>
> **Measurements** (M3 Max, release build, `tree-sitter` at `opt-level = 3`):
> Markdown 6–7.5 ms per reparse; Rust 0.9 ms; JSON 0.25 ms. Markdown cost by
> edit position, as a fraction of the file: 6.5 ms at 1%, 5.8 ms at 25%,
> 4.2 ms at 50%, 2.9 ms at 90%. The cost falls as the edit nears the end, so it
> depends on the length of the unchanged tail. `get_changed_ranges` returns
> nothing, so the new tree has the same shape. At 8 MB the reparse is ~0.4 s.
>
> **Where the time goes.** A profile of 20,000 reparses: under 0.4% in the
> external scanner and lexer together. About a quarter in `ts_parser_parse`
> (reuse checks), a fifth in `ts_subtree_summarize_children` and 3% in
> `ts_subtree_compress`, which rebuild the `document` node's ~666 children and
> the edit's ancestors. The rest is stack pushes, `reusable_node_advance` and
> freeing subtrees.
>
> **Expected.** The parser reuses the unchanged tail whole, as it does for Rust
> and JSON.
>
> **Question.** Is this the grammar's structure (a flat `repeat` of sections
> that can't be reused across the external scanner's state), or the reuse walk?
> If the former, would a change to how `document`/`section` are built be
> acceptable? If the latter, is there a known limit on reuse through nodes that
> follow an external-scanner token?

The last paragraph is a question, not a claim: we haven't established which side
owns the cause (see "What we haven't shown").

### 2. A grammar fork or patch — not now

- **What it would change.** At minimum `tree-sitter-markdown/grammar.js` (611
  lines) to restructure `document`/`section`, then regenerate
  `src/parser.c` (59,787 lines, generated). The external scanner
  (`src/scanner.c`, 1,602 lines) might need changes too, because reuse across
  its serialised state is the likely blocker. If the cause turns out to be in
  tree-sitter's walk, no grammar patch can fix it and the fork would have to be
  of tree-sitter itself (a much larger, C-core fork).
- **Size.** The hand-written diff is probably small; the vendored diff is
  large because `parser.c` is regenerated. Regenerating needs a matching
  `tree-sitter` CLI in the toolchain, which we don't have today.
- **Compatibility.** Node shapes feed our highlight and injection queries
  (`HIGHLIGHT_QUERY_BLOCK`, `INJECTION_QUERY_BLOCK`) and the inline grammar's
  injection. Restructuring sections risks changing them.
- **Licence.** MIT (`tree-sitter-md` 0.5.3 declares `license = "MIT"`),
  already audited in THIRD_PARTY.md. CLEAN_ROOM.md allows patched grammars with
  a person's approval, vendored under `vendor/<crate>/` with a `README.md` and
  a patched mark in THIRD_PARTY.md. The audit adds a table row, nothing new.
  `vendor/tree-sitter-yaml` is the precedent.
- **Upkeep.** Heavy relative to the gain: every upstream release needs a
  rebase of generated C, and `tree-sitter-md` is shared with the inline grammar
  and the `tree-sitter` runtime version. The yaml patch is one scanner file;
  this wouldn't be.
- **Dependency change.** Needs `[patch.crates-io]` in `Cargo.toml` and a
  `Cargo.lock` change. Not done here; a later task once the user picks this.

Rejected for now because the gain is unproven, the upkeep is high, and the
current cost is tolerable (below).

### 3. Accepting ~6 ms — chosen, alongside option 1

- At 10k lines the reparse is one frame-sized chunk of background work. The
  view's synchronous budget is 1 ms: a reparse over budget spends it and
  finishes in the background, so colours lag the text by the reparse time and
  typing isn't blocked (PERF.md, *Syntax*).
- Markdown files are rarely large. The slow case needs tens of thousands of
  lines; most notes are far shorter, and the cost scales with the distance to the
  end, so the common case is well under 6 ms.
- Highlighting itself is cheap (0.05 ms for 60 rows).
- The worst case is bounded by `MAX_PARSE_BYTES` (8 MB): ~0.4 s lag, with the
  text still responsive.

## What we haven't shown

We have profiled where the time goes but haven't shown which of the grammar or
tree-sitter's reuse walk is responsible, and we haven't prototyped a fix. That
is why the issue asks the question rather than naming a cause, and why a fork is
premature.

## Decision

Accept the cost, keep the 5 ms target open for Markdown, and post the drafted
issue. Revisit the fork if upstream declines or doesn't answer, if a Markdown
file in real use shows visible colour lag, or if the issue's replies show a
grammar change would fix it. A fork would then be a new task (with the person's approval
CLEAN_ROOM.md requires) that includes the `Cargo.toml` and `Cargo.lock`
changes.
