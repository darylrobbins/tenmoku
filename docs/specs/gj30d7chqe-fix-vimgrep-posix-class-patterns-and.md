---
task_uid: "01m6w2ydtnv4y80tgj30d7chqe"
task: "T520"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-vim --test vimgrep_patterns passes, including a_posix_class_bracket_stays_in_the_pattern ([[:alpha:]/] splits correctly). F5 is already fixed on helium's base (collection_len skips [:name:] classes), so this test is a guard that pins the behaviour, not a regression test; no production change is needed for F5"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test quickfix_smart_case passes, including a_hex_escape_to_an_uppercase_letter_is_case_sensitive; \\p{Lu} and [[:upper:]] stay case-insensitive as in ripgrep"
    required: true
    manual: false
---

# Fix :vimgrep POSIX-class patterns and smart-case hex escapes (T1405 F5 F6) (Mac T1495)

Follow-up of T1405. Two defects in the :vimgrep pattern handling.
- F5: `collection_len` in crates/fe-vim/src/ex/quickfix.rs ends a collection at the inner `]` of a POSIX class, so `/[[:alpha:]/]/` is split at the wrong `/`. crates/fe-vim/src/search.rs `collection_len` already skips `[:name:]`; do the same here (share it if cheap).
- F6 (narrowed): the workbench's hand-written `smart_case` in crates/fe-workbench/src/quickfix_fill.rs is checked against ripgrep 15 here: `\p{Lu}` and `[[:upper:]]` stay case-INSENSITIVE under rg --smart-case (the existing test is right), but a hex escape that decodes to an uppercase literal (`\x41`, `\x{41}`, `A`) makes rg case-SENSITIVE. Fix only that: decode the escape and test the char. Keep the existing assertions otherwise.


Regression tests use base APIs only. If a named regression test cannot reach the code through base APIs, block and ask the lead for a respec rather than widening scope.
