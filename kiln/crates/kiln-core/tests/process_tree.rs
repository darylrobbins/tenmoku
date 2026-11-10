// SPDX-License-Identifier: Apache-2.0

//! `ProcessTree` ends a program and what it started: a grandchild holding the program's stdout
//! goes with it, so a reader sees the end of the output; on Unix SIGTERM comes first, then
//! SIGKILL once the grace is up; on Windows nothing the program starts escapes its job.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use kiln_core::platform::ProcessTree;

/// Far longer than any wait here: output still open this long was held by something not killed.
const EOF_WITHIN: Duration = Duration::from_secs(10);

/// Writes `body` to `name` in `dir`, and returns its path.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

/// The command that runs `script`: `sh` on Unix, the script itself (through `cmd.exe`) on
/// Windows. Its stdout is piped, the rest null.
fn command(script: &Path) -> Command {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sh");
        command.arg(script);
        command
    };
    #[cfg(windows)]
    let mut command = Command::new(script);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

/// Reads `stdout` until a line containing `marker`, and returns that line.
fn line_with(stdout: &mut BufReader<ChildStdout>, marker: &str) -> String {
    let mut line = String::new();
    loop {
        line.clear();
        let read = stdout.read_line(&mut line).unwrap();
        assert_ne!(read, 0, "the output ended before {marker:?}");
        if line.contains(marker) {
            return line;
        }
    }
}

/// Reads `stdout` to its end on a thread of its own, and says whether it ended within
/// [`EOF_WITHIN`].
fn ends(mut stdout: BufReader<ChildStdout>) -> bool {
    let (ended, ending) = mpsc::channel();
    thread::spawn(move || {
        let mut rest = Vec::new();
        let _ = stdout.read_to_end(&mut rest);
        let _ = ended.send(());
    });
    ending.recv_timeout(EOF_WITHIN).is_ok()
}

#[test]
fn process_tree_kill_ends_grandchild_holding_stdout() {
    let dir = tempfile::tempdir().unwrap();
    // The program starts a grandchild that inherits its stdout and outlives a kill of the
    // program alone, then waits for it.
    #[cfg(unix)]
    let script = script(
        dir.path(),
        "holder.sh",
        "sleep 60 &\necho \"held $!\"\nwait\n",
    );
    #[cfg(windows)]
    let script = script(
        dir.path(),
        "holder.cmd",
        "@echo off\r\nstart \"\" /b ping -n 60 127.0.0.1\r\necho held\r\nping -n 60 127.0.0.1 >nul\r\n",
    );
    let mut tree = ProcessTree::spawn(&mut command(&script)).unwrap();
    let mut stdout = BufReader::new(tree.take_stdout().unwrap());
    let held = line_with(&mut stdout, "held");

    let status = tree.kill().unwrap();
    assert!(!status.success(), "{status:?}");
    assert!(
        ends(stdout),
        "the grandchild still holds stdout after the tree was killed"
    );
    // On Unix the grandchild's pid is known: it has gone too, not only closed its output.
    #[cfg(unix)]
    gone(held.trim().trim_start_matches("held "));
    #[cfg(windows)]
    let _ = held;
}

#[cfg(unix)]
#[test]
fn process_tree_unix_sigterm_grace_then_sigkill() {
    use std::os::unix::process::ExitStatusExt;

    let dir = tempfile::tempdir().unwrap();
    // Each says it is ready once its trap is set. Its `sleep`s are in the group too: SIGTERM
    // ends each in the first, and the ignoring is inherited in the second.
    let traps = script(
        dir.path(),
        "traps.sh",
        "trap 'exit 7' TERM\necho ready\nwhile :; do sleep 0.05; done\n",
    );
    let ignores = script(
        dir.path(),
        "ignores.sh",
        "trap '' TERM\necho ready\nwhile :; do sleep 0.05; done\n",
    );

    // Exits by its trap: its status is its own, not SIGKILL's, so the grace never ran out. A
    // long grace, so a loaded machine can't make it run out; it returns as soon as the tree has
    // ended.
    let mut tree = ProcessTree::spawn(&mut command(&traps)).unwrap();
    line_with(&mut BufReader::new(tree.take_stdout().unwrap()), "ready");
    let status = tree.cancel_within(Duration::from_secs(30)).unwrap();
    assert_eq!(status.code(), Some(7), "{status:?}");

    // Ignores SIGTERM: killed once the grace is up, no sooner.
    let grace = Duration::from_millis(300);
    let mut tree = ProcessTree::spawn(&mut command(&ignores)).unwrap();
    line_with(&mut BufReader::new(tree.take_stdout().unwrap()), "ready");
    let started = Instant::now();
    let status = tree.cancel_within(grace).unwrap();
    let took = started.elapsed();
    assert_eq!(status.signal(), Some(libc::SIGKILL), "{status:?}");
    assert!(took >= grace, "killed after {took:?}, before the grace");
    assert_eq!(ProcessTree::GRACE, Duration::from_secs(2));
}

