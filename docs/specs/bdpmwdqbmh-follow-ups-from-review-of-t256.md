---
task_uid: "01m6ktjw8159jxzkbdpmwdqbmh"
task: "T447"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T256

Findings of the review of T256 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-markdown/src/parse.rs:374: Escaping block markers also changes inline delimiter recognition. For example, `<b>\n```\nx\n```</b>` now produces literal backticks instead of Strong containing Code("x"). The inserted backslashes disable the matching backtick runs before inline parsing; unescaped_code cannot repair a code span that was never recognized.
  fix: Suppress block starts without changing inline delimiter recognition, and add regression cases for matching backtick runs beginning a row.
  instances: crates/fe-markdown/src/parse.rs:374

- F4 medium crates/fe-markdown/src/parse.rs:354: The new loop copies and reparses the entire HTML block once per blocking row. A `<b>` block containing N successive `# x` rows therefore requires N full parses, making processing quadratic. Hover parsing runs during rendering, so sufficiently long documentation can freeze the UI. Scanning every inserted offset for each code span adds another quadratic path.
  fix: Use a bounded number of parsing passes or process block markers together while preserving inline semantics; locate inserted offsets within code spans using ordered ranges rather than repeated full scans.
  instances: crates/fe-markdown/src/parse.rs:354, crates/fe-markdown/src/parse.rs:115, crates/fe-markdown/src/parse.rs:149
