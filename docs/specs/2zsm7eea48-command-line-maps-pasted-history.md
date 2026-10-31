---
task_uid: "01m5ztyvaw3e9g4b2zsm7eea48"
task: "T253"
rev: 6
type: "bugfix"
complexity: ""
criteria:
  - id: "C1"
    text: "With cnoremap a b, pasting s/a/x/ on the vim command line leaves s/a/x/"
    required: true
    manual: false
  - id: "C2"
    text: "A history recall (Up/Down) or Tab completion applied while a cmap prefix is pending lands before later mapped keys, including a mapped \u003cCR\u003e"
    required: true
    manual: false
  - id: "C3"
    text: "IME composition text that is committed in ordinary typing is replayed in order as unmapped input. Known limits, tracked by T258 (T253 review round 2 F1/F2/F4): a composition started or committed while a held exact-and-prefix mapping resolves to Enter, a literal-provenance collision with U+F8FF, and replacement ranges across settlement edits that delete and reinsert text."
    required: true
    manual: false
---

# Command-line maps: pasted, history-recalled and completed text is mapped input

Follow-up to T208 (vim cnoremap/cmap on the status bar's command line), whose review findings F1 (high), F3, F5 and F8 the user waived on 2026-10-27 so it could land with known limits.

Fix F1: on the vim command line, text that does not come from typed keys is unmapped input and must replay in order with the typed keys. That covers pasted text, IME composition, history recall (Up/Down) and Tab completion.

Repro from the review:
- With `cnoremap a b`, pasting `s/a/x/` must leave `s/a/x/`; today it becomes `s/b/x/`.
- A history recall or a completion that lands while a cmap prefix is pending must be applied before any later mapped key, including a mapped `<CR>`.

Fix: record every such edit (the TextInput changes that never go through the typing hook, including the silent history and completion updates that raise no Changed event) with unmapped provenance, synchronised before the later mapping keys are resolved and replayed.

Regression test: a new file next to crates/fe-workbench/src/vim_line/remap_tests.rs (for example crates/fe-workbench/src/vim_line/unmapped_input_tests.rs). It must fail on the T208 head.

Known limits left alone (T208 merge note, no cards): F3 (Backspace cancel and host-command wait lose continuation keys), F5 (live versus replay special keys; eligibility decided by key shape), F8 (a <Cmd> continuation edits text it cannot see).

Also in scope (review F9, medium, found in T208 rev 5 review): a held cmdline prefix that resolves while an IME composition is active (on timeout, blur or a bypassed input) goes through the composition-clearing input path, so composition input can be dropped or reordered. Resolve held prefixes before bypassed platform input and before a composition starts, and keep composition text in order.

Also in scope (T208 rev 6 review, medium F9b and F10): (F9b) with a cmap prefix held (for example j), multi-character non-IME platform input (xy) must apply in order before the held prefix resolves, not give xyj after the timeout (kiln-ui text_input.rs near line 333); (F10) RHS keys the live line ignores, such as <Left> in cnoremap zz <Left>s/a/b/<CR>, must behave the same live and in macro replay (vim.rs near line 2947). Add a regression test for each.

## Implementation notes

- Text no typed key made (paste, IME commit, history recall, Tab completion) is recorded as a private-use marker key (`LITERAL_NEXT`) + char; a macro replay reads the char after the marker on the command line unmapped (`vim_line_literal`). The marker is not `<C-v>`, so no map matches it and a map's own `<C-v>` stays an ignored key, live and in replay. History and completion now call `sync_line_edit` after `set_text`, which raises no `Changed`.
- `TextInput::set_before_platform_input`: asked before any platform edit the typing filter didn't take (multi-char commit, explicit-range replacement or deletion, composition start). The bar resolves a held cmap prefix and the hook returns the edits (insert / backspace) which the input makes first; a key that ends the line swallows the platform edit and the bar replays it after the line's keys. An insertion at the end stays at the end; other ranges keep their text.
- F10: replay drops line keys the live line ignores (`<Left>`), instead of failing.
