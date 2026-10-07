---
task_uid: "01m49v7d9bdbxgbae00vp2t53a"
task: "T152"
rev: 3
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F1: import scanning keeps lexical context across scan restarts for the cases the review supplied. `const r = /;import a from \"util\"/;` and `const s = \"\\\";import a from 'util';\";` report no module, nor does an import-looking line inside a multiline /* */ comment or a multiline template literal. Known limits: a hand-written lexer can't follow every JS/TS/TSX context. Parenthesized arrow return types (F10), templates in JSX type arguments (F11), generic arrows across long comments (F12), deeply nested ternaries in case scopes (F14) and spaced conditionals (F17) are still misread. T168 replaces the lexer with tree-sitter queries and fixes them."
    required: true
    manual: false
  - id: "C2"
    text: "F2: named import and re-export lists accept complete JavaScript identifier tokens, including Unicode escapes and combining-mark continuations. `import { \\u0061 as helper } from './util'` and `export { \\u0061 } from './util'` both report ./util, and malformed-list recovery is kept"
    required: true
    manual: false
  - id: "C3"
    text: "F3: cancellation is polled by the work actually done inside import matching (per definition and import compared), the named-list lookahead and the require loop, and it propagates at once. Deterministic tests cancel after each of those loops has begun"
    required: true
    manual: false
  - id: "C4"
    text: "F4: the cancellation regressions use nonempty workloads and deterministic progress hooks to cancel during scanning and ranking, and after the pre-rank check. They fail if the periodic processing checks or the final definitions check are removed"
    required: true
    manual: false
  - id: "C5"
    text: "New tests live in crates/fe-nav/src/search_import_tests.rs, declared from search.rs by a #[cfg(test)] #[path] mod line, and fail at base. search_followup_tests.rs still builds against base"
    required: true
    manual: false
---

# Follow-ups from review of T136

Findings of the review of T136 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-nav/src/search.rs:968: Scanning arbitrary line contents with the import-only tokenizer introduces false imports. For example, `const r = /;import a from "util"/;` and `const s = "\";import a from 'util';";` now report `util`, whereas the base reports nothing. Resetting lexical state each line also recognizes inline declarations inside multiline comments and template strings. These phantom modules can change Imported ranking and automatic-jump decisions.
  fix: Preserve lexical context across scan restarts and correctly skip regular expressions, escaped strings, multiline comments and template contents before recognizing import/export statements; add regressions for each context.
  instances: crates/fe-nav/src/search.rs:968, crates/fe-nav/src/search.rs:1000

- F2 medium crates/fe-nav/src/search.rs:1084: The new named-list whitelist rejects valid identifiers that the tokenizer splits into punctuation. Both `import { \u0061 as helper } from './util'` and `export { \u0061 } from './util'` lose their module paths; identifiers containing combining marks also fail. The base recognized these statements, so this introduces another Imported-ranking regression.
  fix: Recognize complete JavaScript identifier tokens, including Unicode escapes and valid continuation characters, while retaining malformed-list recovery; test named imports and re-exports with both forms.
  instances: crates/fe-nav/src/search.rs:1084

- F3 medium crates/fe-nav/src/search.rs:620: Cancellation polling still skips substantial import-processing work. The ranking loop checks only every 1,024 definitions, although each definition can search every import; module lookahead consumes an arbitrarily long named list without checking cancellation, and the require loop has no checks at all. Large workloads therefore continue after cancellation despite F10's requested processing checks.
  fix: Poll cancellation by actual work performed inside import matching, named-list lookahead and require scanning, and propagate cancellation immediately; add deterministic regressions cancelling after those loops have begun.
  instances: crates/fe-nav/src/search.rs:620, crates/fe-nav/src/search.rs:989, crates/fe-nav/src/search.rs:1080

- F4 medium crates/fe-nav/src/search_followup_tests.rs:140: The cancellation regressions do not exercise cancellation during processing: the scanner flag starts true, the phase hooks fire before work, and the navigation uses an empty index. In particular, the Rank test exits at the pre-rank check, so deleting the final definitions cancellation check would leave these tests passing.
  fix: Use nonempty workloads and deterministic progress or completion hooks to cancel during scanning/ranking and after the pre-rank check; ensure the tests fail when periodic processing checks or the final definitions check are removed.
  instances: crates/fe-nav/src/search_followup_tests.rs:98, crates/fe-nav/src/search_followup_tests.rs:140, crates/fe-nav/src/search_followup_tests.rs:158

## Lead notes

All findings checked on main 4037dd1. All four still hold: F1 (import_lines re-tokenizes each line from its start with no lexical state carried over; search.rs:955-1000), F2 (names_or_namespace accepts only Word, Str and ',' tokens; search.rs:1076-1087), F3 (the ranking loop uses check_every per definition, while the require loop and the named-list lookahead have no checks; search.rs:616-624, 989-994 and 1076-1087) and F4 (search_followup_tests.rs is unchanged since T136).

New tests go in a new file, crates/fe-nav/src/search_import_tests.rs, declared from search.rs by `#[cfg(test)] #[path = "search_import_tests.rs"] mod import_tests;`. That way tests that use new hooks don't touch the base-compatible file. Edit search_followup_tests.rs only to delete or weaken-proof the T136 cancellation tests F4 names; it must still build against base. Move replacement tests that need new progress hooks into the new file.
