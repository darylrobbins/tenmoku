---
task_uid: "01m7ac1sf2mda7zp7g03w4rg3f"
task: "T606"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-runlog --test libtest pass: parses_a_recorded_cargo_test_run (events equal the expected list, binary context from Running/Doc-tests lines, planned count from 'running N tests'), failure_output_attaches_to_its_test, doc_tests_get_their_own_binary"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-runlog --test nextest passes: parses_recorded_libtest_json (the fixture was recorded from this repo with NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --message-format libtest-json)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-runlog --test assertion passes: current_and_legacy_assert_eq_forms, multiline_values_are_kept, a_non_assertion_panic_is_none"
    required: true
    manual: false
  - id: "C4"
    text: "scripts/check-no-gpui.sh passes (fe-runlog is headless)"
    required: true
    manual: false
---

# fe-runlog: parse cargo test and nextest output into test events, and a failed assertion into left and right (Mac T1573)

# fe-runlog: parse libtest and nextest output, and assertion failures

Spec: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
(Tests: a test tree, per-test results, "a failure shows the assertion with
left in red and right in green").

## Change: a new GPUI-free crate `crates/fe-runlog`

- `LibtestParser` (`src/libtest.rs`): `cargo test`'s human output, fed a line
  at a time (stable, no nightly flags): `Running unittests src/lib.rs (...)`,
  `Doc-tests <crate>` (the binary context of the ids that follow),
  `running N tests` (planned), `test a::b ... ok | FAILED | ignored`, the
  `---- name stdout ----` blocks (the failure's output, attached when the
  block ends), `test result: ...`.
- `NextestParser` (`src/nextest.rs`): `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1
  cargo nextest run --message-format libtest-json` lines (test started, ok,
  failed with stdout, ignored). The fixture is recorded from this repo's own
  nextest run on the Mac and checked in; the id is split as nextest names it.
- `TestEvent { id: TestId { binary, name }, kind: Planned(n) | Started |
  Passed { secs } | Failed { output } | Ignored }`.
- `Assertion` (`src/assertion.rs`): from a failed test's output, the
  `assert_eq!` / `assert_ne!` pair (`left`, `right`, the optional message and
  the op) in both the current multi-line form and the older backtick form;
  none when the panic is something else.
- Rust only. Go and pytest parsers are later cards; the runner (T1572) takes any
  parser.

## Not in this card

History, running anything, UI.

Platform: **Linux-OK**.

## Implementation notes

- Fixtures were recorded on helium (Linux, Rust 1.98.1, nextest 0.9.148)
  rather than the Mac, from this repo: a temporary `tests/sample.rs` and
  doc tests in `src/lib.rs` (pass, `assert_eq!` with a message, `assert_ne!`,
  a two-line `Debug`, a plain panic, ignored with and without a reason) run
  with `cargo test -p fe-runlog --no-fail-fast 2>&1` and with
  `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run -p fe-runlog
  --no-fail-fast --message-format libtest-json` (stdout only), then removed.
  `assert_current.txt` is the recorded `fails_eq` block; `assert_legacy.txt`
  is the same failure in the pre-1.73 form, written by hand from that
  version's `core::panicking::assert_failed_inner` (no older toolchain here).
- `TestId.binary` for `cargo test` is the `Running` line's artifact without
  cargo's hash (`fe_runlog`, `libtest`), and `Doc-tests <crate>` for doc
  tests; for nextest it is nextest's binary id (`fe-runlog::libtest`), split
  from the test name at `$`. `Planned` events have an empty test name.
- nextest's `suite started` line doesn't name its binary and can repeat for
  the same binary in one run (the fixture shows it): `NextestParser` reports
  a binary's `Planned` once, before that binary's first test event.
- `Passed { secs }` is an `Option<f64>`: libtest's human output has no time
  unless `--report-time` (nightly) appends `<0.003s>`, which is read.
- `cargo test` failures are reported when their `---- name stdout ----` block
  ends, or with empty output at the binary's `test result:` line.
- A `TestOutputParser` trait (`feed_line`, `finish`, `parse_all`) is what the
  runner (T1572) takes. `Cargo.lock` was added to scope for the new crate.
