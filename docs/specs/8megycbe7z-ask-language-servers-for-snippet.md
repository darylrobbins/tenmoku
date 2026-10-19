---
task_uid: "01m55ef8mgh7z07h8megycbe7z"
task: "T175"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::completion::snippet_items_are_marked passes: an item with insertTextFormat 2 has snippet: true and its textEdit newText as the source over the decoded range; insertTextFormat 1 or none is snippet: false; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::completion::items_convert_to_byte_edits passes with its pinned completionItem capability changed only from snippetSupport false to true (the one existing test this card edits)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit test tests::lsp_completion::accepting_a_snippet_completion_selects_its_first_placeholder passes: accepting the fake's foo(${1:a}, ${2:b})$0 at fo leaves foo(a, b) with a selected, editor.tab selects b, another puts the caret after ), one editor.undo restores fo; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no other existing test edited; cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Ask language servers for snippet completions and insert them with their placeholders, so call completions tab through their arguments (Mac T857)

## Problem

fe-lsp tells servers it can't take snippets (`snippet_support: Some(false)`, `crates/fe-lsp/src/server.rs:2038`;
`crates/fe-lsp/src/completion.rs:10`: "Snippets aren't advertised, so text is inserted as it is"), so
rust-analyzer's `foo(${1:a}, ${2:b})` call completions and typescript-language-server's
`completeFunctionCalls` come without their placeholders, which VS Code fills and tabs through. With fe-editor's
snippet engine in place (the card this depends on: `Snippet`, `insert_snippet`, `CompletionItem::snippet`), fe-lsp
only has to ask for snippets and say which items are snippets. Gap audit 2026-10-17 (user-approved).

## Change

1. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `snippet_support: Some(true)`. Nothing else in the
   completion capability changes (`insertReplaceSupport` stays false).
2. `crates/fe-lsp/src/completion.rs`: fe-lsp's `CompletionItem` gets `pub snippet: bool`, true when the server's
   item has `insertTextFormat: 2` (Snippet); its `edit.text` is then the snippet's source as the server sent it
   (from `textEdit.newText`, else `insertText`, else the label), decoded to bytes as today. Update the module docs'
   "Edits" bullet. Additional edits are never snippets (the protocol says so).
3. `crates/fe-workbench/src/lsp.rs` `completion_item` (`lsp.rs:1948`): copy `snippet` onto fe-editor's
   `CompletionItem`, which accepts it as a snippet.
4. Tests:
   - `crates/fe-lsp/src/tests/completion.rs` `items_convert_to_byte_edits` pins the advertised `completionItem`
     object with `"snippetSupport": false` (~line 113): change that one value to `true`; it is the only existing
     test this card edits.
   - New test in the same file, `snippet_items_are_marked`: an item with `insertTextFormat: 2` and a `textEdit`
     whose `newText` is `foo(${1:a}, ${2:b})$0`, one with `insertTextFormat: 1` and one with none: the first has
     `snippet: true` and that exact source text over the decoded range; the others `snippet: false`.
   - New test in the existing `crates/fe-workbench/src/tests/lsp_completion.rs`, with its harness:
     `accepting_a_snippet_completion_selects_its_first_placeholder`: the fake answers that snippet item at
     `fo|`; accepting it leaves `foo(a, b)` in the buffer with `a` selected; `editor.tab` selects `b`; another
     `editor.tab` puts the caret after `)`; one `editor.undo` restores `fo`.

## Implementation notes

- Deleted `crates/fe-lsp/src/tests/completion/snippet_guard.rs` (and its `mod` line): its tests pinned the stopgap `plain_snippet` expansion ("until snippet completions land"), which this card removes, so they cannot hold. Scope was widened by respec for that file.
