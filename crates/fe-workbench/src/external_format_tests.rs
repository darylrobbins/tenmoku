//! External formatters over a fake [`Runner`]: only
//! [`real_runner_runs_a_process`] starts a process.

// What only its Unix tests use goes unused elsewhere.
#![cfg_attr(not(unix), allow(unused_imports))]

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fs;
use std::future::{self, Future};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fe_config::{FormatterPrefer, FormatterSettings};
use fe_lsp::LoginPath;
use gpui::{BackgroundExecutor, TestAppContext};

use crate::external_format::{
    BufferPath, FormatError, OUTPUT_LIMIT, ProcessRunner, RunOutput, RunSpec, Runner, diff_to_edit,
    format_text, on_own_thread, resolve_program,
};

fn saved() -> BufferPath<'static> {
    BufferPath::Saved(Path::new("/project/src/main.rs"))
}

fn settings(command: &[&str], stdin: bool) -> FormatterSettings {
    FormatterSettings {
        command: command.iter().map(|word| (*word).to_owned()).collect(),
        stdin,
        prefer: FormatterPrefer::default(),
        timeout_ms: 3000,
    }
}

fn exit_ok(stdout: &str) -> RunOutput {
    RunOutput {
        code: Some(0),
        stdout: stdout.into(),
        stderr: Vec::new(),
    }
}

fn exit(code: i32, stderr: &str) -> RunOutput {
    RunOutput {
        code: Some(code),
        stdout: Vec::new(),
        stderr: stderr.into(),
    }
}

/// A runner that answers at once with what its closure does, and keeps what it
/// was asked.
struct Fake<F> {
    answer: F,
    specs: RefCell<Vec<RunSpec>>,
}

fn fake<F: Fn(&RunSpec) -> io::Result<RunOutput>>(answer: F) -> Fake<F> {
    Fake {
        answer,
        specs: RefCell::default(),
    }
}

impl<F: Fn(&RunSpec) -> io::Result<RunOutput>> Fake<F> {
    fn spec(&self) -> RunSpec {
        self.specs.borrow().last().expect("it ran").clone()
    }
}

impl<F: Fn(&RunSpec) -> io::Result<RunOutput>> Runner for Fake<F> {
    fn run(&self, spec: RunSpec) -> Pin<Box<dyn Future<Output = io::Result<RunOutput>>>> {
        let answer = (self.answer)(&spec);
        self.specs.borrow_mut().push(spec);
        Box::pin(future::ready(answer))
    }
}

/// A runner that never answers, and notes being dropped: the kill a real one
/// does then.
#[derive(Default)]
struct Hangs {
    cancelled: Rc<Cell<bool>>,
    specs: Rc<RefCell<Vec<RunSpec>>>,
}

struct SetOnDrop(Rc<Cell<bool>>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

impl Runner for Hangs {
    fn run(&self, spec: RunSpec) -> Pin<Box<dyn Future<Output = io::Result<RunOutput>>>> {
        self.specs.borrow_mut().push(spec);
        let cancelled = SetOnDrop(self.cancelled.clone());
        Box::pin(async move {
            let _cancelled = cancelled;
            future::pending().await
        })
    }
}

/// `edit` applied to `text`.
fn applied(text: &str, edit: &(Range<usize>, String)) -> String {
    let mut text = text.to_owned();
    text.replace_range(edit.0.clone(), &edit.1);
    text
}

/// The path a run was given for `{file}`, as its only argument.
fn only_arg(spec: &RunSpec) -> PathBuf {
    assert_eq!(spec.args.len(), 1, "{spec:?}");
    PathBuf::from(&spec.args[0])
}

fn executor(cx: &TestAppContext) -> BackgroundExecutor {
    cx.background_executor.clone()
}

#[gpui::test]
async fn stdin_formatter_returns_its_stdout(cx: &mut TestAppContext) {
    let runner = fake(|_| Ok(exit_ok("fn main() {}\n")));
    let text = "fn  main( ) {}\n";
    let edit = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt"], true),
        saved(),
        text,
    )
    .await
    .unwrap()
    .expect("the output differs");

    assert_eq!(applied(text, &edit), "fn main() {}\n");
    let spec = runner.spec();
    assert_eq!(spec.program, "fmt");
    assert!(spec.args.is_empty());
    assert_eq!(spec.stdin.as_deref(), Some(text.as_bytes()));
    assert_eq!(spec.cwd.as_deref(), Some(Path::new("/project/src")));
}

