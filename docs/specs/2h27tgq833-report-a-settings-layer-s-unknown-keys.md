---
task_uid: "01m4syn8hc75v5n12h27tgq833"
task: "T82"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "The new kiln/crates/kiln-core/tests/diagnostic_scaling.rs has a test named exactly unknown_keys_are_reported_in_linear_time (no other name): check_schema on 20,000 unknown keys returns 20,000 diagnostics in under 5 s, the last at line 20,000 column 1; it fails at base (lead decision, overnight: each of the 20,000 lines may carry a trailing TOML comment, as long as the keys, the diagnostics and their lines and columns are unchanged, so that the base fails the 5 s bound on a fast machine; the test states the fixture's size and why it's padded)"
    required: true
    manual: false
  - id: "C2"
    text: "A test named exactly line_and_column_match_the_old_scan: CRLF line ends, a multi-byte char before the key, and a key on a last row without a newline give the hand-computed lines and columns"
    required: true
    manual: false
  - id: "C3"
    text: "Layer::diagnostic_at's public signature and output are unchanged; every existing kiln-core and fe-config test passes unchanged"
    required: true
    manual: false
---

# Report a settings layer's unknown keys in linear time, from a line index built once per check_schema instead of a rescan per key (Mac T626)

Filed on argon from Mac T626 (card ~/coordination/argon-cards/T626.json rev 1). Mac task uid 01m4syj9j4crq80qnmww1e990r. No dependencies. The criteria name exact test names: the pinned regression tests must use exactly those names. Title the host commit "... (Mac T626)".

## Problem

`Layer::check_schema` (kiln/crates/kiln-core/src/layers.rs, around line 214) adds an "unknown key,
ignored" warning for every key the schema doesn't know. Each warning goes through `Layer::diagnostic_at`
(around line 151), which calls `line_column` (kiln/crates/kiln-core/src/diagnostic.rs:81). That rescans
`source` from byte 0 to the key's offset (`before.matches('\n').count()`) on every call, so a layer with
k unknown keys costs O(k x file size): quadratic in the file.

A project's `<root>/.fastedit/settings.toml` is untrusted and may be up to 1 MiB; fastedit loads it with
a schema and runs `check_schema` on it (crates/fe-config/src/settings.rs, around lines 584 and 637), so
opening a folder holding such a file stalls. A probe crate against main (e6988aba, release build) with
lines `k000001=1`, `k000002=1`, ... took 7.4 s for 10k keys and 93 s for 50k; a 1 MiB file (about 110k
keys) stalls opening the folder for minutes. `line_column` is unchanged since K1.2 (95939242).

## Change

- Add a `LineIndex` to kiln/crates/kiln-core/src/diagnostic.rs: built once from a source string (the
  byte offsets where each line starts), with a `line_column(&self, source, offset)` that gives exactly
  what `line_column` gives today (1-based line and char column, an offset inside a char moved back to
  its start, an offset past the end clamped), finding the line by binary search. Make the existing
  `line_column` a thin call through it or keep it for one-off use; either way they must agree.
- `Layer::check_schema` builds one `LineIndex` for the layer's source at its start and the diagnostics
  it adds use it, through a private variant of `diagnostic_at` that takes the index. `diagnostic_at`'s
  public signature and output are unchanged.
- Diagnostic text, order, line and column are unchanged for every input.

Regression test: a new integration test file kiln/crates/kiln-core/tests/diagnostic_scaling.rs (the
crate has no tests/ dir yet), public API only (`kiln_core::parse_layer`, `Layer::check_schema`). It must
contain a test named exactly `unknown_keys_are_reported_in_linear_time`: parse 20,000 lines
`kNNNNNN=1\n` against the schema `{"type":"object","properties":{"editor":{"type":"object"}}}`, then
assert `check_schema` returns 20,000 diagnostics, that the last one is on line 20,000, column 1, and that
the call took under 5 s (milliseconds with the fix even under load; tens of seconds at base in a debug
build). A second test named exactly `line_and_column_match_the_old_scan` checks diagnostics on a small
source with CRLF line ends, a multi-byte char before the key on its line, and a key on the last line with
no trailing newline, against hand-computed lines and columns.

Regression criterion (argon lead clarification): only the pinned regression test unknown_keys_are_reported_in_linear_time must fail at base. line_and_column_match_the_old_scan pins the old scan's lines and columns, so it is meant to pass at base as well as on the candidate.
