---
task_uid: "01m6j5s5jy7gfwepctwt4fb44m"
task: "T438"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "ignore_assist::ignore_assist_tests::path_completion_lists_directory_children: after 'src/' in .gitignore the menu offers src's entries, directories with a trailing slash, relative to the ignore file's directory"
    required: true
    manual: false
  - id: "C2"
    text: "ignore_assist::ignore_assist_tests::nested_ignore_file_completes_from_its_own_directory: in sub/.gitignore, typing 'b' offers sub's children not the root's"
    required: true
    manual: false
  - id: "C3"
    text: "ignore_assist::ignore_assist_tests::hover_lists_matched_files: hovering '*.log' shows the count and the first paths; a pattern matching nothing says it matches no files"
    required: true
    manual: false
  - id: "C4"
    text: "ignore_assist::ignore_assist_tests::comments_dockerignore_and_other_buffers_are_untouched: no completion or hover on a comment line, in .dockerignore, or in a Rust buffer, whose language-server answers are unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; existing lsp hover and completion tests pass"
    required: true
    manual: false
---

# Complete paths and hover match counts in ignore files (Mac T1366)

## Problem
JetBrains' .ignore plugin completes paths and shows what a pattern matches. Tenmoku gives ignore files neither.

## Seams found
- Completion: fe_gpui::CompletionProvider (crates/fe-gpui/src/completion_menu.rs) is one provider per editor; the workbench installs LspHover for all editors (crates/fe-workbench/src/lsp.rs, impl at ~2214; sync_lsp installs it). Extension snippets are merged inside that provider (crates/fe-workbench/src/extension_snippets.rs, offers_snippets) -- follow that pattern: add an ignore-path branch inside LspHover::completions rather than a second provider.
- Hover: fe_gpui::HoverProvider (crates/fe-gpui/src/hover.rs; HoverContent { text, range }) -- same LspHover impl at ~2192; add a branch returning the matches for the pattern on the hovered line.

## Change
New crates/fe-workbench/src/ignore_assist.rs and ignore_assist_tests.rs (mod lines in lib.rs); a few-line branch in the two LspHover methods in crates/fe-workbench/src/lsp.rs.
- Completion in a gitignore-language buffer (not .dockerignore: skip its first cut): after the text typed so far on the line (strip leading '!', anchor '/'), list the entries of the directory it names, relative to the ignore file's directory, files and dirs (dirs offered with trailing '/'), read with std::fs on the background executor, capped at 200, hidden entries included, .git excluded. Trigger characters: '/'.
- Hover on a non-comment pattern line: the count and first 10 paths it matches, 'matches N files', or 'matches no files'; reuse IgnoreReport::matches from check_ignore_file (crates/fe-project/src/ignore_check.rs). Cache by (buffer version, ignore-file mtime); never block the UI.
Tests: new files only, fake project dirs; completion after 'src/' lists src's children; nested file bases at its own dir; hover text for matching and non-matching lines; comments and .dockerignore give nothing; the existing lsp hover and completion tests still pass. Headless; Mac or Linux.
