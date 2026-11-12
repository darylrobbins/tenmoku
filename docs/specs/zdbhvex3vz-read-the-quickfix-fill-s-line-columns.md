---
task_uid: "01m6w2wf4n70213fzdbhvex3vz"
task: "T321"
rev: 5
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test quickfix_fill_columns passes with a_match_past_512_bytes_gets_its_column and a_bom_file_gets_the_right_column (base-API tests through the public :vimgrep path; the regression tests), and the unit tests in quickfix_fill.rs's own test module pass: a_symlinked_line_is_not_followed"
    required: true
    manual: false
  - id: "C2"
    text: "A FIFO line reread is refused without blocking, and UTF-16 lines get the right char column (unit tests in crates/fe-workbench/src/quickfix_fill.rs's test module, which may use the helper directly; no new public export is needed)"
    required: true
    manual: false
  - id: "C3"
    text: "One safe read of each line: columns for many matches on one long line come from one read and decode of that line through open_file_under, with non-regular files refused (a test counts reads or asserts the single-pass helper; no wall-clock thresholds; unit tests in the same module); periodic cancellation checks through preview, fallback and item traversal are a known limit, left to follow-up T353"
    required: true
    manual: false
---

# Read the quickfix fill's line columns safely: open_file_under, one read per line, long-preview matches, BOM and UTF-16 (T1405 F1-F4) (Mac T1494)

Follow-up of T1405 (helium's review of T482). In crates/fe-workbench/src/quickfix_fill.rs, `char_column` (used by the :grep/:vimgrep fill) has four defects:
- F1: the line reread uses `File::open`, not the project's `open_file_under`, so it can follow a symlink out of the root or block on a FIFO.
- F2: it rereads the line prefix once per match, so a long line with many matches is quadratic, and the reread cannot be cancelled. Read the line once per file/line and reuse it, checking the cancel flag.
- F3: a match with `preview_start == 0` can still have a later match in the same line past the 512-byte preview, whose column is then counted in the truncated preview.
- F4: a UTF-8 BOM at file start and long UTF-16 lines miscount the column (count from the decoded text the search used).
Regression tests go in a new tests/ file (base-API only), e.g. crates/fe-workbench/tests/quickfix_fill_columns.rs, declared as the tests layout requires; if char_column is private, expose the check through the public :grep path with a temp project.


Regression tests use base APIs only. If a named regression test cannot reach the code through base APIs, block and ask the lead for a respec rather than widening scope.
