---
task_uid: "01m569n1v7ts44kt2k8vc0173j"
task: "T197"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test view::completion_tests::keyword_menu_tests::ctrl_n_shows_the_candidates (new crates/fe-gpui/src/view/keyword_menu_tests.rs, declared in crates/fe-gpui/src/view/completion_tests.rs) passes: after if\u003cC-n\u003e in a real view the menu paints the candidates with the first selected, \u003cC-n\u003e moves the selection, typing a space closes it, and the CompletionProvider is never asked."
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::completion_tests::keyword_menu_tests::menu_keys_stay_vims passes: with the keyword menu shown, escape leaves insert mode keeping the match and enter breaks the line, as without the menu, and an open language-server menu closes when keyword completion starts."
    required: true
    manual: false
---

# Show vim's <C-n> and <C-p> keyword candidates in the editor's completion menu while they cycle, as nvim's popup menu does (Mac T884)

## Problem

The keyword-completion card makes vim's `<C-n>`/`<C-p>` insert and cycle matches, as nvim does, but nothing
shows the list: nvim (`completeopt=menu,popup`) shows a popup menu of the candidates with the current one
selected. fastedit already has a completion menu for language servers (`crates/fe-gpui/src/completion_menu.rs`,
over fe-editor's `CompletionSession`). The user approved keyword completion in the "Vim parity" track on
2026-10-17.

## Change

While `VimState::completion()` is `Some`, the editor view shows its candidates in the completion menu's rows
(label only, no detail, kind or documentation), the selected one highlighted, placed at the typed text's
start as the menu places a word; nothing is asked of the `CompletionProvider`. It closes when completion
ends. The `completion` key context is not set for it, so `enter`, `tab` and `escape` stay vim's insert-mode
keys, and `<C-n>`/`<C-p>`/`<C-e>`/`<C-y>` keep going to vim. A language server's menu, if open, closes when
keyword completion starts. Work in `crates/fe-gpui/src/completion_menu.rs` and `crates/fe-gpui/src/vim.rs`.

Test in a new `crates/fe-gpui/src/view/keyword_menu_tests.rs`, declared in the existing
`crates/fe-gpui/src/view/completion_tests.rs` by `#[path = "keyword_menu_tests.rs"] mod keyword_menu_tests;`,
using its `Painted` capture.
