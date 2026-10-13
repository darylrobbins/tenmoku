---
task_uid: "01m4syn5x7p7tg4725wkrtyscg"
task: "T110"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "The new crates/fe-vim/tests/pattern_build_memory.rs has a test named exactly composing_leaves_stop_at_the_compiled_budget (no other name): with a counting global allocator, Pattern::parse of the 3000-mark pattern returns PatternError::TooLarge with peak live heap growth under 64 MiB; it fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "Leaves are built as regex_automata::meta::Regex. Their summed memory_usage() never passes MAX_PROGRAM_COMPILED (32 MiB), and each is built with nfa_size_limit set to the smaller of MAX_LEAF_COMPILED and the budget left"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing fe-vim test passes unchanged, backtracking_patterns_are_bounded_to_build and the search oracles included"
    required: true
    manual: false
---

# fe-vim: refuse a backtracking search pattern once its leaves' compiled regexes pass 32 MiB in total, not after 200 MB (T351 review F2) (Mac T624)

## Problem

T351's codex review, finding F2 (crates/fe-vim/src/search/backtrack.rs:304, never filed): the backtracking
matcher's build budget counts regex *source* bytes summed over a program's distinct leaves
(`MAX_LEAF_TEXT`, 64 KiB, checked in `Builder::regex`), while each leaf's *compiled* size is limited only
per leaf (`MAX_LEAF_COMPILED`, 8 MiB, through `RegexBuilder::size_limit`). A leaf with a composing mark is a
short source that compiles to tens of kilobytes, so a pattern made of many distinct such leaves compiles
thousands of large regexes before the source budget refuses it.

The existing `backtracking_patterns_are_bounded_to_build` test (crates/fe-vim/src/search.rs:2635) parses
`\%(a\|.\)*` followed by 3000 distinct CJK chars each with U+0301. It does end in `PatternError::TooLarge`,
but the reviewer measured 205,815,808 bytes peak RSS and 7.33 s for that parse, against about 8 MiB for
the deadline regression. A vim user pasting or typing such a pattern into `/` (which parses on each key in
incremental search) or `:s` pays that in memory and time before the refusal. Still on main at e6988aba:
`backtrack.rs` is unchanged since 02e4e486 (T351).

## Change

Bound what a program's leaves compile to in total, not only each one:

- Build each leaf with `regex_automata::meta::Regex` instead of `regex::Regex` (regex's own `Regex` wraps
  it but hides `memory_usage()`). Add `regex-automata = "0.4"` to crates/fe-vim/Cargo.toml (already in the
  lock through `regex`). Keep the per-leaf limit with `meta::Config::nfa_size_limit(Some(MAX_LEAF_COMPILED))`
  and the same anchored `^(?:…)` source and syntax settings as now, so every leaf matches exactly what it
  matches today; the one call site, `Program`'s `self.regexes[regex].find(&hay[at..])`, becomes a
  `find(Input::new(&hay[at..]))` on the meta regex.
- In `Builder::regex`, add each built leaf's `memory_usage()` to a running total and return
  `PatternError::TooLarge` as soon as it passes a new `MAX_PROGRAM_COMPILED` of 32 MiB, before building
  more; build each leaf with `nfa_size_limit` of the smaller of `MAX_LEAF_COMPILED` and what is left of
  that budget, so no single leaf overshoots it either. Map a meta build error whose `size_limit()` is set to `TooLarge` as `super::build` does for
  `CompiledTooBig`.
- Keep `MAX_LEAF_TEXT`, `MAX_PROGRAM` and the step and stack budgets as they are.

Regression test: a new integration test file crates/fe-vim/tests/pattern_build_memory.rs with a counting
`#[global_allocator]` (a wrapper over `std::alloc::System` tracking live and peak bytes in atomics; it is
its own test binary, so it affects no other test). It has exactly one test, which must be named exactly
`composing_leaves_stop_at_the_compiled_budget`: it resets the peak, calls
`fe_vim::search::Pattern::parse` on the same 3000-mark pattern as `backtracking_patterns_are_bounded_to_build`,
asserts `Err(PatternError::TooLarge(_))`, and asserts the peak live heap growth during the parse stayed
under 64 MiB. No wall-clock assertion: it must not depend on machine load.
