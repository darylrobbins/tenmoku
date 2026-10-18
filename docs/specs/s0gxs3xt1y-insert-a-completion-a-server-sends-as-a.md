---
task_uid: "01m55v1a4dveay51s0gxs3xt1y"
task: "T174"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::completion::snippet_guard::snippet_items_insert_their_plain_text passes: insertTextFormat 2 items' edit texts are foo(a, b), x = f$, outer inner for the spec's three snippets, and a format 1 item's bar($1) is unchanged, each over the range the base decodes; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::completion::snippet_guard::malformed_snippets_stay_literal passes: a${1:b and cost: $ are inserted as they are"
    required: true
    manual: false
  - id: "C3"
    text: "snippetSupport stays false; the completion.rs module docs say snippet items are inserted as plain text until snippet support lands, and plain_snippet's doc names T857 as its replacement"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Insert a completion a server sends as a snippet as the text it expands to, instead of with literal $1 and ${2:x}, until snippet completions land (Mac T924)

## Problem

LSP completeness track, item 1 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md` §3:
"guard `insertTextFormat=2`"). fe-lsp advertises `snippetSupport: false` (`crates/fe-lsp/src/server.rs:2038`),
but nothing stops a server sending snippet items anyway, and some do (servers that pick the format from their
own settings, user-added `[lsp.servers.*]` ones). `convert` (`crates/fe-lsp/src/completion.rs:213-266`)
ignores `insertTextFormat`, so accepting such an item inserts its snippet source as typed: `foo(${1:a}, $0)`
lands as that text, `$` signs and all. VS Code always parses snippet items, so the user sees `foo(a, )`.

T856/T857 (inbox) will insert snippets properly, with tabstops. Until they land, a snippet item should insert
what the snippet expands to, without stops.

## Change

`crates/fe-lsp/src/completion.rs`:

1. New private `fn plain_snippet(source: &str) -> String`: the text a snippet expands to, by the LSP snippet
   grammar (the one VS Code parses):
   - `$1`, `${1}` and `$0` expand to nothing; `${1:text}` to `text`, expanded in turn (placeholders nest);
   - `${1|one,two|}` to its first choice, `one` (`\,` and `\|` escaped inside choices);
   - variables (`$NAME`, `${NAME}`) to nothing, and `${NAME:default}` to its default, expanded in turn;
   - `\$`, `\}` and `\\` to `$`, `}` and `\`; any other `\` is kept as it is;
   - anything malformed (an unclosed `${`, a `$` before nothing it can start) is kept literally from there,
     as VS Code's parser does.
2. `convert`: when an item's `insertTextFormat` is `Snippet` (2), its edit's text is `plain_snippet` of its
   `textEdit.newText`, else of its `insertText`, else of its `label`. Ranges, `additionalTextEdits` (never
   snippets), the filter text and the label are unchanged. Format 1 or none is as today.
3. The module docs' Edits bullet says snippet items are inserted as their plain text until snippet support
   lands, and `plain_snippet`'s doc says T857 replaces it (T857 flips `snippetSupport` and hands the source
   to fe-editor's snippet engine instead; tell its worker).

Tests: new `crates/fe-lsp/src/tests/completion/snippet_guard.rs`, declared by `mod snippet_guard;` in
`crates/fe-lsp/src/tests/completion.rs` (its only change; T913 adds `mod resolve_edits;` there the same way). It
uses only the base's API (`CompletionItem.edit.text`), so it compiles at the base, where it fails.

- `snippet_items_insert_their_plain_text`: with the fake answering a list of four items, format 2 with
  `textEdit.newText` `foo(${1:a}, ${2:b})$0`, format 2 with `insertText` `${1|x,y|} = ${TM_FILENAME:f}` followed by an escaped dollar (`\$` in the snippet),
  format 2 with `${1:outer ${2:inner}}`, and format 1 with `bar($1)`, the items' edit texts are `foo(a, b)`,
  `x = f$`, `outer inner` and `bar($1)` (format 1 is left alone), each over the range the base decodes.
- `malformed_snippets_stay_literal`: format 2 items `a${1:b` and `cost: $` give `a${1:b` and `cost: $`.

Linux-OK.
