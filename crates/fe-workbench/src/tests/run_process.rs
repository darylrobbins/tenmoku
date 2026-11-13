//! The background runner for runs ([`crate::run_process`]): real processes,
//! each an `sh -c` script, their lines read by a sink that records them.

use std::{
    ffi::OsString,
    path::Path,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::{
    run_process::{
        BATCH_CAP, BATCH_INTERVAL, GRACE, LineSink, MAX_LINE, OUTPUT_CAP, OutputLine, Refused,
        RunOutput, RunProcess, RunSpec, RunUpdate, Stream, THREADS_LEFT,
    },
    runs::{Run, RunEnd, RunKind, RunStatus, RunSummary, Runs},
    task_run,
};

/// What a [`Recorder`] has seen.
#[derive(Default)]
struct Record {
    lines: Vec<(Stream, String, Duration)>,
    /// How many lines it was handed, kept or not.
    count: usize,
    /// When each batch's first line came, and when the batch ended.
    batches: Vec<(Instant, Instant)>,
    batch_began: Option<Instant>,
    /// Each batch's size, counted as for the cap.
    batch_sizes: Vec<usize>,
    batch_size: usize,
}

/// A sink that records what it's handed and, if `reports`, says each line
/// is a test passed.
#[derive(Clone)]
struct Recorder {
    record: Arc<Mutex<Record>>,
    keep_lines: bool,
    reports: bool,
    /// How long each batch takes it, if it's slow.
    slow: Option<Duration>,
    /// If set, each line waits for this to open once it's recorded.
    gate: Option<Arc<AtomicBool>>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            record: Arc::default(),
            keep_lines: true,
            reports: false,
            slow: None,
            gate: None,
        }
    }

    fn record(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap()
    }
}

impl LineSink for Recorder {
    fn line(&mut self, stream: Stream, text: &str, at: Duration) -> Vec<RunUpdate> {
        let mut record = self.record();
        record.batch_began.get_or_insert_with(Instant::now);
        record.count += 1;
        record.batch_size += text.len() + 1;
        if self.keep_lines {
            record.lines.push((stream, text.to_owned(), at));
        }
        if let Some(gate) = &self.gate {
            drop(record);
            while !gate.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(5));
            }
            record = self.record();
        }
        match self.reports {
            true => vec![
                RunUpdate::Progress(Some(record.count as f32 / 80.0)),
                RunUpdate::Summary(RunSummary::Tests {
                    passed: record.count,
                    failed: 0,
                    total: None,
                }),
            ],
            false => Vec::new(),
        }
    }

    fn batch_end(&mut self) -> Vec<RunUpdate> {
        let mut record = self.record();
        if let Some(slow) = self.slow {
            thread::sleep(slow);
        }
        let began = record.batch_began.take().expect("a batch with no lines");
        record.batches.push((began, Instant::now()));
        let size = std::mem::take(&mut record.batch_size);
        record.batch_sizes.push(size);
        Vec::new()
    }
}

/// The workbench's runs, with trust decisions kept in memory, and a folder
/// for the scripts to run in, not trusted yet.
fn setup(cx: &mut TestAppContext) -> (Entity<Runs>, tempfile::TempDir) {
    cx.executor().allow_parking();
    cx.update(kiln_shell::trust::init);
    let runs = cx.new(|_| Runs::default());
    (runs, tempfile::tempdir().unwrap())
}

fn trust(folder: &Path, trusted: bool, cx: &mut TestAppContext) {
    let folder = dunce::canonicalize(folder).unwrap();
    cx.update(|cx| kiln_shell::trust::decide(&folder, trusted, cx));
}

/// `sh -c script` in `cwd`, with this process's `PATH`.
fn sh(script: &str, cwd: &Path) -> RunSpec {
    let path = std::env::var_os("PATH").unwrap_or_default();
    RunSpec {
        kind: RunKind::Test,
        title: "sh".to_owned(),
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: cwd.to_owned(),
        env: vec![(OsString::from("PATH"), path)],
    }
}

