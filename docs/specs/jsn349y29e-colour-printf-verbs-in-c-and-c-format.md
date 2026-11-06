---
task_uid: "01m6rgr1njk2fj4djsn349y29e"
task: "T1391"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "verb_in_printf_is_a_placeholder: in printf(\"%s: %d\\n\", a, b) %s and %d are string.placeholder and the rest stays string"
    required: true
    manual: false
  - id: "C2"
    text: "every_c_conversion_form_is_one_placeholder: %5.2f %-8s %ld %llu %zu %#x %*d %1$d %hhd each give one placeholder covering the whole directive, and %% gives escape"
    required: true
    manual: false
  - id: "C3"
    text: "invalid_directives_paint_nothing: '% ', '%5', '%y' and a trailing '%' have no placeholder"
    required: true
    manual: false
  - id: "C4"
    text: "format_position_per_function: fprintf(f, \"%d\", n), snprintf(buf, n, \"%d\", x) and syslog(LOG_ERR, \"%s\", s) colour their verbs; printf(fmt, a), puts(\"%d\") and foo(\"%d\") do not"
    required: true
    manual: false
  - id: "C5"
    text: "cpp_std_qualified_and_raw_strings: std::printf and a C++ R\"(%d)\" format argument colour their verbs"
    required: true
    manual: false
  - id: "C6"
    text: "verbs::c_dialect_tests::chunking_never_changes_the_c_spans: scanning in chunks of 1..len bytes gives the same spans as one pass"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-syntax and cargo clippy -p fe-syntax --all-targets -- -D warnings pass; the Go tests (go_printf_highlights, verbs::tests) pass unchanged and c.c.golden and cpp.cpp.golden differ only by added spans"
    required: true
    manual: false
---

# Colour printf verbs in C and C++ format strings (printf, fprintf, sprintf, snprintf, dprintf, syslog and v-forms) apart from the string, with a C dialect for the verb scanner

## Problem
T1376 (landed as T443/T450) colours Go format verbs by injecting the hidden `printf` language into Printf-style calls and painting the layer with src/verbs.rs's Go scanner (the printf grammar mis-reads Go's `%w`). The same is wanted for C and C++: in printf("%s: %d\n", name, n) the `%s` and `%d` should colour apart from the string (string.placeholder) and `%%` as an escape, as in VS Code and CLion.

## Change
- verbs.rs: add `Dialect { Go, C }` to Scanner (`Scanner::new(base, dialect)`; the Go behaviour and its two existing tests unchanged). The C phases: `%`, optional `n$` position, flags `- + space # 0 '`, width (digits or `*`, or `*n$`), `.` precision (digits or `*`), length modifier (hh h l ll j z t L q), then one conversion char from `diouxXeEfFgGaAcCsSpnmb`; `%%` is an escape. A `%` with no valid conversion (`% `, `%5`, end of string) paints nothing, as for Go. Still resumable by chunk: phase and two offsets only.
- language.rs: a hidden `printf-c` Def beside `printf` (same tree_sitter_printf grammar, no highlights, hidden: true), gated on the printf feature; injection.rs picks the dialect from the layer's language name ("printf" Go, "printf-c" C) where it now tests `name() == "printf"`. Cargo: `c` and `cpp` features also enable `printf` (crates/fe-syntax/Cargo.toml); the c and cpp Defs get `injections: Some(include_str!(..))`.
- queries/c-injections.scm and queries/cpp-injections.scm (ours; C++ grammar names its raw strings raw_string_literal/raw_string_content, C has only string_literal/string_content): inject `printf-c` into the string_content of the format argument of a call_expression whose function is an identifier in: printf vprintf (arg 1); fprintf vfprintf dprintf sprintf vsprintf asprintf vasprintf syslog (arg 2); snprintf vsnprintf (arg 3); `std::`-qualified forms for C++ (qualified_identifier name). Match the position with `.`-anchored argument_list patterns and a `#any-of?` per position, as go-injections.scm does, so a value argument never injects. A concatenated_string argument injects each piece (a verb split across pieces, or by a PRId64-style macro, paints nothing: note it in a comment).
- std::format / fmt::print brace fields are NOT in this card (different syntax; they arrive as a reuse of the Python str.format scanner in the next card, then a C++ follow-up). Rust format! is already coloured by rust-analyzer.
- Regenerate crates/fe-syntax/tests/golden/c.c.golden and cpp.cpp.golden (add one printf call to each sample, crates/fe-syntax/tests/golden/c.c and cpp.cpp, if the samples have none); the diff must show only added placeholder/escape spans.

## Tests
New file crates/fe-syntax/tests/c_printf_highlights.rs (`#![cfg(all(feature = "c", feature = "printf"))]`, with the spans() helper of go_printf_highlights.rs, including the take_missing_languages loop); unit tests for the C dialect in a new `#[cfg(test)] #[path = "verbs_c_tests.rs"] mod c_dialect_tests;` file crates/fe-syntax/src/verbs_c_tests.rs declared from verbs.rs (keep verbs.rs's own tests as they are). C++ cases go in the same integration file under `#![cfg(feature = "cpp")]` blocks or a second file crates/fe-syntax/tests/cpp_printf_highlights.rs.
