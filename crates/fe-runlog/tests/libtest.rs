//! `cargo test`'s human output. The fixture `libtest_mixed.txt` is a real
//! `cargo test -p fe-runlog --no-fail-fast 2>&1` run, recorded with a
//! temporary `tests/sample.rs` and doc tests that pass, fail and are ignored.

use fe_runlog::{LibtestParser, TestEvent, TestEventKind, TestId, TestOutputParser};

const MIXED: &str = include_str!("fixtures/libtest_mixed.txt");

fn event(binary: &str, name: &str, kind: TestEventKind) -> TestEvent {
    TestEvent::new(TestId::new(binary, name), kind)
}

fn planned(binary: &str, count: usize) -> TestEvent {
    event(binary, "", TestEventKind::Planned(count))
}

fn passed(binary: &str, name: &str) -> TestEvent {
    event(binary, name, TestEventKind::Passed { secs: None })
}

fn ignored(binary: &str, name: &str) -> TestEvent {
    event(binary, name, TestEventKind::Ignored)
}

fn failed(binary: &str, name: &str, output: &str) -> TestEvent {
    event(
        binary,
        name,
        TestEventKind::Failed {
            output: output.to_owned(),
        },
    )
}

#[test]
fn parses_a_recorded_cargo_test_run() {
    let events = LibtestParser::new().parse_all(MIXED);

    let doc = "Doc-tests fe_runlog";
    let expected = vec![
        planned("fe_runlog", 0),
        planned("sample", 8),
        ignored("sample", "ignored"),
        ignored("sample", "ignored_with_reason"),
        passed("sample", "inner::nested_passes"),
        passed("sample", "passes"),
        failed(
            "sample",
            "fails_eq",
            "some output first\n\
             \n\
             thread 'fails_eq' (1316142) panicked at crates/fe-runlog/tests/sample.rs:7:5:\n\
             assertion `left == right` failed: lists differ for x\n  \
             left: [1, 2]\n \
             right: [1, 3]\n\
             note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n",
        ),
        failed(
            "sample",
            "fails_multiline",
            "\n\
             thread 'fails_multiline' (1316143) panicked at crates/fe-runlog/tests/sample.rs:26:5:\n\
             assertion `left == right` failed\n  \
             left: first line\n\
             second line\n \
             right: first line\n\
             second line\n",
        ),
        failed(
            "sample",
            "fails_ne",
            "\n\
             thread 'fails_ne' (1316144) panicked at crates/fe-runlog/tests/sample.rs:12:5:\n\
             assertion `left != right` failed\n  \
             left: 2\n \
             right: 2\n",
        ),
        failed(
            "sample",
            "panics_plainly",
            "\n\
             thread 'panics_plainly' (1316146) panicked at crates/fe-runlog/tests/sample.rs:31:5:\n\
             not an assertion\n",
        ),
        planned(doc, 2),
        passed(doc, "crates/fe-runlog/src/lib.rs - (line 3)"),
        failed(
            doc,
            "crates/fe-runlog/src/lib.rs - (line 7)",
            "Test executable failed (exit status: 101).\n\
             \n\
             stderr:\n\
             \n\
             thread 'main' (1316188) panicked at /tmp/rustdoctestqplVvj/doctest_bundle_2024.rs:14:1:\n\
             assertion `left == right` failed: doc maths\n  \
             left: 2\n \
             right: 3\n\
             note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n",
        ),
        planned(doc, 1),
        ignored(doc, "crates/fe-runlog/src/lib.rs - (line 11)"),
    ];
    assert_eq!(events, expected);
}

#[test]
fn failure_output_attaches_to_its_test() {
    let mut parser = LibtestParser::new();
    let mut events = Vec::new();
    for line in [
        "running 3 tests",
        "test a::first ... FAILED",
        "test a::second ... FAILED",
        "test a::third ... ok",
        "",
        "failures:",
        "",
        "---- a::second stdout ----",
        "second's output",
        "",
        "---- a::first stdout ----",
        "first's output",
        "  with a line that looks like nothing in particular",
        "",
    ] {
        events.extend(parser.feed_line(line));
    }
    // a::first's block hasn't ended yet: its output may go on.
    assert_eq!(
        events,
        vec![
            planned("", 3),
            passed("", "a::third"),
            failed("", "a::second", "second's output\n"),
        ]
    );

    let mut rest = Vec::new();
    for line in [
        "",
        "failures:",
        "    a::first",
        "    a::second",
        "",
        "test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
    ] {
        rest.extend(parser.feed_line(line));
    }
    assert_eq!(
        rest,
        vec![failed(
            "",
            "a::first",
            "first's output\n  with a line that looks like nothing in particular\n"
        )]
    );
    assert_eq!(parser.finish(), vec![]);
}

#[test]
fn a_failure_without_output_is_reported_at_the_result() {
    let events = LibtestParser::new().parse_all(
        "running 1 test\n\
         test quiet ... FAILED\n\
         \n\
         failures:\n    \
         quiet\n\
         \n\
         test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n",
    );
    assert_eq!(events, vec![planned("", 1), failed("", "quiet", "")]);
}

#[test]
fn doc_tests_get_their_own_binary() {
    let events = LibtestParser::new().parse_all(MIXED);
    let binaries = |name: &str| -> Vec<&str> {
        events
            .iter()
            .filter(|event| event.id.name == name)
            .map(|event| event.id.binary.as_str())
            .collect()
    };
    assert_eq!(
        binaries("crates/fe-runlog/src/lib.rs - (line 3)"),
        ["Doc-tests fe_runlog"]
    );
    assert_eq!(binaries("passes"), ["sample"]);
    // Every doc test is in the doc-tests binary, and nothing else is.
    for event in &events {
        assert_eq!(
            event.id.binary == "Doc-tests fe_runlog",
            event.id.name.is_empty() && matches!(event.kind, TestEventKind::Planned(1 | 2))
                || event.id.name.starts_with("crates/fe-runlog/src/lib.rs - "),
            "{event:?}"
        );
    }
}

#[test]
fn uncaptured_output_and_modes() {
    let events = LibtestParser::new().parse_all(
        "     Running tests/a.rs (target/debug/deps/a-0123456789abcdef)\n\
         running 3 tests\n\
         test loud ... hello from the test\n\
         ok\n\
         test panicky - should panic ... ok\n\
         test timed ... ok <0.250s>\n",
    );
    assert_eq!(
        events,
        vec![
            planned("a", 3),
            event("a", "loud", TestEventKind::Started),
            passed("a", "loud"),
            passed("a", "panicky"),
            event("a", "timed", TestEventKind::Passed { secs: Some(0.25) }),
        ]
    );
}

#[test]
fn coloured_cargo_lines_are_read() {
    let events = LibtestParser::new().parse_all(
        "\x1b[1m\x1b[92m     Running\x1b[0m unittests src/lib.rs (target/debug/deps/x-0123456789abcdef)\r\n\
         running 1 test\r\n\
         test t ... \x1b[32mok\x1b[0m\r\n",
    );
    assert_eq!(events, vec![planned("x", 1), passed("x", "t")]);
}
