---
task_uid: "01m5jyqvfwxh3wj60e30kfp87r"
task: "T287"
rev: 7
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extension_snippets::a_manifest_declares_snippets: an extension with [[contributes.snippets]] prefix, body (string and array forms), description, languages loads; an empty prefix or body, or an unparsable body, is a warning and drops only that snippet; a manifest needing engine fastedit = \"1.1\" loads and one needing \"1.0\" still does (crates/fastedit/tests/extension_snippets.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "extension_snippets_tests::matching_snippets_are_offered_as_snippet_items: in a buffer of the snippet's language, typing the prefix offers an item with snippet: true, kind snippet and the extension's name as detail, whose edit replaces the word before the caret; a snippet for another language isn't offered, and one with no languages is offered everywhere; a server's trigger character offers the matching snippets only when it is a word character (`_`, which the word before the caret then holds: extension_snippets_tests::a_word_character_trigger_offers_the_snippets_its_word_matches), and a punctuation one (`.`, `:`, `\u003e`) offers none (extension_snippets_tests::a_punctuation_trigger_offers_no_snippets). Known limit: a prefix that starts with punctuation, such as `.log`, isn't offered after its trigger; asking for completions (ctrl-space) there offers it"
    required: true
    manual: false
  - id: "C3"
    text: "extension_snippets_tests::accepting_one_expands_with_tabstops: accepting the item inserts the body indented to the row and Tab walks its tabstops to $0 (the T856 session)"
    required: true
    manual: false
  - id: "C4"
    text: "extension_snippets_tests::they_work_without_a_language_server: with no server for the language, the snippet items still appear, and with a server they sort after its items"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench -p fastedit and cargo clippy -p fe-workbench -p fastedit --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Take [[contributes.snippets]] from extensions as completion items that expand through the snippet engine (extensions record 9 and 2) (Mac T1040)

## Problem

Extensions record §9 and §2: `snippets` becomes a fastedit contribution kind once the snippet engine
exists. It now does: `fe_editor::snippet` (T856, `crates/fe-editor/src/snippet.rs`: parse, insert with
placeholders, tabstops) and LSP snippet completions (T857). Extensions can't contribute snippets yet.

## Change

`[[contributes.snippets]]` in `extension.toml`:

```toml
[[contributes.snippets]]
prefix = "fn"
body = ["fn ${1:name}(${2}) {", "\t$0", "}"]   # a string, or an array of lines joined with \n
description = "Function"
languages = ["rust"]                           # fastedit's language names; none = every language
```

- `crates/fastedit/src/config.rs`: `ExtensionSnippet` (serde + schemars, `deny_unknown_fields` like
  `ExtensionGrammar`), registered as kind `snippets`; the extension API becomes 1.1 (record §2:
  each minor adds), so a manifest needing `1.1` loads and a `1.0` one still loads. An empty `prefix`
  or `body` is a warning and drops that snippet; its body is parsed by `fe_editor::snippet::Snippet`
  and an unparsable one is dropped the same way. The snippets, with their extension id and name, go
  to `fe_workbench::set_extension_snippets(cx, snippets)`.
- New `crates/fe-workbench/src/extension_snippets.rs` (with `#[cfg(test)] mod
  extension_snippets_tests;` and the new file `extension_snippets_tests.rs`) and `lsp.rs`: the
  completion provider adds a snippet item (`snippet: true`, kind "snippet", detail the extension's
  name, edit replacing the word before the caret) for each extension snippet whose language matches the
  buffer's and whose prefix matches the typed word, so Tab walks its tabstops. They appear in
  languages with no language server running (a grammar extension with snippets and no server works),
  and sort after the server's own items. Snippets from `lsp.rs`'s `completions` are the only
  surface; no new command or menu.
- `crates/fe-workbench/src/lib.rs` exports `set_extension_snippets`.

## Out of scope

User snippet files; vim-specific behaviour beyond the engine's; snippet variables beyond what the
engine supports; workspace snippets (the workspace-extensions card passes `snippets` through this).

Lend note (helium lead): spec, scope and criteria replaced with the Mac's full JSON (y51); earlier copies were truncated.

## Implementation notes

- Scope gained `crates/fastedit/tests/extension_snippets.rs` (respec, revision 3): C1's test loads
  manifests through `kiln::init` as the grammar and server integration tests do. It loads a 1.1
  manifest and a 1.0 one, and refuses a 1.2 one. It drops only the snippets with an empty prefix or
  body or an unparsable body; the same extension's others load.
- Unparsable: `fe_editor::snippet::Snippet::parse` never fails, since anything malformed becomes
  literal text, and it doesn't say what it fell back on. fastedit doesn't depend on fe-editor, and
  `snippet.rs` is out of scope. So `fe_workbench::snippet_body_error` checks a body against the
  LSP snippet grammar, following the engine's rules for choices, transforms, `u32` tabstop numbers
  and its `MAX_DEPTH`. It names what the engine would insert as literal text: a `${` never closed,
  a `${` with no tabstop number or variable, an unclosed choice or transform, or nesting too deep.
  `config.rs` warns and skips such a snippet, and `set_extension_snippets` drops it for any other
  host. A body of tabstops alone (`$1$0`) is kept and starts a session (review round 1, F3).
- fe-gpui's `CompletionProvider` gets only a buffer id. The provider (`LspHover`) holds the
  workbench and an `AsyncApp`, and its future reads the editor showing the buffer at the asked
  version or a later one on its first poll. That happens before awaiting the server, so with no
  manager, no root, or no server for the language, snippets are still offered. If the buffer has
  moved on, the items are still worked out in the request's offsets, unless an edit since
  (`edits_since`) started before the caret. Typing at the caret keeps them (round 1, F2). Trigger characters
  (review F1, three rounds; rule set by the lead): one function, `offers_snippets`, decides.
  Requests asked for (`ctrl-space`) or asked again for an incomplete list offer every matching
  snippet. A server's trigger character offers them only when it's a word character (`_`), which
  the word before the caret then holds, so `_log` follows the server's items after `_` and typing
  narrows it. A punctuation trigger (`.`, `:`, `>`) offers none.
- Known limit (C2, respec revision 7): a prefix that starts with punctuation, such as `.log`,
  isn't offered after its own trigger character `.`. Asking for completions there (`ctrl-space`
  at `.lo`) offers it, replacing `.lo`.
- An item replaces the identifier before the caret. It replaces more of the row when that much
  starts the prefix, case aside (`#Reg` for `#region`).
- Server first (round 1, F1; C4 in the full criteria): the session ranked by fuzzy score before
  anything else, so a snippet that matched better came first. Scope gained
  `crates/fe-editor/src/completion.rs` (revision 4) for a `CompletionItem::group`. The lead's
  respec to the Mac's full scope (revision 5) dropped it, and revision 6 restores it. It is ranked before the match, so snippets
  (group 1) follow every server item (group 0), each group ranked by match as before.
- Carried T272 F1 (`Document.root` in lsp.rs) is not addressed. It needs an fe-lsp accessor for a
  document's marker root (fe-lsp is outside this card), and this card touches lsp.rs only in the
  completion provider.