#[gpui::test]
async fn file_placeholder_is_replaced(cx: &mut TestAppContext) {
    // With stdin, {file} is the buffer's path, wherever in a word it falls.
    let runner = fake(|_| Ok(exit_ok("text")));
    format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "--path={file}", "{file}", "--{file}:{file}"], true),
        saved(),
        "text",
    )
    .await
    .unwrap();
    let path = "/project/src/main.rs";
    assert_eq!(
        runner.spec().args,
        [
            format!("--path={path}"),
            path.to_owned(),
            format!("--{path}:{path}")
        ]
        .map(OsString::from)
    );

    // Without it, {file} is a temporary copy, which the tool rewrites.
    let seen: RefCell<Option<(String, bool)>> = RefCell::default();
    let runner = fake(|spec| {
        let copy = only_arg(spec);
        let text = fs::read_to_string(&copy).unwrap();
        fs::write(&copy, "new text\n").unwrap();
        *seen.borrow_mut() = Some((text, copy.is_file()));
        Ok(exit_ok("this is not the result"))
    });
    let edit = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], false),
        saved(),
        "old text\n",
    )
    .await
    .unwrap()
    .expect("the copy was rewritten");

    assert_eq!(applied("old text\n", &edit), "new text\n");
    assert_eq!(seen.take(), Some(("old text\n".to_owned(), true)));
    let spec = runner.spec();
    assert_eq!(spec.stdin, None, "the text goes in the copy");
    assert_eq!(spec.cwd.as_deref(), Some(Path::new("/project/src")));
    let copy = only_arg(&spec);
    assert_ne!(copy, Path::new("/project/src/main.rs"));
    assert!(copy.starts_with(std::env::temp_dir()), "{copy:?}");
    assert_eq!(copy.extension().unwrap(), "rs");
    assert!(!copy.exists(), "the copy goes");
    assert!(!copy.parent().unwrap().exists(), "and its directory");
}

#[cfg(unix)]
#[gpui::test]
#[cfg(unix)]
async fn a_temporary_copy_is_private(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let mode = Cell::new(0);
    let runner = fake(|spec| {
        let copy = only_arg(spec);
        let metadata = fs::metadata(copy.parent().unwrap()).unwrap();
        mode.set(metadata.permissions().mode() & 0o777);
        Ok(exit_ok(""))
    });
    format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], false),
        saved(),
        "text",
    )
    .await
    .unwrap();
    assert_eq!(mode.get(), 0o700);
}

#[cfg(unix)]
#[gpui::test]
#[cfg(unix)]
async fn a_path_that_is_not_utf8_reaches_the_program_as_it_is(cx: &mut TestAppContext) {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    let path = Path::new(OsStr::from_bytes(b"/project/caf\xe9.rs"));
    let runner = fake(|_| Ok(exit_ok("text")));
    format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "--path={file}"], true),
        BufferPath::Saved(path),
        "text",
    )
    .await
    .unwrap();
    assert_eq!(
        runner.spec().args[0].as_bytes(),
        b"--path=/project/caf\xe9.rs"
    );
}

#[gpui::test]
async fn an_unsaved_buffer_uses_stdin_and_a_placeholder(cx: &mut TestAppContext) {
    let runner = fake(|_| Ok(exit_ok("formatted")));
    // Even for a tool that says it only formats files: nothing on disk is this.
    let edit = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], false),
        BufferPath::Unsaved {
            root: Some(Path::new("/project")),
            extension: Some("py"),
        },
        "text",
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(applied("text", &edit), "formatted");
    let spec = runner.spec();
    assert_eq!(only_arg(&spec), Path::new("/project/untitled.py"));
    assert_eq!(spec.cwd.as_deref(), Some(Path::new("/project")));
    assert_eq!(spec.stdin.as_deref(), Some(b"text".as_slice()));

    // With no root it's a bare name and runs wherever this process is; with no
    // language it has no extension.
    format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], true),
        BufferPath::Unsaved {
            root: None,
            extension: None,
        },
        "text",
    )
    .await
    .unwrap();
    let spec = runner.spec();
    assert_eq!(only_arg(&spec), Path::new("untitled"));
    assert_eq!(spec.cwd, None);
}

