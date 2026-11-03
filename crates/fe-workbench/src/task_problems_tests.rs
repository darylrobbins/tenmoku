//! Task output matchers over fake paths.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::task_defs::Source;
use crate::task_problems::{MAX_PROBLEMS, MatchResult, Matcher, Severity, match_output};

const ROOT: &str = "/proj";

fn run(matcher: Matcher, text: &str) -> MatchResult {
    match_output(matcher, text, Path::new(ROOT), Path::new(ROOT), &|_| true)
}

fn one(result: &MatchResult, path: &str, l: u32, c: u32, sev: Severity, msg: &str) {
    assert_eq!(result.problems.len(), 1, "{result:?}");
    let p = &result.problems[0];
    assert_eq!(p.path, PathBuf::from(ROOT).join(path));
    assert_eq!(
        (p.line, p.column, p.severity, p.message.as_str()),
        (l, c, sev, msg)
    );
}

#[test]
fn rustc_blocks_are_matched() {
    let text = "error[E0308]: mismatched types\n --> src/main.rs:12:5\n  |\n12 | x\n";
    one(
        &run(Matcher::Rustc, text),
        "src/main.rs",
        12,
        5,
        Severity::Error,
        "mismatched types (E0308)",
    );
    let text = "warning: unused variable: `x`\n --> src/lib.rs:3:9\n";
    one(
        &run(Matcher::Rustc, text),
        "src/lib.rs",
        3,
        9,
        Severity::Warning,
        "unused variable: `x`",
    );
    let text = "  = help: try this\nnote: something\n --> src/lib.rs:3:9\nhelp: do it\n";
    assert!(run(Matcher::Rustc, text).problems.is_empty());
}

#[test]
fn single_line_matchers() {
    one(
        &run(Matcher::Tsc, "src/a.ts(3,7): error TS2322: msg"),
        "src/a.ts",
        3,
        7,
        Severity::Error,
        "msg",
    );
    one(
        &run(Matcher::Gcc, "a.c:10:3: warning: msg [-Wx]"),
        "a.c",
        10,
        3,
        Severity::Warning,
        "msg [-Wx]",
    );
    one(
        &run(Matcher::Gcc, "a.c:10:3: error: bad"),
        "a.c",
        10,
        3,
        Severity::Error,
        "bad",
    );
    one(
        &run(Matcher::Go, "main.go:5:2: undefined: x"),
        "main.go",
        5,
        2,
        Severity::Error,
        "undefined: x",
    );
    one(
        &run(Matcher::Eslint, "a.js:1:2: msg [Error/rule]"),
        "a.js",
        1,
        2,
        Severity::Error,
        "msg",
    );
    one(
        &run(Matcher::Eslint, "a.js:1:2: msg [Warning/rule]"),
        "a.js",
        1,
        2,
        Severity::Warning,
        "msg",
    );
}

#[test]
fn python_traceback_uses_the_last_frame() {
    let text = "Traceback (most recent call last):\n  File \"x.py\", line 9, in <module>\n    f()\n  File \"y.py\", line 4, in f\n    raise ValueError('bad')\nValueError: bad\n";
    one(
        &run(Matcher::Python, text),
        "y.py",
        4,
        1,
        Severity::Error,
        "ValueError: bad",
    );
}

#[test]
fn ansi_and_paths() {
    let text = "\x1b[1;31merror\x1b[0m: boom\n \x1b[34m-->\x1b[0m src/a.rs:1:2\n";
    let sub = Path::new("/proj/sub");
    let r = match_output(Matcher::Rustc, text, sub, Path::new(ROOT), &|_| true);
    assert_eq!(r.problems.len(), 1);
    assert_eq!(r.problems[0].path, PathBuf::from("/proj/sub/src/a.rs"));
    let outside = "error: boom\n --> ../../etc/x.rs:1:2\n";
    assert!(
        match_output(Matcher::Rustc, outside, sub, Path::new(ROOT), &|_| true)
            .problems
            .is_empty()
    );
    let missing = match_output(Matcher::Rustc, text, sub, Path::new(ROOT), &|_| false);
    assert!(missing.problems.is_empty());
}

#[test]
fn unknown_matcher_and_noise() {
    assert!(Matcher::from_name("nope").is_err());
    assert_eq!(Matcher::from_name("go"), Ok(Matcher::Go));
    assert_eq!(Matcher::default_for(Source::Cargo), Some(Matcher::Rustc));
    assert_eq!(Matcher::default_for(Source::Npm), None);
    let noise = "just some output: nothing here 12\n".repeat(20_000);
    let start = Instant::now();
    for m in [
        Matcher::Rustc,
        Matcher::Tsc,
        Matcher::Gcc,
        Matcher::Go,
        Matcher::Eslint,
        Matcher::Python,
    ] {
        assert!(run(m, &noise).problems.is_empty());
    }
    assert!(start.elapsed() < Duration::from_secs(1));
    let many = "a.c:1:1: error: x\n".repeat(MAX_PROBLEMS + 10);
    let r = run(Matcher::Gcc, &many);
    assert_eq!(r.problems.len(), MAX_PROBLEMS);
    assert!(r.truncated);
}
