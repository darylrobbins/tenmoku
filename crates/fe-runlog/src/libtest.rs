//! `cargo test`'s human output.
//!
//! What it reads, in the order cargo and libtest write it:
//!
//! ```text
//!      Running unittests src/lib.rs (target/debug/deps/fe_runlog-7689c8cf0503661c)
//! running 3 tests
//! test a::passes ... ok
//! test a::fails ... FAILED
//! test a::later ... ignored, needs a network
//! failures:
//! ---- a::fails stdout ----
//! thread 'a::fails' panicked at src/lib.rs:7:5:
//! …
//! failures:
//!     a::fails
//! test result: FAILED. 1 passed; 1 failed; 1 ignored; …
//!    Doc-tests fe_runlog
//! running 1 test
//! test src/lib.rs - parse (line 5) ... ok
//! ```
//!
//! The `Running` and `Doc-tests` lines are cargo's (on stderr); the rest is
//! the test binary's (on stdout). A failure is reported once its
//! `---- name stdout ----` block has been read, or with no output at the
//! binary's `test result:` line if it had none.

use crate::{TestEvent, TestEventKind, TestId, TestOutputParser, clean_line};

/// Reads `cargo test`'s human output (see the module docs) into
/// [`TestEvent`]s.
#[derive(Debug, Default)]
pub struct LibtestParser {
    /// The binary the tests now being reported belong to.
    binary: String,
    /// Tests reported `FAILED` whose output hasn't been read yet.
    failed: Vec<String>,
    /// The `---- name stdout ----` block being read.
    block: Option<Block>,
    /// A test whose `test name ... ` line its own uncaptured output cut
    /// short: its result comes on a later line of its own.
    awaiting: Option<String>,
}

#[derive(Debug)]
struct Block {
    name: String,
    lines: Vec<String>,
}

impl LibtestParser {
    /// A parser at the start of the output.
    pub fn new() -> Self {
        Self::default()
    }

    fn id(&self, name: &str) -> TestId {
        TestId::new(self.binary.clone(), name)
    }

    /// Ends the block being read: reports its test's failure with the block
    /// as the output, if it was a failure (with `--show-output`, passing
    /// tests have blocks too).
    fn end_block(&mut self) -> Vec<TestEvent> {
        let Some(block) = self.block.take() else {
            return Vec::new();
        };
        let Some(at) = self.failed.iter().position(|name| *name == block.name) else {
            return Vec::new();
        };
        self.failed.remove(at);
        // libtest writes the output, then a newline to separate it from the
        // next block (two before the closing `failures:` list): the blank
        // lines at the end are its, not the test's.
        let mut lines = block.lines;
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let mut output = lines.join("\n");
        if !output.is_empty() {
            output.push('\n');
        }
        vec![TestEvent::new(
            self.id(&block.name),
            TestEventKind::Failed { output },
        )]
    }

    /// Reports the failures whose output never came, and forgets an
    /// unfinished block and result: the binary's report has ended.
    fn end_binary(&mut self) -> Vec<TestEvent> {
        let mut events = self.end_block();
        for name in std::mem::take(&mut self.failed) {
            events.push(TestEvent::new(
                self.id(&name),
                TestEventKind::Failed {
                    output: String::new(),
                },
            ));
        }
        self.awaiting = None;
        events
    }

    /// The events of a test's result, `rest` being what follows its
    /// `test name ... `.
    fn result(&mut self, name: &str, rest: &str) -> Vec<TestEvent> {
        let (rest, secs) = split_time(rest);
        let kind = match rest {
            "ok" => TestEventKind::Passed { secs },
            "FAILED" => {
                self.failed.push(name.to_owned());
                return Vec::new();
            }
            "ignored" => TestEventKind::Ignored,
            _ if rest.starts_with("ignored, ") => TestEventKind::Ignored,
            // A benchmark run as a test (`cargo test` runs `#[bench]`es
            // once): it passed.
            _ if rest.starts_with("bench:") => TestEventKind::Passed { secs },
            _ => {
                // Not a result: the test's own output, uncaptured, after
                // the `test name ... ` libtest wrote when it started.
                self.awaiting = Some(name.to_owned());
                return vec![TestEvent::new(self.id(name), TestEventKind::Started)];
            }
        };
        vec![TestEvent::new(self.id(name), kind)]
    }
}

impl TestOutputParser for LibtestParser {
    fn feed_line(&mut self, line: &str) -> Vec<TestEvent> {
        let line = clean_line(line);
        let line = line.as_ref();
        let mut events = Vec::new();

        if let Some(block) = &mut self.block {
            if !ends_block(line) {
                block.lines.push(line.to_owned());
                return events;
            }
            events.extend(self.end_block());
        }

        if let Some(rest) = cargo_status(line, "Running") {
            events.extend(self.end_binary());
            self.binary = binary_from_running(rest);
        } else if let Some(krate) = cargo_status(line, "Doc-tests") {
            events.extend(self.end_binary());
            self.binary = format!("Doc-tests {}", krate.trim());
        } else if let Some(planned) = planned(line) {
            events.push(TestEvent::new(self.id(""), TestEventKind::Planned(planned)));
        } else if let Some(name) = block_header(line) {
            self.block = Some(Block {
                name: name.to_owned(),
                lines: Vec::new(),
            });
        } else if line.starts_with("test result: ") {
            events.extend(self.end_binary());
        } else if let Some((name, rest)) = result_line(line) {
            self.awaiting = None;
            events.extend(self.result(name, rest));
        } else if let Some(name) = self.awaiting.clone()
            && is_result(line)
        {
            self.awaiting = None;
            events.extend(self.result(&name, line));
        }
        events
    }