#[gpui::test]
async fn nonzero_exit_is_an_error_with_stderr(cx: &mut TestAppContext) {
    let executor = executor(cx);
    let text = "keep me\n";
    let run = async |output: RunOutput| {
        let runner = fake(move |_| Ok(output.clone()));
        format_text(&runner, &executor, &settings(&["fmt"], true), saved(), text).await
    };

    let error = run(RunOutput {
        // Output from a run that failed isn't the result.
        stdout: b"half a file".to_vec(),
        ..exit(1, "\n   \n  syntax error: line 3 \nmore detail\n")
    })
    .await
    .unwrap_err();
    assert_eq!(
        error,
        FormatError::Failed {
            program: "fmt".into(),
            stderr_line: "syntax error: line 3".into(),
        }
    );
    assert_eq!(error.to_string(), "fmt: syntax error: line 3");

    // With nothing on stderr, how it ended; so a signal, which has no code.
    assert_eq!(
        run(exit(3, " \n")).await.unwrap_err().to_string(),
        "fmt: exited with status 3"
    );
    assert_eq!(
        run(RunOutput {
            code: None,
            ..RunOutput::default()
        })
        .await
        .unwrap_err()
        .to_string(),
        "fmt: killed by a signal"
    );
    // A stderr that isn't UTF-8 is still shown.
    assert_eq!(
        run(RunOutput {
            stderr: b"bad \xff byte".to_vec(),
            ..exit(1, "")
        })
        .await
        .unwrap_err()
        .to_string(),
        "fmt: bad \u{fffd} byte"
    );
}

#[gpui::test]
async fn a_failed_run_ignores_the_copy_it_rewrote(cx: &mut TestAppContext) {
    let copy_seen = RefCell::new(None);
    let runner = fake(|spec| {
        let copy = only_arg(spec);
        fs::write(&copy, "half a file").unwrap();
        *copy_seen.borrow_mut() = Some(copy);
        Ok(exit(1, "broke"))
    });
    let error = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], false),
        saved(),
        "text",
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "fmt: broke");
    assert!(!copy_seen.take().unwrap().exists());
}

#[gpui::test]
async fn timeout_kills_the_child(cx: &mut TestAppContext) {
    let runner = Hangs::default();
    let cancelled = runner.cancelled.clone();
    let settings = FormatterSettings {
        timeout_ms: 1000,
        ..settings(&["fmt"], true)
    };
    let background = executor(cx);
    let run = cx
        .foreground_executor
        .spawn(async move { format_text(&runner, &background, &settings, saved(), "text").await });

    cx.run_until_parked();
    assert!(!cancelled.get());
    cx.background_executor
        .advance_clock(Duration::from_millis(999));
    cx.run_until_parked();
    assert!(!cancelled.get(), "not before its time");

    cx.background_executor
        .advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert!(cancelled.get(), "the run is dropped at timeout_ms");
    let error = run.await.unwrap_err();
    assert_eq!(
        error,
        FormatError::TimedOut {
            program: "fmt".into(),
            timeout_ms: 1000,
        }
    );
    assert_eq!(error.to_string(), "fmt: timed out after 1000 ms");
}

#[gpui::test]
async fn a_timeout_removes_the_temporary_copy(cx: &mut TestAppContext) {
    let runner = Hangs::default();
    let specs = runner.specs.clone();
    let settings = FormatterSettings {
        timeout_ms: 10,
        ..settings(&["fmt", "{file}"], false)
    };
    let background = executor(cx);
    let run = cx
        .foreground_executor
        .spawn(async move { format_text(&runner, &background, &settings, saved(), "text").await });
    cx.run_until_parked();
    let copy = only_arg(specs.borrow().last().expect("it started"));
    assert_eq!(fs::read_to_string(&copy).unwrap(), "text");

    cx.background_executor
        .advance_clock(Duration::from_millis(10));
    assert!(matches!(run.await, Err(FormatError::TimedOut { .. })));
    assert!(!copy.parent().unwrap().exists());
}

