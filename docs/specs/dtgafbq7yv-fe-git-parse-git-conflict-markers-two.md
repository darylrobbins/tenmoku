---
task_uid: "01m55ef97f1dmk31dtgafbq7yv"
task: "T176"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-git integration tests two_way_conflicts_split_at_their_markers, diff3_bases_are_kept and an_empty_side_has_an_empty_range (crates/fe-git/tests/conflict_markers.rs, top level) pass with exact byte ranges and labels; they fail at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git integration tests crlf_and_a_last_line_without_a_break and stray_markers_are_content pass: CRLF lines and an unterminated last line parse; ======= and \u003e\u003e\u003e\u003e\u003e\u003e\u003e outside a conflict are content, an unclosed \u003c\u003c\u003c\u003c\u003c\u003c\u003c at the end is dropped, and a \u003c\u003c\u003c\u003c\u003c\u003c\u003c inside an open conflict keeps the conflicts before it and ends the scan; they fail at the base"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-git passes with no existing test edited; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-git: parse git conflict markers (two-way and diff3) into each conflict's current, base and incoming ranges with their labels, by VS Code's merge-conflict rules (Mac T859)

## Problem

A file git left with conflict markers opens in fastedit as plain text: nothing outside fe-vim reads `<<<<<<<`
(gap audit 2026-10-17, user-approved: "card the LSP-completeness set and merge conflicts first"). VS Code's
merge-conflict support (bands, Accept Current / Incoming / Both, Compare, Next / Previous Conflict, the Merge
Changes group) all start from one parser of the markers. This card is that parser; the UI cards depend on it.

## Change

New module `crates/fe-git/src/conflict_markers.rs`, `pub mod conflict_markers;` in `crates/fe-git/src/lib.rs`.
Pure text, no git process:

- `pub fn parse(text: &str) -> Vec<Conflict>` scans whole lines (`\n` or `\r\n` ends one; a final line without one
  counts) with VS Code's rules (`extensions/merge-conflict/src/mergeConflictParser.ts`):
  - a line starting with `<<<<<<<` opens a conflict; one met while another is still open drops the open one and
    ends the scan (VS Code gives up there: the conflicts before it stay, the rest of the text has none);
  - inside an open conflict and before its splitter, a line starting with `|||||||` starts a common-ancestor
    (diff3 / zdiff3 base) section; several are allowed, each kept;
  - inside an open conflict with no splitter yet, a line starting with `=======` is the splitter;
  - inside an open conflict with a splitter, a line starting with `>>>>>>>` closes it and it is kept;
  - a conflict still open at the end of the text is dropped; every other line is content.
- `pub struct Conflict { pub start: Marker, pub current: Range<usize>, pub bases: Vec<(Marker, Range<usize>)>,
  pub splitter: Range<usize>, pub incoming: Range<usize>, pub end: Marker }` where `pub struct Marker { pub line:
  Range<usize>, pub label: String }`: every range is in bytes of `text`; a marker's `line` covers the whole marker
  line including its line break; `label` is the text after the marker run and one space, trimmed (`HEAD`,
  `feature`, `parent of 1a2b3c4 (msg)`); `current`, each base's range and `incoming` are the content lines between
  the markers, line breaks included (empty when no lines). Plus `Conflict::range()`: the start marker's first byte
  to the end marker's last.
- Doc comment: the rules above, that labels are free text, and that a `=======` or `>>>>>>>` outside a conflict is
  content (a Markdown setext heading stays one).

New integration test `crates/fe-git/tests/conflict_markers.rs`, top-level fns:
`two_way_conflicts_split_at_their_markers` (two conflicts with labels `HEAD` and `feature`, text between and around:
exact ranges and labels), `diff3_bases_are_kept` (a `||||||| base` section, and a zdiff3 file with two),
`crlf_and_a_last_line_without_a_break`, `stray_markers_are_content` (`=======` and `>>>>>>>` with no open conflict,
an unclosed `<<<<<<<` at the end; a `<<<<<<<` inside an open conflict keeps the conflicts before it and finds none
after), and
`an_empty_side_has_an_empty_range`.
