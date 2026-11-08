use super::*;

fn open_fd(path: &str) -> i32 {
    let path = std::ffi::CString::new(path).unwrap();
    // SAFETY: a valid C string; the descriptor is closed by the caller.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_WRONLY) };
    assert!(fd >= 0);
    fd
}

#[test]
fn redirects_only_a_null_stderr() {
    assert!(redirects(Stderr::Null));
    assert!(!redirects(Stderr::Elsewhere));
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log/tenmoku.log");

    // A pipe and a regular file are left alone, and no log is made.
    let mut pipe = [0; 2];
    // SAFETY: `pipe` has room for two descriptors.
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    assert_eq!(stderr_kind(pipe[1]), Stderr::Elsewhere);
    assert!(!redirect(pipe[1], &log, LIMIT, "x\n"));
    let file_path = dir.path().join("plain");
    fs::write(&file_path, "").unwrap();
    let plain = open_fd(file_path.to_str().unwrap());
    assert_eq!(stderr_kind(plain), Stderr::Elsewhere);
    assert!(!redirect(plain, &log, LIMIT, "x\n"));
    assert!(!log.exists());

    // A null descriptor is pointed at the log, and writes to it land there.
    let null = open_fd("/dev/null");
    assert_eq!(stderr_kind(null), Stderr::Null);
    assert!(redirect(null, &log, LIMIT, "first\n"));
    assert_eq!(stderr_kind(null), Stderr::Elsewhere);
    let mut out = ManuallyFile::new(null);
    writeln!(out.0, "second").unwrap();
    assert_eq!(fs::read_to_string(&log).unwrap(), "first\nsecond\n");
    // SAFETY: closing descriptors this test opened.
    unsafe {
        libc::close(pipe[0]);
        libc::close(pipe[1]);
        libc::close(plain);
    }
}

/// A descriptor as a `File` that closes it.
struct ManuallyFile(File);

impl ManuallyFile {
    fn new(fd: i32) -> Self {
        use std::os::unix::io::FromRawFd as _;
        // SAFETY: `fd` is open and owned by the caller, who gives it up.
        Self(unsafe { File::from_raw_fd(fd) })
    }
}

#[test]
fn rotates_past_the_size_limit() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("tenmoku.log");
    let old = dir.path().join("tenmoku.log.1");
    fs::write(&log, "0123456789").unwrap();
    rotate(&log, 5).unwrap();
    assert!(!log.exists());
    assert_eq!(fs::read_to_string(&old).unwrap(), "0123456789");
    fs::write(&log, "abcdefghij").unwrap();
    rotate(&log, 5).unwrap();
    assert_eq!(fs::read_to_string(&old).unwrap(), "abcdefghij");
    // Under the limit it is appended to.
    fs::write(&log, "ab").unwrap();
    open(&log, 5, "x\n").unwrap();
    assert_eq!(fs::read_to_string(&log).unwrap(), "abx\n");
    assert_eq!(fs::read_to_string(&old).unwrap(), "abcdefghij");
}

#[test]
fn launch_line_is_first() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("sub/tenmoku.log");
    let line = launch_line(LaunchKind::Detached, 42, Some(2));
    let mut file = open(&log, LIMIT, &line).unwrap();
    writeln!(file, "[WARN x] record").unwrap();
    let text = fs::read_to_string(&log).unwrap();
    let first = text.lines().next().unwrap();
    assert!(first.contains("pid 42") && first.contains("detached") && first.contains("displays 2"));
    assert!(first.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.lines().nth(1).unwrap().contains("record"));
}

/// The child half of [`install_follows_the_real_stderr`]: does nothing unless
/// the parent started it with `TENMOKU_LOG_CHILD`.
#[test]
fn install_child() {
    if std::env::var_os("TENMOKU_LOG_CHILD").is_none() {
        return;
    }
    install(LaunchKind::Detached, Some(1));
    eprintln!("[WARN child] record");
}

#[test]
fn install_follows_the_real_stderr() {
    use std::process::{Command, Stdio};
    let run = |stderr: Stdio| {
        let home = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "log_file::tests::install_child", "--nocapture"])
            .env("TENMOKU_LOG_CHILD", "1")
            .env("HOME", home.path())
            .env("XDG_STATE_HOME", home.path().join("state"))
            .stdout(Stdio::null())
            .stderr(stderr)
            .output()
            .unwrap();
        let log = log_path(|name| match name {
            "HOME" => Some(home.path().to_owned()),
            "XDG_STATE_HOME" => Some(home.path().join("state")),
            _ => None,
        })
        .unwrap();
        (output, log, home)
    };

    // Null stderr: the launch line and the record land in the log.
    let (_, log, _home) = run(Stdio::null());
    let text = fs::read_to_string(&log).unwrap();
    let mut lines = text.lines();
    assert!(lines.next().unwrap().contains("launch: version"));
    assert!(lines.next().unwrap().contains("record"));

    // A pipe stays the pipe: the record comes through it, and no log is made.
    let (output, log, _home) = run(Stdio::piped());
    assert!(String::from_utf8_lossy(&output.stderr).contains("record"));
    assert!(!log.exists());
}