async fn start(
    spec: RunSpec,
    sink: &Recorder,
    runs: &Entity<Runs>,
    cx: &mut TestAppContext,
) -> Result<Entity<RunProcess>, Refused> {
    let (sink, runs) = (sink.clone(), runs.clone());
    cx.update(|cx| RunProcess::start(spec, sink, runs, cx))
        .await
}

/// Runs the app until `done`, which real threads bring about, for up to 30 s.
fn wait_until(
    cx: &mut TestAppContext,
    what: &str,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) {
    let give_up = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < give_up, "gave up waiting: {what}");
        thread::sleep(Duration::from_millis(2));
    }
}

fn ended(process: &Entity<RunProcess>, cx: &mut TestAppContext) -> Option<RunEnd> {
    process.read_with(cx, |process, _| process.end())
}

fn wait_to_end(process: &Entity<RunProcess>, cx: &mut TestAppContext) -> RunEnd {
    wait_until(cx, "the run to end", |cx| ended(process, cx).is_some());
    ended(process, cx).unwrap()
}

fn run_of(process: &Entity<RunProcess>, runs: &Entity<Runs>, cx: &mut TestAppContext) -> Run {
    let id = process.read_with(cx, |process, _| process.run());
    runs.read_with(cx, |runs, _| runs.get(id).cloned().unwrap())
}

fn kept(process: &Entity<RunProcess>, cx: &mut TestAppContext) -> Vec<(Stream, String)> {
    process.read_with(cx, |process, _| {
        (process.output().lines())
            .map(|line| (line.stream, line.text.clone()))
            .collect()
    })
}

