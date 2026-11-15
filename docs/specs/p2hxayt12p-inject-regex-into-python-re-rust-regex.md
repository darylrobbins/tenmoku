---
task_uid: "01m7c01f6n9tz1qbp2hxayt12p"
task: "T635"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features regex,python,rust,go --test regex_injections passes: python_re_raw_string_is_a_regex, python_escaped_string_with_backslash_stays_a_string, python_escaped_string_without_backslash_is_a_regex, rust_regex_new_raw_string_is_a_regex, rust_ordinary_string_with_backslash_stays_a_string, go_regexp_raw_string_is_a_regex, non_regex_calls_are_not_injected"
    required: true
    manual: false
  - id: "C2"
    text: "Limit stated in each injection file header: raw/verbatim strings are injected whole; escaped strings only when their content has no backslash"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax highlights_match_golden_files passes with python, rust and go goldens regenerated"
    required: true
    manual: false
---

# Inject regex into Python re.*, Rust Regex::new and Go regexp.* strings (Mac T1824)

# Inject regex into Python re.*, Rust Regex::new and Go regexp.*

Depends on F (built-in `regex` grammar, hidden), D2 (python.py golden) and E (rust/go goldens). Inject the
`regex` language into the string argument of:

- Python (`crates/fe-syntax/queries/python-injections.scm`): `re.compile/search/match/fullmatch/sub/subn/split/findall/finditer`
  (first argument; `re.sub`'s pattern is the first argument, not the replacement), including `import re as x`-free
  plain `re.` only; do not chase aliases. Raw strings (`r"..."`, `rb`) are injected whole; an escaped (non-raw)
  string is injected only when its content contains no backslash, because the host source text then equals the
  regex text; with a backslash it is left a plain string. State this limit in the file header.
- Rust (`rust-injections.scm`): `Regex::new`, `RegexBuilder::new`, `RegexSet::new` (the elements of an array
  argument), `bytes::Regex::new`, and `regex!` macro strings. Raw strings `r"..."` / `r#"..."#` injected whole
  (the content node, not the quotes/hashes); ordinary strings only without a backslash (same limit; Rust's `\\d`
  is two characters in source).
- Go (`go-injections.scm`): `regexp.MustCompile`, `regexp.Compile`, `regexp.MatchString`, `regexp.MustCompilePOSIX`
  (first argument). Raw strings (backticks) injected whole without the backticks; interpreted strings only
  without a backslash. The existing go patterns' disjoint-name-set convention (header line ~9) applies: the new call
  names must not overlap the SQL and printf sets.

The three features gain `"regex"` (`python = [..., "regex"]`, `rust`, `go`) in `crates/fe-syntax/Cargo.toml`.
`#set! injection.language "regex"` and `injection.include-children` exactly as the neighbouring patterns do;
read each file's existing patterns first and follow their shape.

Regenerate goldens `python.py`, `rust.rs`, `go.go` (add a regex call to each sample if absent) and review that
only the new regex spans change.

**Test.** New file `crates/fe-syntax/tests/regex_injections.rs`,
`#![cfg(all(feature = "regex", feature = "python", feature = "rust", feature = "go"))]`, `spans` helper like
`tests/go_sql_injections.rs`. `python_re_raw_string_is_a_regex` (`re.compile(r"\d+(?P<n>x)")`: `\d` escape,
`+` operator, `n` label), `python_escaped_string_with_backslash_stays_a_string` and
`python_escaped_string_without_backslash_is_a_regex` (`re.search("a+b", s)`), `rust_regex_new_raw_string_is_a_regex`,
`rust_ordinary_string_with_backslash_stays_a_string`, `go_regexp_raw_string_is_a_regex`, and
`non_regex_calls_are_not_injected` (`foo.compile(r"\d")`, `Other::new(r"\d")`, `regexp.QuoteMeta` stay strings).
Base fails (no injection).

Conflicts: python-injections.scm, rust-injections.scm, go-injections.scm, python/rust/go goldens, Cargo.toml: no open
task lists them besides E, D2 and F (dependencies). Linux-OK.
