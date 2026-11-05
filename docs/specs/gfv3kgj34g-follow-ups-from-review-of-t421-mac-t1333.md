---
task_uid: "01m6gzy0ata00yzxgfv3kgj34g"
task: "T430"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax exposes the token boundaries of injected layers, with a test"
    required: true
    manual: false
  - id: "C2"
    text: "ranges_in expands to whole tokens of injected languages at both viewport edges, tested"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax and -p fe-workbench pass"
    required: true
    manual: false
  - id: "C4"
    text: "Shell heredoc and injected named raw-string delimiters are trimmed correctly, injected-language escapes use that language's widths, and string forms are recognised from node kinds not name substrings, each with a test"
    required: true
    manual: false
---

# Follow-ups from review of T421 (Mac T1333): expose injected-layer token boundaries so spell viewport ranges expand whole in injected languages

## Problem
Review of T421 (Mac T1333, spellable ranges) left F4 as a Known limit: at a viewport edge, `ranges_in(row_range)` expands to whole tokens of the host language only. Tokens in injected languages (a Rust comment in a Markdown fence is skipped in v1, but other injections such as strings in HTML script) are not expanded whole, because the injected layer's token boundaries are not exposed by fe-syntax.
## Change
Expose the injected layers' token boundaries from fe-syntax (an injected-tree API next to `crates/fe-syntax/src/injection.rs`) and use them in spell_ranges.rs so viewport expansion follows the containing token of whichever layer holds the range. Tests cover both viewport boundaries on injected content. Remove the known-limit note from C6's test comments.

## Also (literal forms, from reviews erg9v1 and jw88bq of T421)
Known limits taken at the round cap, fix them here with tests: F5 delimiter handling for shell heredoc markers and injected named raw-string delimiters; F8 escape widths in injected-language strings (e.g. JavaScript hex escapes inside an injection) follow the injected language, not a fixed width; F11 literal structure told from the grammar's node kinds, not node-name substrings (e.g. Python string forms). Remove the matching known-limit notes from the criteria of T421's tests.
