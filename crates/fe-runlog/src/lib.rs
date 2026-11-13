//! Test runners' output, read into test events, and a failed assertion read
//! into its two sides (the design: `docs/design/design-system.md`, "Runs,
//! builds, tests and agent tasks in one list").
//!
//! A runner feeds a parser its process's output a line at a time and gets
//! [`TestEvent`]s back. [`LibtestParser`] reads `cargo test`'s human output
//! (stable Rust, no nightly flags); [`NextestParser`] reads
//! `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --message-format
//! libtest-json`. Both are [`TestOutputParser`]s, so a runner can take either.
//! [`Assertion::parse`] reads a failed test's output for the `assert_eq!` or
//! `assert_ne!` that failed, for showing left and right side by side.
//!
//! Nothing here runs a process, keeps history or draws anything.

mod assertion;
mod libtest;
mod nextest;

pub use assertion::{AssertOp, Assertion};
pub use libtest::LibtestParser;
pub use nextest::NextestParser;

/// Which test an event is about.
///
/// `binary` is the test binary as the runner names it, so the same test name
/// in two binaries are two tests. A [`TestEventKind::Planned`] event's `name`
/// is empty: it is about the binary as a whole.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TestId {
    /// The test binary: nextest's binary id (`fe-runlog::libtest`), or for
    /// `cargo test` the artifact named in its `Running` line, without the
    /// hash (`fe_runlog`, `libtest`), or `Doc-tests <crate>`. Empty when the
    /// output never said.
    pub binary: String,
    /// The test's name within its binary (`a::b`), or for a doc test what
    /// rustdoc calls it (`src/lib.rs - parse (line 5)`).
    pub name: String,
}

impl TestId {
    /// The id of `name` in `binary`.
    pub fn new(binary: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            name: name.into(),
        }
    }
}

/// One thing a test run said about a test, or about a binary's tests.
#[derive(Clone, Debug, PartialEq)]
pub struct TestEvent {
    /// The test, or for [`TestEventKind::Planned`] the binary (an empty name).
    pub id: TestId,
    /// What happened.
    pub kind: TestEventKind,
}

impl TestEvent {
    /// An event of `kind` about `id`.
    pub fn new(id: TestId, kind: TestEventKind) -> Self {
        Self { id, kind }
    }
}

/// What a [`TestEvent`] says.
#[derive(Clone, Debug, PartialEq)]
pub enum TestEventKind {
    /// The binary is about to run this many tests (ignored ones included,
    /// filtered-out ones not).
    Planned(usize),
    /// The test has started. `cargo test` says so only when a test's output
    /// isn't captured (`--nocapture` with one thread); nextest always does.
    Started,
    /// The test passed, in `secs` seconds when the runner said.
    Passed {
        /// How long it took, when the runner reported it.
        secs: Option<f64>,
    },
    /// The test failed. `output` is what it printed, the panic message
    /// included ([`Assertion::parse`] reads it); it can be empty when the
    /// runner showed none.
    Failed {
        /// The test's captured output.
        output: String,
    },
    /// The test was ignored and didn't run.
    Ignored,
}

/// Reads one runner's output, a line at a time, into [`TestEvent`]s.
pub trait TestOutputParser {
    /// Reads one line of output, with or without its line ending, and returns
    /// the events it completes (often none). An event can come some lines
    /// after the line that started it: a `cargo test` failure is reported
    /// when its output has been read.
    fn feed_line(&mut self, line: &str) -> Vec<TestEvent>;

    /// The output has ended: returns the events still held back, such as a
    /// failure whose output never came.
    fn finish(&mut self) -> Vec<TestEvent>;

    /// Feeds every line of `text`, then [`finish`](Self::finish)es.
    fn parse_all(&mut self, text: &str) -> Vec<TestEvent> {
        let mut events = Vec::new();
        for line in text.lines() {
            events.extend(self.feed_line(line));
        }
        events.extend(self.finish());
        events
    }
}

/// `line` without its line ending and without ANSI colour codes, which cargo
/// writes when it is told to colour a pipe.
pub(crate) fn clean_line(line: &str) -> std::borrow::Cow<'_, str> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    if !line.contains('\x1b') {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        // A CSI sequence: ESC [ parameters... final byte in @..~.
        if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    std::borrow::Cow::Owned(out)
}
