---
task_uid: "01m6j5s4khx2y6m1ejwf6j1hcc"
task: "T435"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "gitignore_highlights::directory_and_anchor_slashes_are_distinct: in 'build/', '/dist' and 'a/b' the trailing and leading slashes get a different highlight from the inner slash, and the names get a non-plain highlight"
    required: true
    manual: false
  - id: "C2"
    text: "gitignore_highlights::negated_line_is_styled_as_a_whole: in '!keep.txt' the '!' and the name both carry the negation styling, which 'keep.txt' on a plain line lacks"
    required: true
    manual: false
  - id: "C3"
    text: "gitignore_highlights::wildcards_classes_and_escapes_are_coloured: '**', '*', '?', '[a-z]' (bracket, range dash) and '\\#x' each get a highlight and '**' differs from '*'"
    required: true
    manual: false
  - id: "C4"
    text: "gitignore_highlights::comments_and_dockerignore_names_still_work: a '# note' line stays comment and a file named .dockerignore resolves to the gitignore language"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax and cargo clippy -p fe-syntax --all-targets -- -D warnings pass; config_file_names and git_file_names still pass"
    required: true
    manual: false
---

# Highlight ignore files richly: directory and anchored patterns, globstar, classes, escapes, negated lines (Mac T1363)

## Problem
The user (2026-11-03, screenshot of .gitignore): only comments, '!' and '*' are coloured. The vendored grammar (vendor/tree-sitter-gitignore, query in vendor/tree-sitter-gitignore/queries/highlights.scm, wired by the gitignore Def in crates/fe-syntax/src/language.rs) leaves pattern_char uncaptured, so names, directory slashes and anchors look like plain text. The Def serves .gitignore, .dockerignore, .npmignore, .prettierignore, .eslintignore, .ignore and similar; one query serves all.

## Change
- Write crates/fe-syntax/queries/gitignore.scm (grammar node names from vendor/tree-sitter-gitignore/grammar.js; only capture names highlight.rs maps). Keep what the vendored query does, and add: a whole-line negation (a pattern with a negation child) styled as a unit so '!' lines read differently from ignore lines (capture the pattern's chars on negated lines with a distinct name, e.g. @string.special for ordinary, @variable for negated chars; decide and document in the query); the trailing directory_flag separator and the leading relative_flag separator as distinct captures from inner separators (use the grammar's field names); pattern_char runs as @string.special.path-style text if mapped, else @string; '**' (wildcard_chars_allow_slash) distinct from '*' and '?'; bracket classes and ranges; escapes.
- Point the gitignore Def's highlights at include_str!("../queries/gitignore.scm") in language.rs. Do not edit the vendored crate.
Tests in the new file crates/fe-syntax/tests/gitignore_highlights.rs (#![cfg(feature = "gitignore")], spans() helper as in rust_doc_comment_colours.rs). Headless; Mac or Linux.