#[gpui::test]
async fn dropping_the_format_stops_the_run_and_removes_the_copy(cx: &mut TestAppContext) {
    let runner = Hangs::default();
    let cancelled = runner.cancelled.clone();
    let specs = runner.specs.clone();
    let settings = settings(&["fmt", "{file}"], false);
    let background = executor(cx);
    let run = cx
        .foreground_executor
        .spawn(async move { format_text(&runner, &background, &settings, saved(), "text").await });
    cx.run_until_parked();
    let copy = only_arg(specs.borrow().last().expect("it started"));
    assert!(copy.exists());

    drop(run);
    cx.run_until_parked();
    assert!(cancelled.get());
    assert!(!copy.parent().unwrap().exists());
}

#[gpui::test]
async fn missing_program_is_not_found(cx: &mut TestAppContext) {
    let executor = executor(cx);
    let run = async |kind: io::ErrorKind| {
        let runner = fake(move |_| Err(kind.into()));
        format_text(
            &runner,
            &executor,
            &settings(&["fmt"], true),
            saved(),
            "text",
        )
        .await
    };

    let error = run(io::ErrorKind::NotFound).await.unwrap_err();
    assert_eq!(error, FormatError::NotFound("fmt".into()));
    assert_eq!(error.to_string(), "fmt: program not found");

    // Any other failure to start is a failed run, not a cue to fall back quietly.
    assert!(matches!(
        run(io::ErrorKind::PermissionDenied).await,
        Err(FormatError::Failed { program, .. }) if program == "fmt"
    ));
}

#[gpui::test]
async fn a_copy_the_tool_removed_is_a_failure_not_a_missing_program(cx: &mut TestAppContext) {
    let runner = fake(|spec| {
        fs::remove_file(only_arg(spec)).unwrap();
        Ok(exit_ok(""))
    });
    let error = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], false),
        saved(),
        "text",
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&error, FormatError::Failed { stderr_line, .. } if stderr_line.contains("temporary copy")),
        "{error:?}"
    );
}

#[gpui::test]
async fn output_that_is_not_utf8_is_failed(cx: &mut TestAppContext) {
    let runner = fake(|_| {
        Ok(RunOutput {
            stdout: b"caf\xe9\n".to_vec(),
            ..exit(0, "")
        })
    });
    let error = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt"], true),
        saved(),
        "text",
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "fmt: its output isn't UTF-8");
}

#[gpui::test]
async fn no_output_from_text_is_failed_but_blank_text_may_go(cx: &mut TestAppContext) {
    let runner = fake(|_| Ok(exit_ok("")));
    let executor = executor(cx);
    let settings = settings(&["fmt"], true);
    let error = format_text(&runner, &executor, &settings, saved(), "text\n")
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "fmt: it gave no output");

    let edit = format_text(&runner, &executor, &settings, saved(), " \n\n")
        .await
        .unwrap();
    assert_eq!(edit, Some((0..3, String::new())));
    assert_eq!(
        format_text(&runner, &executor, &settings, saved(), "")
            .await
            .unwrap(),
        None
    );
}

#[gpui::test]
async fn a_rewritten_copy_past_the_limit_is_failed(cx: &mut TestAppContext) {
    let runner = fake(|spec| {
        let file = fs::File::create(only_arg(spec))?;
        file.set_len(OUTPUT_LIMIT as u64 + 1)?;
        Ok(exit_ok(""))
    });
    let error = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], false),
        saved(),
        "text",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("more than 64 MiB"), "{error}");
}

#[gpui::test]
async fn an_empty_command_is_failed(cx: &mut TestAppContext) {
    let runner = fake(|_| Ok(exit_ok("")));
    let error = format_text(
        &runner,
        &executor(cx),
        &settings(&[], true),
        saved(),
        "text",
    )
    .await
    .unwrap_err();
    assert!(matches!(error, FormatError::Failed { .. }), "{error:?}");
    assert!(runner.specs.borrow().is_empty());
}

