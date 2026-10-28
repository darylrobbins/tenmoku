---
task_uid: "01m5z4bd5e68vrv84xgghqzcdy"
task: "T1205"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-vim/tests/corpus/history.toml has recorded cases for empty-only macros under both :g and an address command, and for a final search with a delimiter or offset, with history.expected.json re-recorded from the pinned Neovim; the several-searches-in-one-ex-line cases (global_macro_with_several_searches, address_macro_with_several_searches) are left out: that divergence is T1209"
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-vim/tests/history_oracle.rs asserts the empty-only macro case keeps the pending search (foo) in history"
    required: true
    manual: false
  - id: "C3"
    text: "a case submits a final search containing a delimiter or offset and asserts its recall from both / and ?"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-vim --test history_oracle passes"
    required: true
    manual: false
---

# Follow-up from review of T1201: oracle coverage for nested search history (empty-only macros, delimiters, both directions)

Follow-up from review of T1201 (finding F1, medium, crates/fe-vim/tests/history_oracle.rs:218).

T1201 fixed nested search history in the vim command line, but its regression coverage is incomplete:
- no recorded oracle cases were added;
- the direct test only checks an empty search after nonempty submissions, never the empty-only macro case that must preserve the pending `foo`;
- its search strings contain no delimiter or offset, so delimiter-sensitive recall is unverified.

Add test coverage only (no production change unless a new case exposes a real bug; if one does, stop and report it rather than fixing it here):
- recorded oracle cases (pinned Neovim, via the existing history corpus/recorder) for multiple submitted searches and for empty-only macros, under both `:g` (global) and address commands;
- matching direct-history assertions in history_oracle.rs;
- a final submitted search containing a delimiter or an offset, with recall asserted from both `/` and `?`.