    fn finish(&mut self) -> Vec<TestEvent> {
        self.end_binary()
    }
}

/// What follows cargo's right-aligned status `word` (`     Running …`), if
/// `line` is that status line.
fn cargo_status<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let rest = line.trim_start_matches(' ');
    // Cargo right-aligns its status words in 12 columns.
    if line.len() - rest.len() != 12usize.saturating_sub(word.len()) {
        return None;
    }
    rest.strip_prefix(word)?.strip_prefix(' ')
}

/// The binary a `Running` line names: its artifact's file name without
/// cargo's hash (`fe_runlog`, `libtest`), or what the line says if it names
/// no file.
fn binary_from_running(rest: &str) -> String {
    let rest = rest.trim();
    // `unittests src/lib.rs (target/debug/deps/fe_runlog-7689c8cf0503661c)`;
    // older cargo wrote the path alone.
    let path = match rest.strip_suffix(')').and_then(|r| r.rsplit_once(" (")) {
        Some((_, path)) => path,
        None => rest,
    };
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let file = file.strip_suffix(".exe").unwrap_or(file);
    match file.rsplit_once('-') {
        Some((stem, hash))
            if !stem.is_empty()
                && hash.len() >= 8
                && hash.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            stem.to_owned()
        }
        _ if file.is_empty() => rest.to_owned(),
        _ => file.to_owned(),
    }
}

/// The count in `running 3 tests` or `running 1 test`.
fn planned(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("running ")?;
    let count = rest
        .strip_suffix(" tests")
        .or_else(|| rest.strip_suffix(" test"))?;
    count.parse().ok()
}

/// The test named in a `---- name stdout ----` line.
fn block_header(line: &str) -> Option<&str> {
    line.strip_prefix("---- ")?.strip_suffix(" stdout ----")
}

/// Whether `line` ends the output block being read: the next block, or the
/// list of failures or successes, or the binary's result.
fn ends_block(line: &str) -> bool {
    block_header(line).is_some()
        || line == "failures:"
        || line == "successes:"
        || line.starts_with("test result: ")
        || cargo_status(line, "Running").is_some()
        || cargo_status(line, "Doc-tests").is_some()
}

/// The test name and what follows it in `test name ... ok`.
fn result_line(line: &str) -> Option<(&str, &str)> {
    let (name, rest) = line.strip_prefix("test ")?.split_once(" ...")?;
    // The modes libtest appends to the name when it reports the result.
    let name = name
        .strip_suffix(" - should panic")
        .or_else(|| name.strip_suffix(" - compile fail"))
        .unwrap_or(name);
    Some((name.trim_end(), rest.trim()))
}

/// Whether `line` is a bare result, as it comes after a test's uncaptured
/// output.
fn is_result(line: &str) -> bool {
    let (line, _) = split_time(line.trim());
    matches!(line, "ok" | "FAILED" | "ignored") || line.starts_with("ignored, ")
}

/// `rest` without the `<0.003s>` `--report-time` appends, and those seconds.
fn split_time(rest: &str) -> (&str, Option<f64>) {
    let Some((before, time)) = rest.rsplit_once(" <") else {
        return (rest, None);
    };
    match time.strip_suffix("s>").and_then(|secs| secs.parse().ok()) {
        Some(secs) => (before.trim_end(), Some(secs)),
        None => (rest, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_lines_name_the_artifact() {
        assert_eq!(
            binary_from_running(
                "unittests src/lib.rs (target/debug/deps/fe_runlog-7689c8cf0503661c)"
            ),
            "fe_runlog"
        );
        assert_eq!(
            binary_from_running(r"tests\a.rs (target\debug\deps\a_b-0123456789abcdef.exe)"),
            "a_b"
        );
        assert_eq!(
            binary_from_running("target/debug/deps/old-0123456789abcdef"),
            "old"
        );
    }

    #[test]
    fn report_time_is_read() {
        assert_eq!(split_time("ok <0.25s>"), ("ok", Some(0.25)));
        assert_eq!(split_time("ok"), ("ok", None));
    }

    #[test]
    fn status_words_must_be_cargo_aligned() {
        assert_eq!(cargo_status("     Running x", "Running"), Some("x"));
        assert_eq!(cargo_status("   Doc-tests c", "Doc-tests"), Some("c"));
        assert_eq!(cargo_status("Running x", "Running"), None);
    }
}
