---
task_uid: "01m46tf179pqzq0nyn51qm29j3"
task: "T67"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T66

Findings of the review of T66 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/src/outline.rs:374: The declarator fallback selects comments because they are named children. Valid definitions such as `int (/* note */twice)(int x) { return x; }` and `typedef void (/* note */*Callback)(int);` consequently receive the name `/* note */` instead of the declared identifier.
  fix: Select only identifier or declarator children when following an unfielded declarator, excluding extras and modifiers. Add regression cases for comments inside parenthesized function and typedef declarators.
  instances: crates/fe-syntax/src/outline.rs:374

- F2 medium crates/fe-syntax/src/outline.rs:347: Section boundaries are computed only from retained items after the query stops at the cap. With 10,001 sibling level-1 headings, heading 10,000 incorrectly extends through heading 10,001 to EOF, so item_at reports the retained sibling throughout the omitted section. Omitted higher-level headings can likewise leave retained ancestors open incorrectly.
  fix: Determine retained headings' section boundaries using subsequent headings in the parsed tree, including omitted headings, while preserving the item cap. Test truncation with omitted same-level and higher-level headings and verify ranges and item_at.
  instances: crates/fe-syntax/src/outline.rs:265, crates/fe-syntax/src/outline.rs:347

- F3 medium crates/fe-syntax/src/outline.rs:433: Closing-sequence detection treats all Unicode whitespace as Markdown spaces. Both `# T<NBSP>#` and `# T #<NBSP>` lose their literal hash and become `T`; their normalized outline name should be `T #`. [CommonMark](https://spec.commonmark.org/0.31.2/#atx-headings) permits only spaces or tabs before and after a closing sequence.
  fix: Use ASCII space and tab checks for closing-sequence recognition, retaining the existing whitespace normalization afterward. Test nonbreaking spaces both before and after trailing hashes.
  instances: crates/fe-syntax/src/outline.rs:429, crates/fe-syntax/src/outline.rs:433
