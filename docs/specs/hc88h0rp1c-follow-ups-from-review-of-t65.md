---
task_uid: "01m48qeb7eayr1ynhc88h0rp1c"
task: "T96"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T65

Findings of the review of T65 that don't block the merge (below merge.block_severity):

- F9 medium crates/fe-project/src/search.rs:85: Standalone carriage returns remain unsearchable with valid regexes that disable dot-all, such as (?-s:a.b) or (?-s)a.b on a\rb\n. The file-level matcher retains CRLF mode, so these inline flags make dot exclude carriage returns despite dot_matches_new_line(true); the line matcher would accept the match, but never receives the line.
  fix: Make file selection conservative for supported inline flags, or evaluate the canonical matcher on terminator-stripped lines. Add scoped and global (?-s) regressions for embedded and unterminated carriage returns.
  instances: crates/fe-project/src/search.rs:85

- F10 medium crates/fe-project/src/search.rs:359: Removing the searcher's CRLF terminator configuration regresses end-of-input anchors on CRLF files. For foo\r\n, regexes foo\z and (?-m:foo$) previously matched, but now file selection searches foo\r and rejects it. CRLF mode only adjusts multiline anchors; the canonical line matcher searches stripped foo and would match. Both plain and UTF-16 searches inherit this configuration.
  fix: Ensure file selection and range extraction agree about stripped terminators for all supported anchors while preserving standalone carriage returns. Add CRLF regressions for \z and scoped (?-m:$), including UTF-16.
  instances: crates/fe-project/src/search.rs:359