#[test]
fn minimal_edit_keeps_the_surroundings() {
    assert_eq!(diff_to_edit("same\n", "same\n"), None);
    assert_eq!(diff_to_edit("", ""), None);

    // Only the middle.
    assert_eq!(
        diff_to_edit("let a  = 1;\nkeep\n", "let a = 1;\nkeep\n"),
        Some((6..7, String::new()))
    );
    assert_eq!(
        diff_to_edit("head old tail", "head newer tail"),
        Some((5..8, "newer".to_owned()))
    );

    // On char boundaries: é and è share a first byte, é and ĩ a last one.
    assert_eq!(diff_to_edit("aéb", "aèb"), Some((1..3, "è".to_owned())));
    assert_eq!(diff_to_edit("aéb", "aĩb"), Some((1..3, "ĩ".to_owned())));
    assert_eq!(diff_to_edit("é", "ĩ"), Some((0..2, "ĩ".to_owned())));

    // The prefix and suffix don't overlap when the text repeats.
    assert_eq!(diff_to_edit("aa", "aaa"), Some((2..2, "a".to_owned())));
    assert_eq!(diff_to_edit("aaa", "aa"), Some((2..3, String::new())));
    assert_eq!(diff_to_edit("abab", "ab"), Some((2..4, String::new())));

    // At either end, and the whole text.
    assert_eq!(diff_to_edit("", "new"), Some((0..0, "new".to_owned())));
    assert_eq!(diff_to_edit("old", ""), Some((0..3, String::new())));
    assert_eq!(diff_to_edit("old", "new"), Some((0..3, "new".to_owned())));

    // Whatever the pair, the edit makes one the other, and it can't be cut
    // shorter on a side where both still agree.
    let texts = [
        "",
        "a",
        "aa",
        "é",
        "ĩ",
        "aé",
        "éa",
        "éé",
        "é\n",
        "\r\n",
        "\n",
        "日本語",
        "日本",
    ];
    for old in texts {
        for new in texts {
            match diff_to_edit(old, new) {
                None => assert_eq!(old, new),
                Some(edit) => {
                    assert_eq!(applied(old, &edit), new, "{old:?} to {new:?}");
                    let removed = &old[edit.0.clone()];
                    let (gone, put) = (removed.chars().next(), edit.1.chars().next());
                    assert!(gone.is_none() || put.is_none() || gone != put);
                }
            }
        }
    }
}

