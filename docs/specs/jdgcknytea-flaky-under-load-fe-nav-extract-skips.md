---
task_uid: "01m4k920dnp9qfpsjdgcknytea"
task: "T326"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "New fe-nav unit test extract::lossy_tests::symbols_after_lossily_decoded_bytes_select_their_names writes the same \u003e64 KiB lossy.rs fixture, asserts from_reader_detecting reports UTF-8, extracts it through extract_with with Syntax::parse_until(Duration::MAX, || false), and keeps every assertion the old case made: one symbol after_lossy, name_range filler.len()+13..filler.len()+24, row 6_001, rel/len/modified/truncated, and the loaded text at name_range is after_lossy"
    required: true
    manual: false
  - id: "C2"
    text: "tests/extract.rs skips_what_it_cannot_index no longer writes or extracts lossy.rs, and its Unsupported, TooLarge, Binary and Unreadable cases are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Nothing outside the three scope files changes (apart from Regatta's own spec file), and extract.rs gains only the #[cfg(test)] mod declaration: no change to fe-syntax, FULL_PARSE_CAP, extract_in or any Cargo.toml"
    required: true
    manual: false
  - id: "C4"
    text: "Under a 1-min load average of at least 2x hw.ncpu (busy loops added only below that, in their own process group with a kill trap and a timeout of 10 min or less), cargo nextest run -p fe-nav -E 'binary(extract) | test(lossy_tests)' passes in 10 of 10 runs, with each run's load average recorded in the hand-in"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-nav passes"
    required: true
    manual: false
---

# Flaky under load: fe-nav extract::skips_what_it_cannot_index: parse its lossy fixture without fe-syntax's 5 s parse cap

# Make the flaky fe-nav test extract::skips_what_it_cannot_index pass under load

## Problem

`fe-nav::extract skips_what_it_cannot_index` (`crates/fe-nav/tests/extract.rs:85`) fails in gates under
machine load (75-240 on 14 cores) and passes alone in about 0.07-0.3 s:

- run `01m4k7ptaeqsn5x27vmcb0avhp`: FAIL after 8.146 s, panicked at `crates/fe-nav/tests/extract.rs:35:46`,
  `called Result::unwrap() on an Err value: ParseFailed`;
- run `01m4k7rwdjanwve1mkb899edma`: FAIL after 6.549 s;
- run `01m4k5x4h5sddmkfd8hwsqr8ch`: FAIL after 6.105 s.

(Logs: `~/.local/state/regatta/runs/<run>/test.fastedit.log`.)

Cause: the test's input, not production. The lossy case (`tests/extract.rs:114-132`, added by T287 rev 2)
writes `lossy.rs` as `"plain ascii filler\n".repeat(6_000)` (114,000 bytes, which isn't Rust, so tree-sitter
error-recovers over all of it) plus `// \xff\xfe\nfn after_lossy() {}\n`, and parses it twice from scratch:
once at line 125 and again in `checked` (line 35). `fe_nav::extract` goes through `extract_in`
(`crates/fe-nav/src/extract.rs:103`), which calls `ParseJob::run`. That gives a full parse a wall-clock
`FULL_PARSE_CAP` of 5 s (`crates/fe-syntax/src/syntax.rs:60`); unlike `QUERY_BUDGET` (`syntax.rs:41`) it has
no test or `no-query-budget` exemption. On overrun the tree is `None`, `symbols` returns
`NoOutline::NotParsed`, and `extract_with` maps that to `Skipped::ParseFailed` (`extract.rs:142`). That is
the documented runaway-parse guard working as designed, as in T126. A smaller fixture only shrinks the
window: the observed slowdown is more than 50x, so the case has to parse without a wall clock.

The filler size is needed: more than the 64 KiB `Buffer::from_reader_detecting` looks at, so the file is
taken for UTF-8 and decoded lossily (`tests/extract.rs:114-117`).

## Change

Test-only. Production behaviour doesn't change.

- New unit-test file `crates/fe-nav/src/extract_lossy_tests.rs`, declared at the end of
  `crates/fe-nav/src/extract.rs` as `#[cfg(test)] #[path = "extract_lossy_tests.rs"] mod lossy_tests;`
  (the pattern `search.rs` uses for `search_followup_tests.rs`). It has one test that moves the lossy case
  there unchanged in intent:
  - write the same fixture (same filler, same invalid bytes, same `after_lossy`) to a tempdir and assert
    `Buffer::from_reader_detecting` of it reports `UTF-8`;
  - canonicalize the root (`extract_with` expects a canonical root, as `extract` passes it), then call
    `extract_with(&root, Path::new("lossy.rs"), |syntax| assert!(syntax.parse_until(Duration::MAX, || false)))`:
    the same read and outline as `extract`, but a parse with no wall-clock stop;
  - keep every assertion the lossy case and `checked` made for it: exactly one symbol, `after_lossy`,
    `name_range == filler.len() + 13..filler.len() + 24`, `row == 6_001`, `rel`, `len` and `modified`
    match the file's, `truncated` is false, the text that `Buffer::from_reader` loads selects
    `after_lossy` at `name_range` and has it on `row`, and `range` contains `name_range`.
- `crates/fe-nav/tests/extract.rs` `skips_what_it_cannot_index`: delete the lossy block (lines 114-132,
  from the "More than the 64 KiB" comment to the end). Its other cases (Unsupported, TooLarge, Binary,
  Unreadable) stay as they are.
- Don't touch `crates/fe-syntax`, `FULL_PARSE_CAP` or `extract_in`. No `Cargo.toml` change: `tempfile` is
  already a dev-dependency and `fe-buffer` a dependency.

Not on this card: `search::tests::the_deadline_doesnt_wait_for_an_extraction` is a different flake. Its
extraction is a fake `Gate`, nothing is parsed, and it fails as `gate.started()` == 0 at
`search.rs:1294` after about 0.36 s (runs `01m4k5x4h5sddmkfd8hwsqr8ch`, `01m4ehwzkkkse6zbr7v8wkw4vx`,
`01m4c5bz0zmmdnbgc0b0akeqm4`): the 300 ms budget runs out before the fallback thread reaches the
extraction.

## Proof

Record in the hand-in: the load average for each run, and how much load was added. Add load only
while the 1-minute load average is below 2× `sysctl -n hw.ncpu`. A generator runs busy loops in its own
process group, with a `trap` that kills the group on EXIT/INT/TERM and a hard timeout of 10 minutes or
less. Cap: 10 runs.

## Implementation notes

Proof (hw.ncpu=14, threshold 28; no load was added, the machine was already loaded). 1-min load average per run, 10 of 10 passed:

- 62.20
- 58.18
- 59.37
- 59.37
- 58.06
- 58.06
- 57.17
- 57.17
- 56.20
- 56.20
