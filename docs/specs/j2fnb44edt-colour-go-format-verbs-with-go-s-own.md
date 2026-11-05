---
task_uid: "01m6mm1tdk4298dqj2fnb44edt"
task: "T450"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "go_printf_highlights::go_verbs_followed_by_text: in fmt.Errorf(\"%w happened\", e), fmt.Errorf(\"%w: x\", e) and fmt.Printf(\"%t ok\", b) exactly the verb is string.placeholder and the rest stays string"
    required: true
    manual: false
  - id: "C2"
    text: "go_printf_highlights::space_flag_is_part_of_the_verb: fmt.Printf(\"% d\", n) gives one string.placeholder span covering % d"
    required: true
    manual: false
  - id: "C3"
    text: "go_printf_highlights::c_size_modifiers_are_not_go_verbs: in fmt.Printf(\"%ld %zu\", a, b) the placeholder spans are %l and %z, one letter per verb as Go reads them, and d and u stay string"
    required: true
    manual: false
  - id: "C4"
    text: "T443's go_printf_highlights tests still pass; cargo nextest run -p fe-syntax and cargo clippy -p fe-syntax --all-targets -- -D warnings pass"
    required: true
    manual: false
  - id: "C5"
    text: "go_printf_highlights::appendf_format_is_the_second_argument: in fmt.Appendf(b, \"%d\", n) the %d is string.placeholder, and in Appendf(\"%d\", n) the string stays plain"
    required: true
    manual: false
---

# Colour Go format verbs with Go's own syntax: a small verb scanner so %w and %t followed by text, and % d, get one placeholder (follow-up of T443, Mac T1376)

## Problem
T443 (Mac T1376) colours Go format verbs by injecting tree-sitter-printf, which is a C grammar. It lexes `w` and `t` as length modifiers, so a Go `%w` or `%t` followed by more text parses as one long directive: `'%w happened'` matches `%w h`, and `'%w: x'` has no directive at all. T443 ships with no colour on any match that contains whitespace, recorded as a known limit (decided by the Mac lead for the user, 2026-11-05). So these common Go forms stay plain string, and a space-flag verb like `% d` does too.

## Change
Replace the printf grammar's directive parsing for Go with a small scanner for Go's own verb syntax: `%`, flags (`+ - # 0` and space), optional width and precision (including `*` and `[n]` indexes), one verb letter; `%%` is an escape. No new crate. The call-site selection from T443's go-injections.scm stays. Pick the seam by reading how fe-syntax applies highlights; keep other languages unaffected. Remove the whitespace exclusion and the known limit if nothing needs them any more, and drop tree-sitter-printf if nothing else uses it (THIRD_PARTY.md row included).

## Tests
Extend crates/fe-syntax/tests/go_printf_highlights.rs. C grammar size modifiers must not be read as part of a Go verb.

## Also: T443 review finding F3
`fmt.Appendf(b, format, args...)` takes its format as the second argument, but go-injections.scm lists Appendf under the first-argument pattern (the `^[A-Za-z]+f$` match at line 18) and not under the second-argument pattern (line 34). So its format string gets no verb colour, and a string first argument would be treated as a format. Exclude Appendf from the first-argument pattern and add it to the second. This is the one change to T443's call-site selection.

## Implementation notes

Injected `printf` layers are painted by `crates/fe-syntax/src/verbs.rs` (a Go verb scanner) from `Painter::paint` in injection.rs; the query file is gone. tree-sitter-printf stays, only to parse the layer, because layers are keyed by a tree; THIRD_PARTY.md row says so.
