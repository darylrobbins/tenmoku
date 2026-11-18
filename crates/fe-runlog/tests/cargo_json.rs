//! cargo's JSON build messages. `cargo_build.jsonl` and `cargo_build.stderr`
//! are the stdout and stderr of a real `cargo build -p fe-runlog
//! --message-format=json-render-diagnostics` run whose dependencies were built
//! already and whose own crate was touched, with an unused variable added to it
//! (the warning). `cargo_timing.jsonl` is that stdout with `timing-info`
//! records, in cargo's shape, before each rebuilt crate's artifact: stable
//! cargo can't emit them.

use std::{path::PathBuf, time::Duration};

use fe_runlog::{BuildTimeline, CargoParser, CrateSpan};

const BUILD: &str = include_str!("fixtures/cargo_build.jsonl");
const STDERR: &str = include_str!("fixtures/cargo_build.stderr");
const TIMING: &str = include_str!("fixtures/cargo_timing.jsonl");

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn parse(stdout: &str, stderr: &str) -> BuildTimeline {
    let mut parser = CargoParser::new();
    // The `Compiling` line comes first, at 100 ms; the rest of stdout lands
    // 400 ms in.
    for (i, line) in stderr.lines().enumerate() {
        parser.feed_stderr(line, ms(100 + i as u64));
    }
    for line in stdout.lines() {
        parser.feed_stdout(line, ms(400));
    }
    parser.finish()
}

#[test]
fn parses_a_recorded_build_into_spans() {
    let timeline = parse(BUILD, STDERR);
    assert!(timeline.success);
    let names: Vec<_> = timeline.spans.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "fe-runlog",
            "zmij",
            "serde_core",
            "serde_json",
            "itoa",
            "memchr"
        ]
    );
    for span in &timeline.spans {
        if span.name == "fe-runlog" {
            assert!(!span.fresh && span.local);
            assert_eq!((span.start, span.end), (ms(100), ms(400)));
            assert_eq!(span.version, "0.1.5");
        } else {
            assert!(span.fresh && !span.local, "{}", span.name);
            assert_eq!(span.start, span.end, "{}", span.name);
            assert_eq!(span.duration(), Duration::ZERO);
        }
    }
}

#[test]
fn timing_info_wins_when_present() {
    let timeline = parse(TIMING, STDERR);
    let local = timeline.spans.iter().find(|s| s.local).unwrap();
    assert_eq!(local.start, ms(100));
    assert_eq!(local.end, ms(1350));
    assert!(
        timeline
            .spans
            .iter()
            .filter(|s| s.fresh)
            .all(|s| s.duration().is_zero())
    );
}

#[test]
fn warning_and_error_counts() {
    // The recorded build's one warning is rendered on stderr.
    let timeline = parse(BUILD, STDERR);
    assert_eq!((timeline.warnings, timeline.errors), (1, 0));
    let warning = &timeline.messages[0];
    assert_eq!(warning.file.as_deref(), Some("crates/fe-runlog/src/lib.rs"));
    assert_eq!((warning.line, warning.column), (Some(143), Some(26)));
    // The whole diagnostic, up to the summary line.
    assert!(
        warning
            .rendered
            .starts_with("warning: unused variable: `x`\n")
    );
    assert!(warning.rendered.contains("^ help: if this is intentional"));
    assert!(
        warning.rendered.ends_with("on by default"),
        "{}",
        warning.rendered
    );

    // Plain `--message-format=json` carries them as compiler-messages.
    let mut parser = CargoParser::new();
    let id = "path+file:///w/crates/a#0.1.0";
    let message = |level: &str, text: &str, spans: &str| {
        format!(
            r#"{{"reason":"compiler-message","package_id":"{id}","message":{{"message":"{text}","level":"{level}","spans":[{spans}],"rendered":"{level}: {text}\n"}}}}"#
        )
    };
    let span = r#"{"file_name":"src/lib.rs","line_start":3,"column_start":9,"is_primary":true}"#;
    for line in [
        message("warning", "unused variable", span),
        message("error", "mismatched types", span),
        message("error", "aborting due to 1 previous error", ""),
        message("warning", "1 warning emitted", ""),
        r#"{"reason":"build-finished","success":false}"#.to_owned(),
    ] {
        parser.feed_stdout(&line, ms(5));
    }
    let timeline = parser.finish();
    assert_eq!((timeline.warnings, timeline.errors), (1, 1));
    assert!(!timeline.success);
    let error = &timeline.messages[1];
    assert_eq!(error.file.as_deref(), Some("src/lib.rs"));
    assert_eq!((error.line, error.column), (Some(3), Some(9)));
    // A crate that failed has no artifact, but it isn't fresh.
    assert!(!timeline.spans[0].fresh && timeline.spans[0].name == "a");
}

