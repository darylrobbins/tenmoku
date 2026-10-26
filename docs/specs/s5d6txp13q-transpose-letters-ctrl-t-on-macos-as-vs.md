---
task_uid: "01m5vxsa6t5ade32s5d6txp13q"
task: "T1185"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "transpose_tests::swaps_around_the_caret: editor.transpose_letters with the caret between a and b in \"ab\" gives \"ba\" with the caret after both; mid-row \"x|yz\" gives \"yxz\" with the caret after the pair; one undo step"
    required: true
    manual: false
  - id: "C2"
    text: "transpose_tests::row_edges: at the end of a row it swaps the last two characters (caret stays at the end); at column 0 and on a row of one character it does nothing and reports no change; the row break is never moved"
    required: true
    manual: false
  - id: "C3"
    text: "transpose_tests::graphemes_and_carets: an emoji ZWJ sequence or a letter with a combining mark counts as one letter; several carets each transpose, on separate rows and on one row without overlapping; a non-empty selection is left alone"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap transpose_key: ctrl-t resolves to editor.transpose_letters on macOS in the editor context with vim off; with vim on (any mode) ctrl-t keeps its existing vim bindings on every platform (fe-gpui vim_indent_keys_tests and fe-keymap vim_insert_keys pass unchanged); the Linux keymap gets no ctrl-t transpose binding; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Transpose Letters (ctrl-t on macOS), as VS Code's editor.action.transposeLetters

New module transpose.rs with editor.transpose_letters ("Transpose Letters"), no other fe-editor file but the shared registration points. Behaviour to match, from VS Code's TransposeLettersAction (verify against its source and note any divergence in the module doc): for each empty selection that is not at column 0, swap the grapheme before the caret with the one after it and put the caret after the pair; at the row end swap the two before it; nothing at column 0 or on a row with fewer than two graphemes. Selections with text are skipped. VS Code works in UTF-16 units; use graphemes. One transaction for all carets, built as edits over disjoint ranges (drop a caret whose pair overlaps the previous one).

Key: macOS only (ctrl-t), context "editor && !vim.enabled" (lead decision, rev 2: with vim on, ctrl-t stays vim's insert-mode indent on every platform, as in vim and VS Code's vim extension; vim_indent_keys_tests and vim_insert_keys stay unchanged). Off macOS ctrl-t is vim insert's (fastedit.toml ~1288) so it gets no Linux binding, and the macOS-only list in fe-workbench tests/default_keymaps.rs gains the id. Headless tests only, using the case_transform_tests.rs dispatch helper.

## Implementation notes

- The binding's context is `editor && !vim.enabled`, not join_lines' context:
  vim insert/replace ctrl-t indents the row on every platform (`vim_indent_keys_tests`,
  `vim_insert_keys` assert it), so the key stays vim's whenever vim is enabled.
- Divergence from VS Code is in the module doc of `transpose.rs`.
