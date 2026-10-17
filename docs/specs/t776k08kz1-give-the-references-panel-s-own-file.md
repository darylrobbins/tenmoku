---
task_uid: "01m53var9vy6zge2t776k08kz1"
task: "T125"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-nav test references::preview_tests::a_hit_near_a_long_lines_end_fills_the_window passes: a hit on x in 600 a's then ' x' gets a 512-byte window ending in ' x' with its range on x; with é's in place of the a's, a 511- or 512-byte window on char boundaries ending in ' x' with its range on x; a hit at byte 300 of a 1,000-byte line and one at its start keep today's windows; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-nav passes with no existing test edited (a_long_word_has_no_preview_range_when_cut included); fe-project is unchanged; cargo clippy -p fe-nav --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Give the References panel's own-file previews a full 512-byte window near a long line's end, as its other files' previews have (Mac T732)

Filed on argon from Mac T732 (card ~/coordination/argon-cards/T732.json rev 1; note zp). Mac task uid 01m53v4sx2jrfv1qvrqsz888f4. Base: argon trunk fd842fd (= mac/main a6fea65 synced). Lead's note: rgt's regression check runs the named test at the pre-fix base with the test files carried over, and matches a src/ unit test by its full module path. So declare the new references_preview_tests.rs from the existing test module references_followup_tests.rs (already included under cfg(test) at the base), not from references.rs, and keep it there. The regression test is filed as references::followup_tests::preview_tests::a_hit_near_a_long_lines_end_fills_the_window. Before finishing, check the real id with `cargo nextest list -p fe-nav` and tell the lead if it differs, so the lead can respec it. references_followup_tests.rs is added to the scope and is a test-support path. Title the host commit "... (Mac T732)".

## Problem

`crates/fe-nav/src/references.rs` `preview` cuts the preview of a hit in the request's own file, and its
doc and `PREVIEW_BYTES` say it's cut as `fe_project::LineMatch::preview` is for every other file. It isn't
near a long line's end: it takes `(PREVIEW_BYTES - hit.len()) / 2` bytes before the hit and then up to
`PREVIEW_BYTES` from there, so when the line ends sooner the window is short, and the room the after side
didn't use isn't given to the before side. `fe_project::search::preview` (crates/fe-project/src/search.rs
~948) takes the hit, up to half the rest before it, then what fits after, then the rest before.

Read at a6fea65f: for a line of 600 `a`s and ` x`, a hit on `x` (bytes 601..602) gets a 256-byte window
(255 `a`s, space, `x`) in the own file, and a 512-byte one in the same line of any other file. So in the
References panel the current file's hits at the end of long lines (minified JS, long SQL or log lines)
show half the context its other files' hits do.

## Change

- `preview` in references.rs: after taking the hit and up to half the room before it and what fits after
  (to the line's end), give what's left of `PREVIEW_BYTES` to the before side, back to the line's start
  or the look-back limit, on char boundaries. A hit in the middle of a long line or near its start keeps
  today's window; a long word cut by the window still gets no range. No more than `PREVIEW_BYTES` either
  side of the hit is looked at, as now. Don't change fe-project.
- New test file `crates/fe-nav/src/references_preview_tests.rs`, declared from references.rs as
  `#[cfg(test)] #[path = "references_preview_tests.rs"] mod preview_tests;` like `followup_tests`. Test
  `a_hit_near_a_long_lines_end_fills_the_window`: the 600-`a` line above gives a 512-byte window ending
  in ` x` with the range on `x`; the same with `é`s in place of the `a`s gives a window of 511 or 512
  bytes on char boundaries ending in ` x` with the range on `x`; a hit at byte 300 of a 1,000-byte line
  and a hit at its start keep today's windows. It fails at the base.