#[test]
fn malformed_lines_are_skipped() {
    let mut parser = CargoParser::new();
    for line in [
        "",
        "not json",
        "{\"reason\":",
        "[1,2]",
        "{\"reason\":7}",
        "{\"reason\":\"x\"}",
    ] {
        parser.feed_stdout(line, ms(1));
        parser.feed_stderr(line, ms(1));
    }
    parser.feed_stderr("Compiling", ms(1));
    parser.feed_stdout(r#"{"reason":"build-finished","success":true}"#, ms(2));
    let timeline = parser.finish();
    assert!(timeline.spans.is_empty() && timeline.success);
    assert_eq!((timeline.warnings, timeline.errors), (0, 0));
}

const REG: &str = "registry+https://github.com/rust-lang/crates.io-index#dup@1.0.0";
const GIT: &str = "git+https://example.com/dup#dup@1.0.0";

fn artifact(id: &str, fresh: bool) -> String {
    format!(r#"{{"reason":"compiler-artifact","package_id":"{id}","fresh":{fresh}}}"#)
}

#[test]
fn a_cached_warning_does_not_make_a_fresh_crate_rebuilt() {
    let mut parser = CargoParser::new();
    let id = "path+file:///w/crates/a#0.1.0";
    let warning = format!(
        r#"{{"reason":"compiler-message","package_id":"{id}","message":{{"message":"unused","level":"warning","spans":[],"rendered":"warning: unused\n"}}}}"#
    );
    parser.feed_stdout(&warning, ms(10));
    parser.feed_stdout(&artifact(id, true), ms(12));
    let timeline = parser.finish();
    assert_eq!(timeline.warnings, 1);
    let span = &timeline.spans[0];
    assert!(span.fresh && span.duration().is_zero());
}

#[test]
fn packages_sharing_a_name_and_version_stay_apart() {
    let mut parser = CargoParser::new();
    parser.feed_stderr("   Compiling dup v1.0.0", ms(1));
    parser.feed_stderr(
        "   Compiling dup v1.0.0 (https://example.com/dup#abc)",
        ms(2),
    );
    parser.feed_stdout(&artifact(REG, false), ms(5));
    parser.feed_stdout(&artifact(GIT, false), ms(9));
    parser.feed_stdout(&artifact("path+file:///w/dup#1.0.0", false), ms(11));
    let timeline = parser.finish();
    assert_eq!(timeline.spans.len(), 3);
    assert_eq!(
        (timeline.spans[0].start, timeline.spans[0].end),
        (ms(1), ms(5))
    );
    assert!(!timeline.spans[0].local);
    assert!(timeline.spans[2].local);
    assert_eq!(timeline.spans[2].start, ms(11));
}

#[test]
fn real_diagnostics_that_mention_generated_or_emitted_count() {
    let mut parser = CargoParser::new();
    for line in [
        "warning: this code is generated by a macro and emitted twice",
        "  --> src/lib.rs:1:1",
        "error[E0599]: no method named `generated` found",
        "warning: `a` (lib) generated 1 warning",
        "error: could not compile `a` (lib) due to 1 previous error; 1 warning emitted",
    ] {
        parser.feed_stderr(line, ms(1));
    }
    let timeline = parser.finish();
    assert_eq!((timeline.warnings, timeline.errors), (1, 1));
}

#[test]
fn out_of_range_timings_are_skipped() {
    let mut parser = CargoParser::new();
    let id = "path+file:///w/a#0.1.0";
    let timing =
        |secs: &str| format!(r#"{{"reason":"timing-info","package_id":"{id}","duration":{secs}}}"#);
    parser.feed_stderr("   Compiling a v0.1.0 (/w/a)", ms(0));
    for secs in ["1e30", "-1", "1.8e19", "1.8e19"] {
        parser.feed_stdout(&timing(secs), ms(1));
    }
    parser.feed_stdout(&artifact(id, false), ms(3));
    let span = &parser.finish().spans[0];
    // The second 1.8e19 sums past what a Duration holds and is skipped, as
    // are the one too big to convert and the negative one.
    assert_eq!(span.duration(), Duration::from_secs_f64(1.8e19));
}

#[test]
fn a_build_script_artifact_before_compiling_keeps_one_span() {
    let mut parser = CargoParser::new();
    let id = "path+file:///w/a#a@1.0.0";
    parser.feed_stdout(&artifact(id, true), ms(1));
    parser.feed_stderr("   Compiling a v1.0.0 (/w/a)", ms(4));
    parser.feed_stdout(&artifact(id, false), ms(9));
    let timeline = parser.finish();
    assert_eq!(timeline.spans.len(), 1);
    let span = &timeline.spans[0];
    assert!(!span.fresh && span.local);
    assert_eq!((span.start, span.end), (ms(4), ms(9)));
}

#[test]
fn a_path_with_spaces_matches_its_encoded_id() {
    let mut parser = CargoParser::new();
    parser.feed_stderr("   Compiling a v1.0.0 (/w/my project/a)", ms(2));
    parser.feed_stdout(
        &artifact("path+file:///w/my%20project/a#a@1.0.0", false),
        ms(7),
    );
    let timeline = parser.finish();
    assert_eq!(timeline.spans.len(), 1);
    assert_eq!(
        (timeline.spans[0].start, timeline.spans[0].end),
        (ms(2), ms(7))
    );
}

#[test]
fn a_diagnostic_keeps_lines_that_start_with_a_capital() {
    let mut parser = CargoParser::new();
    for line in [
        "warning: odd",
        "  --> src/lib.rs:1:1",
        "   |",
        "   = note: first",
        "   Second line of the note",
        "",
        "   Finished `dev` profile in 1s",
        "   After the build",
    ] {
        parser.feed_stderr(line, ms(1));
    }
    let message = &parser.finish().messages[0];
    assert!(message.rendered.ends_with("Second line of the note"));
    assert!(!message.rendered.contains("Finished"));
}

#[test]
fn a_snapshot_marks_the_crates_being_compiled() {
    let mut parser = CargoParser::new();
    let a = "path+file:///w/a#a@1.0.0";
    parser.feed_stdout(&artifact(REG, true), ms(1));
    parser.feed_stderr("   Compiling a v1.0.0 (/w/a)", ms(4));
    parser.feed_stderr("   Compiling b v1.0.0 (/w/b)", ms(5));
    let running = |timeline: &BuildTimeline| -> Vec<(String, bool)> {
        (timeline.spans.iter())
            .map(|span| (span.name.clone(), span.running))
            .collect()
    };
    let snapshot = parser.snapshot();
    assert_eq!(
        running(&snapshot),
        [
            ("dup".to_owned(), false),
            ("a".to_owned(), true),
            ("b".to_owned(), true)
        ]
    );
    assert!(snapshot.spans[0].fresh && !snapshot.spans[1].fresh);

    parser.feed_stdout(&artifact(a, false), ms(9));
    let snapshot = parser.snapshot();
    assert_eq!(running(&snapshot)[1], ("a".to_owned(), false));
    assert_eq!(
        (snapshot.spans[1].start, snapshot.spans[1].end),
        (ms(4), ms(9))
    );
    // A snapshot leaves the parser as it was.
    let timeline = parser.finish();
    assert!(timeline.spans.iter().all(|span| !span.running));
    assert_eq!(
        timeline.spans[1],
        CrateSpan {
            running: false,
            ..snapshot.spans[1].clone()
        }
    );
}

#[test]
fn a_local_crate_has_its_folder() {
    let mut parser = CargoParser::new();
    parser.feed_stdout(
        &artifact("path+file:///w/my%20project/a#a@1.0.0", false),
        ms(1),
    );
    parser.feed_stdout(&artifact(REG, false), ms(2));
    parser.feed_stdout(&artifact(GIT, false), ms(3));
    // One that failed has no artifact to name it: its line names its folder.
    parser.feed_stderr("   Compiling c v0.1.0 (/w/c)", ms(4));
    parser.feed_stderr("   Compiling d v0.1.0 (https://example.com/d#abc)", ms(5));
    let timeline = parser.finish();
    let dirs: Vec<(bool, Option<PathBuf>)> = (timeline.spans.iter())
        .map(|span| (span.local, span.dir.clone()))
        .collect();
    assert_eq!(
        dirs,
        [
            (true, Some(PathBuf::from("/w/my project/a"))),
            (false, None),
            (false, None),
            (true, Some(PathBuf::from("/w/c"))),
            (false, None),
        ]
    );
}
