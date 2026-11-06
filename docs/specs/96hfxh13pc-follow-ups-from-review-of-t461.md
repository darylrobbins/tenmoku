---
task_uid: "01m6qajv93962wyg96hfxh13pc"
task: "T467"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T461

Findings of the review of T461 that don't block the merge (below merge.block_severity), filed by the lead at the Mac lead's request (2026-11-06, note 1106a), for helium batch 105:

- F1 medium crates/fe-syntax/src/injection.rs:474: The fixed 256-byte margin and resynchronization heuristic lose valid highlights. A directive containing 600 zero flags is omitted when highlighting its middle because its opening percent lies outside the window; it is also omitted when its verb lies beyond the trailing margin. Starting the window inside a long run of percent signs skips every visible escape because none follows a non-percent byte. These incomplete results are reported as finished and can be cached.
  fix: Preserve scanner context across bounded, deadline-aware chunks instead of assuming directives and percent runs fit within a fixed margin. Add partial-range tests for long directives crossing either boundary and long percent runs.
  instances: crates/fe-syntax/src/injection.rs:474, crates/fe-syntax/src/injection.rs:475, crates/fe-syntax/src/injection.rs:488

- F2 medium crates/fe-syntax/src/injection.rs:492: The scanner base uses the requested start, but chunks() rounds that start left to a UTF-8 boundary. When subtracting 256 lands inside a multibyte character, every returned span shifts right. For content consisting of 150 'é' characters followed by 'a%d!', highlighting content-relative bytes 301..304 paints 'd!' instead of '%d'.
  fix: Normalize the window start to a character boundary before collecting bytes and use that actual start as the scanner base. Add a partial-range regression test with multibyte text at the margin boundary.
  instances: crates/fe-syntax/src/injection.rs:474, crates/fe-syntax/src/injection.rs:477, crates/fe-syntax/src/injection.rs:492
