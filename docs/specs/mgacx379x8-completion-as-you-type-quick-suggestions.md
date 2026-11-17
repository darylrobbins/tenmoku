---
task_uid: "01m7kmed8qwz636amgacx379x8"
task: "T668"
rev: 5
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Typing a word opens the completion menu after editor.completion.delay_ms with the provider asked as Invoked; a second key within the delay cancels the pending ask; no auto popup in ordinary comments or strings by default (injected-language layering and incomplete highlights are best effort, see Known limits), in vim normal/visual mode, or with auto_trigger = false (ctrl-space and trigger characters still work): tests in crates/fe-gpui/tests/completion_auto_trigger.rs and crates/fe-workbench/src/tests/completion_as_you_type.rs"
    required: true
    manual: false
  - id: "C2"
    text: "With an auto-opened menu whose selection hasn't moved, Enter inserts a newline and closes it, Tab accepts; accept_on_enter always/never/explicit behave as the spec says (tests)"
    required: true
    manual: false
  - id: "C3"
    text: "Word completion: with no provider, plain text and config buffers offer words from open buffers of the same language (\u003e= 3 chars, not the word being typed); with a server, words follow server items and duplicates are dropped; editor.completion.words off/current_buffer/open_buffers honoured (buffers whose language is still loading are best effort, see Known limits) (tests in crates/fe-editor/tests/word_completion.rs and crates/fe-workbench/src/tests/completion_words.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "The new [editor.completion] settings are in settings.rs, settings.schema.json, settings-template.toml and l10n, reload live; existing completion, snippet and vim \u003cC-n\u003e/\u003cC-p\u003e tests pass; the decision file records the defaults"
    required: true
    manual: false
---

# Completion as you type (quick suggestions after a short debounce, quiet in comments/strings, Enter not stolen) and word completion from open buffers when no language server answers (Mac T1918)

The user (2026-11-17): "Do we even have auto-complete today?" — the lead's answer: the completion menu exists (crates/fe-gpui/src/completion_menu.rs over fe-lsp completion and fe-editor's CompletionSession, plus user/extension snippets and vim's <C-n>/<C-p>), but it opens only on ctrl-space or a server trigger character (`completion_typed` checks trigger characters only), and there is nothing without a language server. The user: "Sounds good" to the fix below. Priority card.

## 1. Quick suggestions (as you type)
- After a word character is typed and the word at the caret has at least `editor.completion.min_word_chars` (default 1) characters, ask the provider (CompletionTrigger::Invoked or a new TypedWord kind, as LSP's `triggerKind = Invoked` with no trigger character) after `editor.completion.delay_ms` (default 50 ms, debounced: a newer keystroke cancels the pending ask). Reuse the existing narrowing: once the menu is open, typing keeps filtering as today.
- Not in comments or strings by default (use the syntax tree's capture at the caret; `editor.completion.in_comments` / `in_strings` default false), not in vim normal/visual mode, not while a snippet's tabstop is active unless the provider is asked explicitly, not in multi-caret edits beyond the primary caret.
- Don't steal keys typed at speed: while the menu opened by itself and the selection hasn't been moved, Enter inserts a newline as usual and closes the menu; Tab accepts (setting `editor.completion.accept_on_enter`: "explicit" default = only after the selection moved or the menu was opened by ctrl-space, "always", "never"). Escape closes as today.
- `editor.completion.auto_trigger` (default true) turns it off; ctrl-space and trigger characters keep working either way.

## 2. Words from open buffers
- A word source: identifiers (by the language's word characters, at least 3 chars, not the word being typed) from the current buffer and other open buffers of the same language, capped (e.g. 10k distinct words, scanned off the main thread and kept fresh incrementally or per edit batch).
- With a language server, word items come after the server's items and only for words the server didn't offer; without a provider (no server, plain text, config files), the word source alone backs the menu, so quick suggestions work everywhere. Kind shown as "Text", detail "Word in <file>" for other files.
- `editor.completion.words` = "open_buffers" (default) | "current_buffer" | "off".

## Settings, docs, tests
- New `[editor.completion]` keys in fe-config with schema, settings-template and l10n; live reload applies.
- Headless tests with the fake LSP: typing a word opens the menu after the delay; a fast second key cancels the pending ask; no auto popup in a comment/string by default; Enter after an auto-opened menu inserts a newline; Tab accepts; auto_trigger=false keeps ctrl-space working; plain text with no server offers buffer words; server items rank before words and duplicates are dropped; vim normal mode never auto-opens.
- docs/decisions entry for the defaults and the key rules (VS Code's quickSuggestions/acceptSuggestionOnEnter and JetBrains' behaviour as the reference).
## Known limits (lead respec after review round 2)
- The comment/string check reads the highlight captures at the typed character. Where those captures are incomplete or ambiguous (an injected language's captures layered over a host comment or string, a highlight query that ran out of time, an injected grammar still compiling), detection is best effort: the card waits briefly for complete highlights and otherwise asks nothing, and it doesn't promise a correct answer for every injection layering. Hardening that is a follow-up (review F8/F9).
- The same holds wherever the syntax context isn't known yet: a buffer whose language or grammar is still loading, a tree that hasn't caught up, or highlights still computing. The card waits briefly and otherwise asks nothing, and its word source keeps a loading buffer's words apart from other buffers' (review round 3 F1, fixed). Beyond that, how suggestions and words behave while a language loads is best effort, not a criterion.
- Files too big to parse whole (only a window's tree) never open the menu by themselves; ctrl-space and trigger characters still work there.

## Implementation notes

- Design and defaults: `docs/decisions/mgacx379x8-completion-as-you-type.md`.
- fe-gpui: `completion_menu/auto.rs` (quick suggestions, the `enter` rule),
  `completion_menu/words.rs` (open editors' words, scanned off the main thread
  and kept per buffer version); `completion_menu.rs` asks both and merges.
  view.rs gains a field, the registration in `EditorView::new` and one line in
  `set_wrap_settings`, through which new editors and reloads (`apply_settings`)
  take `[editor.completion]`.
- fe-editor: `word_completion.rs` (scan, `word_at`, `word_items`,
  `merge_words`). `CompletionTrigger` is unchanged: quick suggestions ask as
  `Invoked`, so fe-lsp and the workbench's provider need no change, and the
  workbench needs no `completion_words.rs`.
- `enter` can't be told from `tab` in `completion.accept` (one command, both
  bound in fastedit.toml, matched by kiln's interceptor before the menu's), so
  the command holds the accept and the menu's interceptor settles it for the
  same keystroke; see the decision's key rules.
- A trigger character counts as explicit for `accept_on_enter = "explicit"`, so
  `enter` still accepts member completion after `.`.
- Scope added by respec: the qps-ploc template fixture (regenerated from the
  template), the Settings tab's labels (fe-workbench settings.ftl) and fe-gpui's
  catalogue (the word items' kind and detail).
