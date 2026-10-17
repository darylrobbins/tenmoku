---
task_uid: "01m55sbhgz25bbb59nczj6ebht"
task: "T164"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::completion::resolve_edits::resolve_keeps_the_additional_edits_it_resolves passes: an item listed with no additionalTextEdits, resolved after the buffer was edited, has the resolve reply's additionalTextEdits (an import at row 0 and one after an emoji, UTF-16 session) as byte ranges in the request version's text, and its main edit unchanged; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::completion::resolve_edits::a_resolve_without_edits_keeps_the_listed_ones passes: a resolve reply with no additionalTextEdits leaves the item's listed additional_edits as they were"
    required: true
    manual: false
  - id: "C3"
    text: "The initialize params advertise completionItem.resolveSupport.properties [documentation, detail, additionalTextEdits]; fe-lsp unit test tests::completion::items_convert_to_byte_edits passes with only that pinned list changed, the one existing test this card edits; the completion.rs module docs' Resolve bullet says what is resolved"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Keep the additionalTextEdits a server sends when it resolves a completion item, so accepting a TypeScript completion adds its auto-import (Mac T913)

## Problem

LSP gap audit (2026-10-17, main c7b6a04a; `~/.local/share/fastedit-lead/lsp-gap-audit.md` §4): "Auto-imports
likely lost on TS/JS". Re-verified at main 40608138 (no fe-lsp change since c7b6a04a):

- `LspManager::resolve` (`crates/fe-lsp/src/completion.rs:143-188`) copies only `documentation` and `detail`
  from the `completionItem/resolve` reply (`:175-180`) and then stores the reply as `raw` (`:181`). The
  reply's `additionalTextEdits` are thrown away.
- An item's `additional_edits` are decoded once, at list time, in `convert` (`:248-253`). A server that
  computes them lazily never has them applied. typescript-language-server computes auto-import edits in
  resolve (from tsserver's `completionEntryDetails` code actions); VS Code applies them. (Not re-checked
  against a live server: the client-side drop is certain from the code.)
- `client_capabilities()` (`crates/fe-lsp/src/server.rs:2040-2042`) advertises
  `resolveSupport.properties: ["documentation", "detail"]`, so a server that honours the 3.16 rule may
  hold the edits back. VS Code advertises `additionalTextEdits` there too.

The rest of the path already carries them: fe-workbench's `completion_item` (`crates/fe-workbench/src/lsp.rs:1969`)
copies `additional_edits` from the resolved item, fe-gpui's menu swaps the resolved item in
(`set_item`, `crates/fe-gpui/src/completion_menu.rs:368`), and `CompletionSession::accept`
(`crates/fe-editor/src/completion.rs:306`) applies them in one transaction against the request's version.

## Change

1. `crates/fe-lsp/src/completion.rs`:
   - `CompletionItem` keeps, in private fields, the text it was decoded against (the request version's
     `at.document.sent.text`, as `convert` uses) and the session's `PositionEncoding`.
   - `resolve`: when the reply has `additionalTextEdits`, they replace `additional_edits`, decoded against
     that kept text exactly as `convert` decodes them (`from_lsp` both ends, ordered). A reply without them
     leaves the listed `additional_edits` as they were. `edit` (the main edit) never changes on resolve, as
     the protocol says.
   - Update the module docs' "Resolve" bullet.
2. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `resolveSupport.properties` becomes
   `["documentation", "detail", "additionalTextEdits"]`.
3. Tests:
   - `crates/fe-lsp/src/tests/completion.rs` `items_convert_to_byte_edits` pins the advertised
     `completionItem` object: add `"additionalTextEdits"` to its `resolveSupport.properties`. That is the
     only change to an existing test. Declare `mod resolve_edits;` in the same file.
   - New `crates/fe-lsp/src/tests/completion/resolve_edits.rs` (the `diagnostics/` and `navigation/`
     subdirectories are the precedent), built like `resolve_triggers_and_timeouts` with a resolving
     UTF-16 server:
     - `resolve_keeps_the_additional_edits_it_resolves`: the text has an emoji before the completion on its
       row and an `import` row above. The list item has no `additionalTextEdits`. The buffer is then edited
       (synced to version 1) before resolving. The resolve reply carries a `textEdit` identical to the
       list's and an `additionalTextEdits` entry inserting `import { foo } from "./foo";\n` at row 0 and
       one replacing a range after the emoji. `resolved.additional_edits` equals both, as byte ranges in
       the *request's* version text; `resolved.edit == item.edit`.
     - `a_resolve_without_edits_keeps_the_listed_ones`: a list item with one `additionalTextEdits` entry and
       a resolve reply with none: `resolved.additional_edits == item.additional_edits`.

Not in this card (follow-ups, other seams): accepting before the resolve lands still applies the item
without the edits (fe-gpui accepts the item as it is, `completion_menu.rs:319`); the item's `command` is not
run (accept → workbench → `workspace/executeCommand`); `commitCharacters`.

Order: T850 (fe-lsp helpers, inbox) and T857 (snippet completions, inbox, lent to helium) edit
`completion.rs`, `server.rs`' capabilities and T857 the same pinned test. This card is meant to land first;
T857's C2 ("pinned completionItem changed only from snippetSupport false to true") still holds against a
base with this card in. Tell the T857 worker that the pinned `resolveSupport` now lists three properties.