/// More hung runs than any machine has workers, each blocking a thread until
/// it's stopped: on the executor they'd hold up the timers that stop them, and
/// on this one, whose workers are the test's own thread, this test too.
#[gpui::test]
async fn hung_runs_on_their_own_threads_still_time_out(cx: &mut TestAppContext) {
    struct Blocks(Arc<AtomicUsize>);

    impl Runner for Blocks {
        fn run(&self, _: RunSpec) -> Pin<Box<dyn Future<Output = io::Result<RunOutput>>>> {
            let stopped_runs = self.0.clone();
            on_own_thread(move |stopped| {
                let give_up = Instant::now() + Duration::from_secs(10);
                while !stopped.load(Ordering::SeqCst) {
                    assert!(Instant::now() < give_up, "never stopped");
                    std::thread::sleep(Duration::from_millis(1));
                }
                stopped_runs.fetch_add(1, Ordering::SeqCst);
                Err(io::Error::other("stopped"))
            })
        }
    }

    const RUNS: usize = 128;
    let stopped_runs = Arc::new(AtomicUsize::new(0));
    let settings = FormatterSettings {
        timeout_ms: 100,
        ..settings(&["fmt"], true)
    };
    let runs: Vec<_> = (0..RUNS)
        .map(|_| {
            let (runner, settings) = (Blocks(stopped_runs.clone()), settings.clone());
            let background = executor(cx);
            cx.foreground_executor.spawn(async move {
                format_text(&runner, &background, &settings, saved(), "text").await
            })
        })
        .collect();
    cx.run_until_parked();
    assert_eq!(stopped_runs.load(Ordering::SeqCst), 0);

    cx.background_executor
        .advance_clock(Duration::from_millis(100));
    for run in runs {
        assert!(matches!(run.await, Err(FormatError::TimedOut { .. })));
    }
    let give_up = Instant::now() + Duration::from_secs(10);
    while stopped_runs.load(Ordering::SeqCst) < RUNS {
        assert!(Instant::now() < give_up, "a run wasn't told to stop");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[gpui::test]
async fn a_thread_that_panics_is_an_error(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let answer = on_own_thread::<()>(|_| panic!("on purpose")).await;
    assert_eq!(answer.unwrap_err().kind(), io::ErrorKind::Other);
}

/// A tool that leaves a FIFO in place of its copy: nothing will ever write to
/// it, so opening it as a file would block for good.
#[cfg(unix)]
#[gpui::test]
#[cfg(unix)]
async fn a_copy_replaced_by_a_fifo_is_failed_and_removed(cx: &mut TestAppContext) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;

    let seen = RefCell::new(None);
    let runner = fake(|spec| {
        let copy = only_arg(spec);
        fs::remove_file(&copy)?;
        let name = CString::new(copy.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid C string, which mkfifo(3) only reads.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        *seen.borrow_mut() = Some(copy);
        Ok(exit_ok(""))
    });
    let error = format_text(
        &runner,
        &executor(cx),
        &settings(&["fmt", "{file}"], false),
        saved(),
        "text",
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&error, FormatError::Failed { stderr_line, .. } if stderr_line.contains("regular file")),
        "{error:?}"
    );
    assert!(!seen.take().unwrap().parent().unwrap().exists());
}

/// None of these starts a shell: the `PATH` is given.
#[cfg(unix)]
#[test]
#[cfg(unix)]
fn a_program_that_exists_but_cannot_run_is_not_missing() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let (plain, runnable) = (root.path().join("plain"), root.path().join("runnable"));
    fs::create_dir(&plain).unwrap();
    fs::create_dir(&runnable).unwrap();
    fs::write(plain.join("fmt"), "#!/bin/sh\n").unwrap();
    fs::write(runnable.join("fmt"), "#!/bin/sh\n").unwrap();
    let mode = fs::Permissions::from_mode(0o755);
    fs::set_permissions(runnable.join("fmt"), mode).unwrap();
    fs::create_dir(plain.join("a-directory")).unwrap();

    let login = LoginPath::new("/nonexistent/shell");
    let resolve = |program: &str, path: &[&Path], cwd: &Path| {
        let path = std::env::join_paths(path).unwrap();
        resolve_program(&login, Path::new(program), Some(&path), cwd)
    };
    let kind = |answer: io::Result<PathBuf>| answer.unwrap_err().kind();

    assert_eq!(
        resolve("fmt", &[&runnable], root.path()).unwrap(),
        runnable.join("fmt")
    );
    assert_eq!(
        kind(resolve("fmt", &[&plain], root.path())),
        io::ErrorKind::PermissionDenied
    );
    // A later directory's runnable one is passed to, as in a shell.
    assert_eq!(
        resolve("fmt", &[&plain, &runnable], root.path()).unwrap(),
        runnable.join("fmt")
    );
    assert_eq!(
        kind(resolve("absent", &[&plain, &runnable], root.path())),
        io::ErrorKind::NotFound
    );
    // A directory by that name isn't a program that can't run.
    assert_eq!(
        kind(resolve("a-directory", &[&plain], root.path())),
        io::ErrorKind::NotFound
    );
    // A relative entry isn't searched, so what's under it says nothing.
    assert_eq!(
        kind(resolve("fmt", &[Path::new("plain")], root.path())),
        io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[gpui::test]
async fn real_runner_runs_a_process(cx: &mut TestAppContext) {
    // Its answer comes from a thread of its own, which the test scheduler
    // doesn't drive.
    cx.executor().allow_parking();
    let runner = ProcessRunner;
    let output = runner
        .run(RunSpec {
            program: "cat".into(),
            args: Vec::new(),
            cwd: None,
            stdin: Some(b"from stdin\n".to_vec()),
        })
        .await
        .unwrap();
    assert_eq!(output, exit_ok("from stdin\n"));

    // Through the whole of it, what cat gives back is no change.
    let edit = format_text(
        &runner,
        &executor(cx),
        &settings(&["cat"], true),
        BufferPath::Unsaved {
            root: None,
            extension: None,
        },
        "text\n",
    )
    .await
    .unwrap();
    assert_eq!(edit, None);
}
