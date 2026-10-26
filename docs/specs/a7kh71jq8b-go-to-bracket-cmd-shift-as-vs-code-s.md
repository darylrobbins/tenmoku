---
task_uid: "01m5tygyva6atqh7a7kh71jq8b"
task: "T351"
rev: 5
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "enclosing_bracket::innermost: for 'f(a[1], {x: 2})' enclosing_bracket_pair at the offset of '2' returns the braces, at 'a' the parentheses, at the top level None; brackets in strings and comments are ignored as in bracket_pair"
    required: true
    manual: false
  - id: "C2"
    text: "go_to_bracket_tests::partner: with the caret just after '(' editor.go_to_bracket moves to the matching ')' position and back again on a second run; with several selections each moves"
    required: true
    manual: false
  - id: "C3"
    text: "go_to_bracket_tests::enclosing: a caret inside 'foo(bar, baz)' not next to a bracket jumps to the closing ')' of the innermost pair; at top level or with a stale tree nothing moves"
    required: true
    manual: false
  - id: "C4"
    text: "go_to_bracket_tests::jump_list: after the jump nav.back returns the caret to where it started"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap go_to_bracket_key: secondary-shift-\\ resolves to editor.go_to_bracket in the editor context and pane.split_right on secondary-\\ is unchanged; catalog tests updated; nextest -p fe-syntax -p fe-gpui -p fe-keymap -p fe-workbench and clippy pass"
    required: true
    manual: false
---

# Go to Bracket (cmd-shift-\), as VS Code's editor.action.jumpToBracket (Mac T1157)

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Go to Bracket (editor.action.jumpToBracket, cmd-shift-\ on macOS, ctrl-shift-\ elsewhere) is missing. Verified: the editor outlines a caret's bracket pair (`fe_syntax::bracket_pair`, used at `crates/fe-gpui/src/element.rs:2461`) but has no command to jump to the partner; only fe-vim's `%` (`crates/fe-vim/src/motion.rs`) does, and only with vim on.

## Current state
- `crates/fe-syntax/src/brackets.rs:36` `bracket_pair(syntax, offset)` finds the pair when `offset` is next to a bracket (the char at or before it); nothing finds the pair enclosing an arbitrary offset.
- fe-editor does not depend on fe-syntax; the view owns the parsed tree (`EditorView.syntax`, element.rs:2451-2461 shows the version/needs_parse guards), so the command lives in fe-gpui. View-level editor commands are in `crates/fe-gpui/src/commands.rs`.
- `secondary-\` is `pane.split_right` (fastedit.toml:736) with context `workbench`; `secondary-shift-\` is unbound.

## Change
- fe-syntax `brackets.rs`: `enclosing_bracket_pair(syntax, offset) -> Option<BracketPair>`, the innermost pair whose open comes at or before and close after `offset`, with the same error-node and 4,096-sibling rules as `bracket_pair`.
- fe-gpui: command `editor.go_to_bracket` ("Go to Bracket"), as VS Code's jumpToBracket: caret next to a bracket moves to just after its partner's position as `bracket_pair` defines (opening <-> closing, the caret ending on the same side the partner's char starts); otherwise it jumps to the closing bracket of the innermost enclosing pair; nothing if there is none or the tree is stale. Every selection moves independently; selections collapse to carets. Records a jump-list entry so back/forward (`nav.back`) return.
- Keymap: `secondary-shift-\` in the editor context (vim-enabled normal mode keeps `%`; the binding applies like the other editor-context ones). No menu entry besides Go > "Go to Bracket" if the Go menu lists navigation commands (VS Code has it there).

## Tests
New `crates/fe-syntax/tests/enclosing_bracket.rs`, `crates/fe-gpui/src/go_to_bracket_tests.rs` (headless view tests with Rust/JSON text), `crates/fe-keymap/tests/go_to_bracket_key.rs`; catalog tests updated.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y68).
