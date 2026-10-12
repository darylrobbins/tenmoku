---
task_uid: "01m4rjfjdqw4hn6dvhypmng3q8"
task: "T50"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "replace: for literal, regex, whole-word and case-insensitive queries over a temp project including a CRLF file, the ranges Replacer::edits returns for each file equal the ranges search reports for it (line offset plus range)"
    required: true
    manual: false
  - id: "C2"
    text: "replace: the template table of the find bar ($0, $\u0026, $1..$99, ${name}, $$, \\n, \\t, \\\\, literal with regex off) expands identically"
    required: true
    manual: false
  - id: "C3"
    text: "replace: an invalid regex is SearchError::InvalidPattern from Replacer::new, and Replacer::count equals edits().len()"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-project and cargo clippy -p fe-project --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Compute project-search replacements in fe-project: a Replacer that finds exactly what search finds in a file's text and expands the replacement (Mac T506)

Filed on argon from Mac T506 (card ~/coordination/argon-cards/T506.json rev 1). Mac task uid 01m4rhd3bhbvn44mandanxrx9e. Title the host commit "... (Mac T506)".

## Problem

The project search panel (`crates/fe-workbench/src/search_panel.rs`) can only find; there is no project-wide
replace (day-one audit 2026-10-12; user: "card the day-one ones"). fe-project's search (`crates/fe-project/src/
search.rs`) reports line ranges from grep-regex but nothing turns a file's text plus a replacement into edits with
the same matching rules.

## Change

New module `crates/fe-project/src/search/replace.rs`, declared from `crates/fe-project/src/search.rs` (a child
module, so it reuses the private `SearchQuery::matcher`), re-exported from `crates/fe-project/src/lib.rs`. Pure,
no GPUI.

- `Replacer::new(query: &SearchQuery, template: &str) -> Result<Replacer, SearchError>`; an invalid pattern is the
  same `InvalidPattern` error `search` gives.
- `Replacer::edits(&self, text: &str) -> Vec<(Range<usize>, String)>`: every match `search` would report in a file
  with this text, as byte ranges in `text`, in order, each with its expansion. Same rules as the search: line by
  line, never across `\n`, a `\r` before `\n` not part of the line, `^`/`$` at line ends, whole word by
  `is_word_char`, case from `case_sensitive`, empty matches included once.
- `Replacer::count(&self, text: &str) -> usize` for previews.
- Template rules are the find bar's (fe-vim `search::replace`, a sibling card): literal unless `query.regex`; with
  regex `$0`/`$&`, `$1`..`$99` (two digits at most), `${name}`, `$$`, `\n`, `\t`, `\\`. Write the parser here (fe-
  project doesn't depend on fe-vim); the tests use the same example table.

## Tests

New file `crates/fe-project/tests/replace.rs`.
