---
task_uid: "01m54r2g695ghx541n2hgtf30n"
task: "T133"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-config test one_character_negated_classes_match_any_other_character (tests/editorconfig_negated_class.rs, top level) passes: with indent_size = 3 under the section, [[!_]*.py] applies to a.py and not _a.py, [*.[!c]] to a.h and not a.c, [[!a]] to b, [*.[!-]] and [*.[!\\]]] to a.x; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: classes with two or more members ([*.[!ch]]: a.x yes, a.h no), plain classes ([[c]]), property lines and every existing tests/editorconfig*.rs case; no ec4rs upgrade"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-config passes with no existing test edited; cargo clippy -p fe-config --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Match a one-character negated class [!x] in an .editorconfig section glob as any character but x, as EditorConfig defines, instead of x itself (Mac T766)

Filed on argon from Mac T766 (card ~/coordination/argon-cards/zzb/T766.json rev 1; note zzb). Mac task uid 01m54qvvjzwca1c8t5p0gzz068. Base: argon trunk 79b99f1 (= mac/main 42180d6 synced); the files it changes are byte-identical on Mac main 838c0b35. Title the host commit "... (Mac T766)".

## Problem

A negated character class holding one character, `[!x]`, in an `.editorconfig` section glob matches only `x`
in fastedit, the opposite of what EditorConfig defines (`[!name]` matches any single character not in
name). So `[[!_]*.py]` ("Python files not starting with `_`") applies to `_a.py` and not to `a.py`, and
`[*.[!c]]` applies to `a.c` and not to `a.h`. Classes with two or more members (`[!ch]`) are right.

The cause is in ec4rs 1.2.0 (`crates/fe-config` parses `.editorconfig` with it, `Cargo.toml` `ec4rs = "1.2"`):
its `glob/parser/charclass.rs` `parse` turns a class whose member set has one character into that literal
character, dropping the `!`. `crates/fe-config/src/editorconfig.rs` `read_file` hands the file's text to
`ec4rs::ConfigParser` unchanged (after stripping a byte order mark, which it already works around the same
way).

Reproduced at Mac main 838c0b35 with `fe_config::editorconfig_for` on a temp project whose `.editorconfig` is
`root = true` and the section below, with `indent_size = 3`:

| section | file | EditorConfig | fastedit |
|---|---|---|---|
| `[[!_]*.py]` | `a.py` | `indent_size` 3 | none |
| `[[!_]*.py]` | `_a.py` | none | 3 |
| `[*.[!c]]` | `a.h` | 3 | none |
| `[*.[!c]]` | `a.c` | none | 3 |
| `[[!a]]` | `b` | 3 | none |
| `[*.[!-]]` | `a.x` | 3 | none |
| `[*.[!\]]]` | `a.x` | 3 | none |

Already right, and must stay so: `[*.[!ch]]` (`a.x` 3, `a.h` none), `[[c]]` (`c` 3), and every other glob form
the existing `tests/editorconfig*.rs` cover.

## Change

In `crates/fe-config/src/editorconfig.rs` `read_file`, after the byte order mark is stripped and before
`ConfigParser` reads the text: in each section header line (its trimmed text starts with `[`), give every
negated class with exactly one member, `[!` then one character other than `]` (or `\` and one character) then
`]`, a second member, NUL (`\0`), which no path holds, so ec4rs builds a negated class from it. Nothing else in
the file changes, and property lines are left alone. Say why in a comment next to the byte order mark one. No
ec4rs upgrade (2.0 is a release candidate) and no Cargo change.

Regression test: a new integration test `crates/fe-config/tests/editorconfig_negated_class.rs`, at the top
level of the file, `one_character_negated_classes_match_any_other_character`, writing each section above into
a temp `.editorconfig` and checking `editorconfig_for(...).indent_size` for the table's files.
