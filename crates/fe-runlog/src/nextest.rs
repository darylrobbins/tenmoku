//! `cargo nextest run --message-format libtest-json`'s output (with
//! `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`): a JSON object per line.
//!
//! ```text
//! {"type":"suite","event":"started","test_count":8}
//! {"type":"test","event":"started","name":"fe-runlog::sample$fails_ne"}
//! {"type":"test","event":"failed","name":"fe-runlog::sample$fails_ne","exec_time":0.004,"stdout":"…"}
//! {"type":"suite","event":"failed","passed":2,"failed":2,…}
//! ```
//!
//! A test's name is nextest's binary id and the test's name joined by `$`.
//! A suite is one binary's tests, but its `started` line doesn't name the
//! binary, and nextest can start a binary's suite more than once in a run
//! (when its running tests drain and more start): the binary's planned count
//! is reported once, before its first test's event.

use std::collections::HashSet;

use serde_json::Value;

use crate::{TestEvent, TestEventKind, TestId, TestOutputParser, clean_line};

/// Reads nextest's libtest-json lines (see the module docs) into
/// [`TestEvent`]s. Lines that aren't JSON objects are skipped.
#[derive(Debug, Default)]
pub struct NextestParser {
    /// The `test_count` of the suite just started: its binary is that of the
    /// next test event (nextest starts a suite as it starts its first test).
    suite_count: Option<usize>,
    /// The binaries whose planned count has been reported.
    planned: HashSet<String>,
}

impl NextestParser {
    /// A parser at the start of the output.
    pub fn new() -> Self {
        Self::default()
    }
}

impl TestOutputParser for NextestParser {
    fn feed_line(&mut self, line: &str) -> Vec<TestEvent> {
        let line = clean_line(line);
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let field = |key: &str| message.get(key).and_then(Value::as_str);
        match (field("type"), field("event")) {
            (Some("suite"), Some("started")) => {
                self.suite_count = message
                    .get("test_count")
                    .and_then(Value::as_u64)
                    .and_then(|count| usize::try_from(count).ok());
                Vec::new()
            }
            (Some("suite"), _) => {
                self.suite_count = None;
                Vec::new()
            }
            (Some("test"), Some(event)) => {
                let Some(name) = field("name") else {
                    return Vec::new();
                };
                let id = split_name(name);
                let kind = match event {
                    "started" => TestEventKind::Started,
                    "ok" => TestEventKind::Passed {
                        secs: message.get("exec_time").and_then(Value::as_f64),
                    },
                    "failed" | "timeout" => TestEventKind::Failed {
                        output: [field("stdout"), field("stderr")]
                            .into_iter()
                            .flatten()
                            .collect(),
                    },
                    "ignored" => TestEventKind::Ignored,
                    _ => return Vec::new(),
                };
                let mut events = Vec::new();
                if let Some(count) = self.suite_count.take()
                    && self.planned.insert(id.binary.clone())
                {
                    events.push(TestEvent::new(
                        TestId::new(id.binary.clone(), ""),
                        TestEventKind::Planned(count),
                    ));
                }
                events.push(TestEvent::new(id, kind));
                events
            }
            _ => Vec::new(),
        }
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        self.suite_count = None;
        Vec::new()
    }
}

/// `fe-runlog::libtest$a::b` as binary `fe-runlog::libtest`, test `a::b`.
fn split_name(name: &str) -> TestId {
    match name.split_once('$') {
        Some((binary, test)) => TestId::new(binary, test),
        None => TestId::new("", name),
    }
}
