---
task_uid: "01m5v8ywyak7vpnx34sp54rdys"
task: "T1177"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "indent_convert_tests::to_spaces: with tab size 4, editor.convert_indentation_to_spaces turns the leading tabs of every row into 4 spaces each (mixed '\\t  x' gives 6 spaces), leaves tabs after the first non-blank alone, sets indent_style to Spaces(4), is one undo step and keeps selections on the same text"
    required: true
    manual: false
  - id: "C2"
    text: "indent_convert_tests::to_tabs: editor.convert_indentation_to_tabs turns each full tab-size run of leading spaces into a tab, keeps the remainder as spaces, sets IndentStyle::Tabs; rows inside multi-line strings are converted too (as VS Code, it is text-only)"
    required: true
    manual: false
  - id: "C3"
    text: "indent_convert_tests::detect: editor.detect_indentation applies detect_indent_style to the buffer, overriding an .editorconfig or setting-derived style, and with too few indented rows leaves the style unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "indent_convert_tests::empty_edit_is_unchanged: a buffer with nothing to convert reports no change and creates no undo step; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-editor tests::commands); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Convert Indentation to Spaces / Tabs and Detect Indentation from Content, as VS Code's editor.action.indentationToSpaces, indentationToTabs and detectIndentation

Indent style detection exists (indent_style.rs: detect_indent_style) and runs at open, and the status bar shows 'Spaces: N' / 'Tab Size: N', but there is no way to convert a file's indentation or re-detect. New module indent_convert.rs; no keybindings (palette only, as in VS Code). The Indent Using Spaces/Tabs width picker and a status-bar click are a follow-up card.
