---
task_uid: "01m57yae0m3m0dexdtve0s8vrv"
task: "T195"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Brackets at depth 1,2,3 take bracket_1..3 colours, cycling; a wide node with a named preamble before its opening delimiter (a Rust macro_rules! definition, a C# switch expression) may report one less enclosing depth (documented known limit)"
    required: true
    manual: false
  - id: "C2"
    text: "Brackets in strings and comments are not coloured"
    required: true
    manual: false
  - id: "C3"
    text: "bracket_colors.rs proves both"
    required: true
    manual: false
---

# Colour nested bracket pairs by depth in the editor, behind editor.bracket_pair_colors (Mac T988)

## Problem
VS Code Parity Notes (grammar): VS Code colours nested bracket pairs; fastedit only outlines the pair at the caret (bracket_rects, crates/fe-gpui/src/element.rs; pairs from fe-syntax brackets.rs).
## Change
Colour bracket characters by nesting depth (cycle of three theme colours bracket_1..3), behind editor.bracket_pair_colors, using fe-syntax's pairing and skipping strings and comments.

## Implementation notes
- `fe_syntax::bracket_depths(syntax, range)` (brackets.rs) walks only the part of the tree touching the
  visible range and pairs each walked node's bracket children as `bracket_pair` does: among siblings, so
  brackets in strings and comments (not leaves) never count, nor do unpaired, `MISSING` or `ERROR`-node
  brackets. Every node's bracket children are paired exactly. Pairing a node with more than 4,096
  children (a long JSON array, a Go `const (` group, a big `macro_rules!` or C# switch) is cached on the
  `Syntax` by node id and dropped when the tree is edited or replaced, so it's paid once a parse: about
  30 ms for a 200,000-element array in a release build, against 0.06 ms a call once cached. Earlier rounds
  paired only a wide node's end brackets; each widening of that shortcut had another counterexample
  (Go keywords and comments, then named preambles: review F2, F3), so it's gone, along with the C1 rev 2
  known limit (T204).
- Tree-sitter's seek to the first child at a byte returns nothing in some gaps between children (Go's
  blank lines); the walk then steps to the first child ending past the range start (review F4).
- `bracket_depths` is empty while the tree needs a reparse (`Syntax::needs_parse`), as `bracket_rects`
  is. An edited tree's leaves are moved, not re-lexed, so quotes typed around brackets would leave them
  coloured, and checking a leaf's byte against the text doesn't catch that (review F1, rounds 1 and 2).
  The view reparses within the frame when it can (1 ms), so in practice only a slow reparse of a big file
  shows brackets uncoloured, until the background parse lands.
- `styled_highlights` (element.rs) lays the depth colours over the syntax highlights, cutting the
  highlight spans where a bracket sits. Depth 1 (outermost) takes `bracket_1`, then round in threes.
- `bracket_1..3` are new `[editor]` theme keys, set in all three bundled themes (VS Code's defaults in
  Dark and Light, Ember's own warm ones). `EditorTheme::brackets` holds them; `set_metrics` copies the
  setting as it does `indent_guides`.
- `editor.bracket_pair_colors` defaults to off, so existing screenshot goldens don't change.
- `EditorView::drawn_color(offset)` is public so the integration test can read what the last frame drew.
- Interpolation braces (Python f-strings) are code delimiters in the tree and are coloured.