#[cfg(windows)]
#[test]
fn process_tree_windows_nothing_escapes_the_job() {
    let dir = tempfile::tempdir().unwrap();
    // Its first act starts a grandchild holding stdout, and then the program exits: a job
    // assigned after the start would miss the grandchild, and the program's own handle can't
    // reach it once it has gone.
    let script = script(
        dir.path(),
        "escaper.cmd",
        "@start \"\" /b ping -n 60 127.0.0.1\r\n",
    );
    let mut tree = ProcessTree::spawn(&mut command(&script)).unwrap();
    let mut stdout = BufReader::new(tree.take_stdout().unwrap());
    // The grandchild is running: ping's first line is its own.
    line_with(&mut stdout, "127.0.0.1");
    let started = Instant::now();
    while tree.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < EOF_WITHIN, "the program never exited");
        thread::sleep(Duration::from_millis(20));
    }

    tree.kill().unwrap();
    assert!(
        ends(stdout),
        "the grandchild escaped the job: it still holds stdout"
    );
}

/// Whether process `pid` is a zombie: exited, not yet reaped.
#[cfg(unix)]
fn is_zombie(pid: u32) -> bool {
    let state = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&state.stdout)
        .trim_start()
        .starts_with('Z')
}

/// Waits for process `pid` to be gone, failing after [`EOF_WITHIN`].
#[cfg(unix)]
fn gone(pid: &str) {
    let started = Instant::now();
    while Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
    {
        assert!(started.elapsed() < EOF_WITHIN, "process {pid} still runs");
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn process_tree_unix_cancel_after_the_program_exited_kills_what_it_left() {
    let dir = tempfile::tempdir().unwrap();
    let script = script(dir.path(), "leaver.sh", "sleep 60 &\necho \"left $!\"\n");
    let mut tree = ProcessTree::spawn(&mut command(&script)).unwrap();
    let mut stdout = BufReader::new(tree.take_stdout().unwrap());
    let left = line_with(&mut stdout, "left");
    let left = left.trim().trim_start_matches("left ");
    let started = Instant::now();
    while tree.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < EOF_WITHIN, "the program never exited");
        thread::sleep(Duration::from_millis(20));
    }
    // Seen to exit, yet unreaped: its pid, the group's id, is still its own.
    assert!(is_zombie(tree.id()), "try_wait reaped the program");

    // What it left gets SIGTERM too, which ends a `sleep`, and the cancel returns as soon as
    // nothing in the group runs, well before the grace is up.
    let started = Instant::now();
    let status = tree.cancel_within(Duration::from_secs(30)).unwrap();
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert!(started.elapsed() < EOF_WITHIN, "{:?}", started.elapsed());
    assert!(ends(stdout), "what the program left still holds stdout");
    gone(left);
}

#[test]
fn process_tree_ended_twice_returns_the_first_status() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let script = script(dir.path(), "waiter.sh", "echo ready\nsleep 60\n");
    #[cfg(windows)]
    let script = script(
        dir.path(),
        "waiter.cmd",
        "@echo ready\r\n@ping -n 60 127.0.0.1 >nul\r\n",
    );
    let mut tree = ProcessTree::spawn(&mut command(&script)).unwrap();
    line_with(&mut BufReader::new(tree.take_stdout().unwrap()), "ready");
    let killed = tree.kill().unwrap();
    assert!(!killed.success(), "{killed:?}");
    // Reaped by the first: the second, the cancel and the drop have nothing left to signal.
    assert_eq!(tree.kill().unwrap(), killed);
    assert_eq!(tree.cancel().unwrap(), killed);
    assert_eq!(tree.wait().unwrap(), killed);
    assert_eq!(tree.try_wait().unwrap(), Some(killed));
}

