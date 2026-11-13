//! nextest's libtest-json output. The fixture `nextest_libtest.jsonl` is a
//! real `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run -p fe-runlog
//! --no-fail-fast --message-format libtest-json` run's stdout, recorded with
//! a temporary `tests/sample.rs` whose tests pass, fail and are ignored.

use fe_runlog::{Assertion, NextestParser, TestEvent, TestEventKind, TestId, TestOutputParser};

const RECORDED: &str = include_str!("fixtures/nextest_libtest.jsonl");
const BINARY: &str = "fe-runlog::sample";

fn event(name: &str, kind: TestEventKind) -> TestEvent {
    TestEvent::new(TestId::new(BINARY, name), kind)
}

fn passed(name: &str, secs: f64) -> TestEvent {
    event(name, TestEventKind::Passed { secs: Some(secs) })
}

fn failed(name: &str, output: &str) -> TestEvent {
    event(
        name,
        TestEventKind::Failed {
            output: output.to_owned(),
        },
    )
}

#[test]
fn parses_recorded_libtest_json() {
    let events = NextestParser::new().parse_all(RECORDED);
    let backtrace =
        "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n";
    let expected = vec![
        // The binary's planned count, once, though nextest started its
        // suite twice.
        event("", TestEventKind::Planned(8)),
        event("ignored", TestEventKind::Started),
        event("ignored_with_reason", TestEventKind::Started),
        event("inner::nested_passes", TestEventKind::Started),
        event("fails_eq", TestEventKind::Started),
        event("fails_multiline", TestEventKind::Started),
        event("fails_ne", TestEventKind::Started),
        event("passes", TestEventKind::Started),
        event("panics_plainly", TestEventKind::Started),
        event("ignored", TestEventKind::Ignored),
        event("ignored_with_reason", TestEventKind::Ignored),
        passed("inner::nested_passes", 0.004599416),
        failed(
            "fails_ne",
            &format!(
                "\nthread 'fails_ne' (1316451) panicked at crates/fe-runlog/tests/sample.rs:12:5:\n\
                 assertion `left != right` failed\n  left: 2\n right: 2\n{backtrace}"
            ),
        ),
        failed(
            "panics_plainly",
            &format!(
                "\nthread 'panics_plainly' (1316452) panicked at crates/fe-runlog/tests/sample.rs:31:5:\n\
                 not an assertion\n{backtrace}"
            ),
        ),
        passed("passes", 0.00489961),
        failed(
            "fails_multiline",
            &format!(
                "\nthread 'fails_multiline' (1316456) panicked at crates/fe-runlog/tests/sample.rs:26:5:\n\
                 assertion `left == right` failed\n  left: first line\nsecond line\n right: first line\nsecond line\n{backtrace}"
            ),
        ),
        failed(
            "fails_eq",
            &format!(
                "some output first\n\nthread 'fails_eq' (1316457) panicked at crates/fe-runlog/tests/sample.rs:7:5:\n\
                 assertion `left == right` failed: lists differ for x\n  left: [1, 2]\n right: [1, 3]\n{backtrace}"
            ),
        ),
    ];
    assert_eq!(events, expected);

    // The failures' outputs carry their assertions.
    let assertions: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            TestEventKind::Failed { output } => Some((
                event.id.name.as_str(),
                Assertion::parse(output).map(|a| a.right),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        assertions,
        [
            ("fails_ne", Some("2".to_owned())),
            ("panics_plainly", None),
            (
                "fails_multiline",
                Some("first line\nsecond line".to_owned())
            ),
            ("fails_eq", Some("[1, 3]".to_owned())),
        ]
    );
}

#[test]
fn other_lines_are_skipped_and_names_split_at_the_dollar() {
    let events = NextestParser::new().parse_all(
        "info: experimental features enabled\n\
         {\"type\":\"suite\",\"event\":\"started\",\"test_count\":1}\n\
         {\"type\":\"test\",\"event\":\"ok\",\"name\":\"fastedit::bin/fastedit$cli::parses\",\"exec_time\":0.5}\n\
         {\"type\":\"suite\",\"event\":\"ok\",\"passed\":1}\n\
         {\"type\":\"test\",\"event\":\"ok\",\"name\":\"no_binary\"}\n",
    );
    assert_eq!(
        events,
        vec![
            TestEvent::new(
                TestId::new("fastedit::bin/fastedit", ""),
                TestEventKind::Planned(1)
            ),
            TestEvent::new(
                TestId::new("fastedit::bin/fastedit", "cli::parses"),
                TestEventKind::Passed { secs: Some(0.5) }
            ),
            TestEvent::new(
                TestId::new("", "no_binary"),
                TestEventKind::Passed { secs: None }
            ),
        ]
    );
}