#[gpui::test]
async fn streams_lines_in_order_in_batches(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let sink = Recorder {
        reports: true,
        ..Recorder::new()
    };
    let script = "i=0; while [ $i -lt 40 ]; do \
                    echo out$i; echo err$i >&2; i=$((i + 1)); sleep 0.005; \
                  done";
    let process = start(sh(script, dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    assert_eq!(wait_to_end(&process, cx), RunEnd::Passed);

    let record = sink.record();
    let of = |stream: Stream| -> Vec<String> {
        (record.lines.iter())
            .filter(|(from, ..)| *from == stream)
            .map(|(_, text, _)| text.clone())
            .collect()
    };
    let expected =
        |prefix: &str| -> Vec<String> { (0..40).map(|i| format!("{prefix}{i}")).collect() };
    assert_eq!(of(Stream::Stdout), expected("out"));
    assert_eq!(of(Stream::Stderr), expected("err"));
    for stream in [Stream::Stdout, Stream::Stderr] {
        let times: Vec<Duration> = (record.lines.iter())
            .filter(|(from, ..)| *from == stream)
            .map(|(.., at)| *at)
            .collect();
        assert!(times.is_sorted(), "{stream:?} read out of order: {times:?}");
    }
    // The kept output is what the sink saw, in the same order.
    let seen: Vec<(Stream, String)> = (record.lines.iter())
        .map(|(stream, text, _)| (*stream, text.clone()))
        .collect();
    assert_eq!(kept(&process, cx), seen);

    // The script takes at least 200 ms: more than one batch, never two
    // within the interval, so no more than one each interval.
    let batches = &record.batches;
    assert!(batches.len() > 1, "{} batches", batches.len());
    let span = batches.last().unwrap().0.duration_since(batches[0].0);
    let most = span.as_millis() / BATCH_INTERVAL.as_millis() + 1;
    assert!(
        batches.len() as u128 <= most,
        "{} batches in {span:?}",
        batches.len()
    );
    for pair in batches.windows(2) {
        let gap = pair[1].0.duration_since(pair[0].1);
        assert!(gap >= BATCH_INTERVAL, "two batches {gap:?} apart");
    }

    // The sink's updates reached the run; the exit kept its summary.
    let run = run_of(&process, &runs, cx);
    assert_eq!(run.status, RunStatus::Passed);
    let tests = RunSummary::Tests {
        passed: 80,
        failed: 0,
        total: None,
    };
    assert_eq!(run.summary, tests);
}

#[gpui::test]
async fn a_nonzero_exit_finishes_the_run_failed_with_its_code(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let sink = Recorder::new();
    let script = "echo out; echo err >&2; exit 3";
    let process = start(sh(script, dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    let failed = RunEnd::Failed { code: Some(3) };
    assert_eq!(wait_to_end(&process, cx), failed);

    let run = run_of(&process, &runs, cx);
    assert_eq!(run.status, RunStatus::Failed { code: Some(3) });
    assert_eq!(run.summary, RunSummary::Exit(3));
    assert!(run.finished.is_some());
    let mut lines = kept(&process, cx);
    lines.sort_by_key(|(stream, _)| *stream == Stream::Stderr);
    let expected = vec![
        (Stream::Stdout, "out".to_owned()),
        (Stream::Stderr, "err".to_owned()),
    ];
    assert_eq!(lines, expected);
}

/// Whether process `pid` is gone: no longer there, or a zombie.
fn gone(pid: &str) -> bool {
    let ps = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&ps.stdout);
    stat.trim().is_empty() || stat.trim().starts_with('Z')
}

#[gpui::test]
async fn cancel_kills_the_whole_process_group(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let sink = Recorder::new();
    // Both the shell and its child ignore SIGTERM, which the child inherits:
    // only the SIGKILL that follows ends them.
    let script = "trap '' TERM; sleep 30 & echo $!; wait";
    let process = start(sh(script, dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    wait_until(cx, "the child's pid", |cx| !kept(&process, cx).is_empty());
    let child = kept(&process, cx)[0].1.clone();
    assert!(!gone(&child), "the child sleep isn't running");

    let cancelled_at = Instant::now();
    process.update(cx, |process, cx| process.cancel(cx));
    assert_eq!(wait_to_end(&process, cx), RunEnd::Cancelled);
    // The run is cancelled at once, not once the group is killed.
    assert!(cancelled_at.elapsed() < GRACE);
    let run = run_of(&process, &runs, cx);
    assert_eq!(run.status, RunStatus::Cancelled);

    let give_up = Instant::now() + Duration::from_secs(20);
    while !gone(&child) {
        assert!(
            Instant::now() < give_up,
            "the child sleep outlived the cancel"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[gpui::test]
async fn output_is_capped_at_8_mib_and_marked_truncated(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let sink = Recorder {
        keep_lines: false,
        ..Recorder::new()
    };
    // About 10.9 MB of lines, "1" to "1500000".
    const LAST: usize = 1_500_000;
    let script = format!("seq 1 {LAST}");
    let process = start(sh(&script, dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    assert_eq!(wait_to_end(&process, cx), RunEnd::Passed);
    // The sink saw every line, whatever was kept.
    assert_eq!(sink.record().count, LAST);

    let (truncated, size, numbers) = process.read_with(cx, |process, _| {
        let output = process.output();
        let numbers: Vec<usize> = (output.lines())
            .map(|line| line.text.parse().unwrap())
            .collect();
        (output.truncated(), output.size(), numbers)
    });
    assert!(truncated);
    // The newest that fit: no line is more than 8 bytes with its newline.
    assert!(
        size <= OUTPUT_CAP && size > OUTPUT_CAP - 8,
        "{size} bytes kept"
    );
    let first = numbers[0];
    assert!(first > 1);
    assert_eq!(numbers, (first..=LAST).collect::<Vec<_>>());
    let counted: usize = numbers.iter().map(|n| n.to_string().len() + 1).sum();
    assert_eq!(counted, size);
}

#[gpui::test]
async fn an_untrusted_root_refuses_with_the_task_message(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    let refused = Refused::Untrusted(task_run::untrusted_message());
    let ran = dir.path().join("ran");
    let script = "touch ran";

    // Undecided.
    let sink = Recorder::new();
    let answer = start(sh(script, dir.path()), &sink, &runs, cx).await;
    assert_eq!(answer.err(), Some(refused.clone()));
    assert_eq!(refused.message(), task_run::untrusted_message());

    // Decided against.
    trust(dir.path(), false, cx);
    let answer = start(sh(script, dir.path()), &sink, &runs, cx).await;
    assert_eq!(answer.err(), Some(refused.clone()));

    // A trusted folder's link to one that isn't: the folder it reaches
    // counts.
    #[cfg(unix)]
    {
        let trusted = tempfile::tempdir().unwrap();
        trust(trusted.path(), true, cx);
        let link = trusted.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        let answer = start(sh(script, &link), &sink, &runs, cx).await;
        assert_eq!(answer.err(), Some(refused.clone()));
    }

    // A folder that isn't there can't be trusted.
    let missing = dir.path().join("missing");
    let answer = start(sh(script, &missing), &sink, &runs, cx).await;
    assert_eq!(answer.err(), Some(refused));

    cx.run_until_parked();
    thread::sleep(Duration::from_millis(100));
    assert!(!ran.exists(), "the script ran");
    assert_eq!(runs.read_with(cx, |runs, _| runs.list().count()), 0);
}

#[gpui::test]
async fn dropping_the_process_cancels_its_run_and_stops_it(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let sink = Recorder::new();
    let process = start(sh("echo $$; sleep 30", dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    wait_until(cx, "the shell's pid", |cx| !kept(&process, cx).is_empty());
    let shell = kept(&process, cx)[0].1.clone();
    let id = process.read_with(cx, |process, _| process.run());

    drop(process);
    // Releasing it is an effect, flushed by the next update.
    cx.update(|_| {});
    let status = runs.read_with(cx, |runs, _| runs.get(id).unwrap().status);
    assert_eq!(status, RunStatus::Cancelled);
    let give_up = Instant::now() + Duration::from_secs(20);
    while !gone(&shell) {
        assert!(Instant::now() < give_up, "the shell outlived its run");
        thread::sleep(Duration::from_millis(20));
    }
}

#[gpui::test]
async fn a_slow_sink_takes_bounded_batches_and_every_line(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let sink = Recorder {
        slow: Some(Duration::from_millis(30)),
        ..Recorder::new()
    };
    // About 2 MB, far faster than the sink takes it.
    const LAST: usize = 300_000;
    let script = format!("seq 1 {LAST}");
    let process = start(sh(&script, dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    assert_eq!(wait_to_end(&process, cx), RunEnd::Passed);

    let record = sink.record();
    let numbers: Vec<usize> = (record.lines.iter())
        .map(|(_, text, _)| text.parse().unwrap())
        .collect();
    assert_eq!(numbers, (1..=LAST).collect::<Vec<_>>());
    // What waits for the sink is bounded: the pipes hold the rest.
    let most = record.batch_sizes.iter().max().copied().unwrap();
    assert!(most <= BATCH_CAP + MAX_LINE + 1, "a batch of {most} bytes");
    assert!(record.batch_sizes.len() > 1);
}

#[gpui::test]
async fn a_cancel_kills_on_time_while_the_main_thread_is_behind(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let sink = Recorder {
        keep_lines: false,
        ..Recorder::new()
    };
    // Floods its output, and only SIGKILL ends it.
    let script = "trap '' TERM; sleep 30 & echo $!; yes";
    let process = start(sh(script, dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    wait_until(cx, "the child's pid", |cx| !kept(&process, cx).is_empty());
    let child = kept(&process, cx)[0].1.clone();
    assert!(!gone(&child), "the child sleep isn't running");

    // The main thread takes nothing from here on: its batches back up.
    thread::sleep(Duration::from_millis(300));
    let cancelled_at = Instant::now();
    process.update(cx, |process, cx| process.cancel(cx));
    while !gone(&child) {
        let waited = cancelled_at.elapsed();
        assert!(
            waited < GRACE + Duration::from_secs(5),
            "not killed after {waited:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(wait_to_end(&process, cx), RunEnd::Cancelled);
    assert_eq!(run_of(&process, &runs, cx).status, RunStatus::Cancelled);
}

#[test]
fn a_truncated_batch_replaces_the_older_output() {
    let line = |text: String| OutputLine {
        stream: Stream::Stdout,
        text,
        at: Duration::ZERO,
    };
    let mut output = RunOutput::default();
    output.extend([line("old".to_owned())]);
    // Nine lines of a million bytes: the batch keeps eight, with room left
    // for "old", which is older than the line it dropped.
    let mut batch = RunOutput::default();
    batch.extend((0..9).map(|i| line(i.to_string().repeat(1_000_000))));
    assert!(batch.truncated());
    output.append(batch);

    assert!(output.truncated());
    let firsts: Vec<char> = (output.lines())
        .map(|line| line.text.chars().next().unwrap())
        .collect();
    assert_eq!(firsts, ('1'..='8').collect::<Vec<_>>());
    assert_eq!(output.size(), 8 * 1_000_001);

    // A batch that kept all its lines follows the output, which keeps all
    // it has room for.
    let mut whole = RunOutput::default();
    whole.extend([line("new".to_owned())]);
    output.append(whole);
    assert_eq!(output.lines().len(), 9);
    assert_eq!(output.lines().last().unwrap().text, "new");
    assert!(output.truncated());
}

#[gpui::test]
async fn a_cancel_kills_on_time_while_the_sink_is_blocked(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let gate = Arc::new(AtomicBool::new(false));
    let sink = Recorder {
        gate: Some(gate.clone()),
        ..Recorder::new()
    };
    // Only SIGKILL ends it, and the sink never returns from its first line.
    let script = "trap '' TERM; sleep 30 & echo $!; wait";
    let process = start(sh(script, dir.path()), &sink, &runs, cx)
        .await
        .unwrap();
    wait_until(cx, "the child's pid", |_| !sink.record().lines.is_empty());
    let child = sink.record().lines[0].1.clone();
    assert!(!gone(&child), "the child sleep isn't running");

    let cancelled_at = Instant::now();
    process.update(cx, |process, cx| process.cancel(cx));
    // Cancelled at once, though the runner's thread is still in the sink.
    assert_eq!(ended(&process, cx), Some(RunEnd::Cancelled));
    assert_eq!(run_of(&process, &runs, cx).status, RunStatus::Cancelled);
    while !gone(&child) {
        let waited = cancelled_at.elapsed();
        assert!(
            waited < GRACE + Duration::from_secs(5),
            "not killed after {waited:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(cancelled_at.elapsed() >= GRACE, "killed before its grace");
    gate.store(true, Ordering::SeqCst);
    cx.run_until_parked();
    assert_eq!(run_of(&process, &runs, cx).status, RunStatus::Cancelled);
}

/// No thread to stop it, or none to run it: either way nothing starts.
#[gpui::test]
async fn nothing_starts_without_its_threads(cx: &mut TestAppContext) {
    let (runs, dir) = setup(cx);
    trust(dir.path(), true, cx);
    let ran = dir.path().join("ran");
    // The stopper's thread is the first, the runner's the second.
    for allowed in [0, 1] {
        let sink = Recorder::new();
        THREADS_LEFT.with(|left| left.set(Some(allowed)));
        let process = start(sh("touch ran", dir.path()), &sink, &runs, cx)
            .await
            .unwrap();
        THREADS_LEFT.with(|left| left.set(None));
        let failed = RunEnd::Failed { code: None };
        assert_eq!(wait_to_end(&process, cx), failed);
        assert_eq!(
            run_of(&process, &runs, cx).status,
            RunStatus::Failed { code: None }
        );
        let lines = kept(&process, cx);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].0, Stream::Stderr);
    }
    thread::sleep(Duration::from_millis(100));
    assert!(!ran.exists(), "the script ran");
}