#[test]
fn process_tree_wait_closes_stdin() {
    // Each reads its stdin to the end before it exits.
    #[cfg(unix)]
    let mut command = Command::new("cat");
    #[cfg(windows)]
    let mut command = Command::new("sort");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut tree = ProcessTree::spawn(&mut command).unwrap();
    let (waited, waiting) = mpsc::channel();
    thread::spawn(move || {
        let _ = waited.send(tree.wait().map(|status| status.success()));
    });
    let waited = waiting.recv_timeout(EOF_WITHIN);
    assert!(
        matches!(waited, Ok(Ok(true))),
        "wait never closed stdin: {waited:?}"
    );
}

/// A program that exits at once on SIGTERM, leaving a worker that runs `worker` (a script body
/// that says `worker ready $$` once its trap is set) in the same group. Returns the tree, its
/// stdout, and the worker's pid, read from it once the worker is ready.
#[cfg(unix)]
fn leader_and_worker(dir: &Path, worker: &str) -> (ProcessTree, BufReader<ChildStdout>, String) {
    let worker = script(dir, "worker.sh", worker);
    let leader = script(
        dir,
        "leader.sh",
        &format!(
            "trap 'exit 0' TERM\nsh '{}' &\nwhile :; do sleep 0.05; done\n",
            worker.display()
        ),
    );
    let mut tree = ProcessTree::spawn(&mut command(&leader)).unwrap();
    let mut stdout = BufReader::new(tree.take_stdout().unwrap());
    let ready = line_with(&mut stdout, "worker ready");
    let pid = ready.trim().trim_start_matches("worker ready ").to_owned();
    (tree, stdout, pid)
}

#[cfg(unix)]
#[test]
fn process_tree_unix_grace_outlives_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let flushed = dir.path().join("flushed");
    // The program exits on SIGTERM at once; its worker takes 200 ms to clean up first.
    let (mut tree, stdout, worker) = leader_and_worker(
        dir.path(),
        &format!(
            "trap 'sleep 0.2; echo flushed > \"{}\"; exit 0' TERM\necho \"worker ready $$\"\nwhile :; do sleep 0.05; done\n",
            flushed.display()
        ),
    );
    let started = Instant::now();
    // A long grace, which a loaded machine can't use up: the cancel returns once the worker
    // has ended.
    let status = tree.cancel_within(Duration::from_secs(30)).unwrap();
    let took = started.elapsed();
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert_eq!(
        std::fs::read_to_string(&flushed).ok().as_deref(),
        Some("flushed\n"),
        "the worker was killed before it cleaned up"
    );
    assert!(
        took >= Duration::from_millis(200),
        "returned after {took:?}"
    );
    assert!(took < EOF_WITHIN, "returned after {took:?}");
    assert!(ends(stdout));
    gone(&worker);
}

#[cfg(unix)]
#[test]
fn process_tree_unix_what_the_program_left_is_killed_when_the_grace_is_up() {
    let dir = tempfile::tempdir().unwrap();
    // The program exits on SIGTERM at once; its worker ignores it.
    let (mut tree, stdout, worker) = leader_and_worker(
        dir.path(),
        "trap '' TERM\necho \"worker ready $$\"\nwhile :; do sleep 0.05; done\n",
    );
    let grace = Duration::from_millis(300);
    let started = Instant::now();
    let status = tree.cancel_within(grace).unwrap();
    let took = started.elapsed();
    assert_eq!(status.code(), Some(0), "{status:?}");
    assert!(
        took >= grace,
        "the worker was killed after {took:?}, before the grace"
    );
    assert!(ends(stdout), "the worker still holds stdout");
    gone(&worker);
}
