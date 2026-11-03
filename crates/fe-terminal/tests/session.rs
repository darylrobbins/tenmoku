use std::time::{Duration, Instant};

use fe_terminal::{
    GridSize, SelectionKind, Side, Snapshot, Terminal, TerminalEvent, TerminalOptions,
};

const DEADLINE: Duration = Duration::from_secs(5);

struct Session {
    terminal: Terminal,
    events: async_channel::Receiver<TerminalEvent>,
    exited: Option<Option<i32>>,
}

impl Session {
    /// Runs `/bin/sh -c script` in `cwd` on a `cols`×`rows` grid.
    fn sh(script: &str, cwd: &std::path::Path, cols: u16, rows: u16) -> Session {
        let options = TerminalOptions {
            cwd: cwd.to_owned(),
            shell: Some((
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), script.to_owned()],
            )),
            scrollback: TerminalOptions::DEFAULT_SCROLLBACK,
            env: Default::default(),
        };
        let size = GridSize {
            cols,
            rows,
            cell_width: 8,
            cell_height: 16,
        };
        let (terminal, events) = Terminal::spawn(options, size).expect("spawn /bin/sh");
        Session {
            terminal,
            events,
            exited: None,
        }
    }

    fn drain(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if let TerminalEvent::Exited(code) = event {
                assert!(self.exited.is_none(), "Exited sent twice");
                self.exited = Some(code);
            }
        }
    }

    /// Waits up to 5 s for `done` to hold, then returns the snapshot it held on.
    fn wait_for(&mut self, what: &str, done: impl Fn(&Snapshot, &Session) -> bool) -> Snapshot {
        let start = Instant::now();
        loop {
            self.drain();
            let snapshot = self.terminal.snapshot();
            if done(&snapshot, self) {
                return snapshot;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {what}; screen:\n{}",
                snapshot.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_text(&mut self, text: &str) -> Snapshot {
        self.wait_for(text, |snapshot, _| snapshot.text().contains(text))
    }
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

#[test]
fn refuses_a_cwd_that_is_not_a_directory() {
    let dir = temp_dir();
    let file = dir.path().join("file");
    std::fs::write(&file, "").unwrap();
    let size = GridSize {
        cols: 80,
        rows: 24,
        cell_width: 8,
        cell_height: 16,
    };
    for (cwd, kind) in [
        (dir.path().join("missing"), std::io::ErrorKind::NotFound),
        (file, std::io::ErrorKind::NotADirectory),
    ] {
        let options = TerminalOptions {
            cwd,
            shell: Some((
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), "pwd".to_owned()],
            )),
            scrollback: TerminalOptions::DEFAULT_SCROLLBACK,
            env: Default::default(),
        };
        let error = Terminal::spawn(options, size)
            .err()
            .expect("spawn should fail");
        assert_eq!(error.kind(), kind);
    }
}

#[test]
fn env_reaches_the_child_over_the_terminals_own() {
    let dir = temp_dir();
    let script = "printf '%s/%s' \"$TASK_VAR\" \"$TERM\"";
    let options = TerminalOptions {
        shell: Some((
            "/bin/sh".to_owned(),
            vec!["-c".to_owned(), script.to_owned()],
        )),
        env: [("TASK_VAR", "from task"), ("TERM", "dumb")]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .into(),
        ..TerminalOptions::new(dir.path())
    };
    let size = GridSize {
        cols: 80,
        rows: 10,
        cell_width: 8,
        cell_height: 16,
    };
    let (terminal, events) = Terminal::spawn(options, size).unwrap();
    let mut session = Session {
        terminal,
        events,
        exited: None,
    };
    let snapshot = session.wait_for("Exited", |_, session| session.exited.is_some());
    assert_eq!(snapshot.row_text(0), "from task/dumb");
}

#[test]
fn runs_a_command_in_cwd() {
    let dir = temp_dir();
    let cwd = dir.path().canonicalize().unwrap();
    let mut session = Session::sh("pwd; printf hello", &cwd, 200, 10);

    // The child's last output is in the grid by the time `Exited` arrives.
    let snapshot = session.wait_for("Exited", |_, session| session.exited.is_some());
    assert_eq!(snapshot.row_text(0), cwd.to_str().unwrap());
    assert_eq!(snapshot.row_text(1), "hello");
    assert_eq!(session.exited, Some(Some(0)));
}

#[test]
fn input_resize_and_scrollback() {
    let dir = temp_dir();

    // Input reaches cat, which echoes it back after the tty's own echo.
    let mut cat = Session::sh("cat", dir.path(), 80, 24);
    // Empty input is dropped, not queued where it would stall the writer.
    cat.terminal.input(Vec::new());
    cat.terminal.input(&b"ping from input\r"[..]);
    let snapshot = cat.wait_for("cat's echo", |snapshot, _| {
        snapshot.text().matches("ping from input").count() == 2
    });
    assert_eq!(snapshot.row_text(0), "ping from input");
    assert_eq!(snapshot.row_text(1), "ping from input");

    // A resize reaches the child before input sent after it.
    let script = "read -r x; stty size; read -r x; \
                  i=0; while [ $i -lt 2000 ]; do echo line$i; i=$((i+1)); done; read -r x";
    let mut sh = Session::sh(script, dir.path(), 80, 24);
    sh.terminal.resize(GridSize {
        cols: 100,
        rows: 30,
        cell_width: 8,
        cell_height: 16,
    });
    sh.terminal.input(&b"\r"[..]);
    let snapshot = sh.wait_for_text("30 100");
    assert_eq!(snapshot.rows.len(), 30);
    assert_eq!(snapshot.cols, 100);

    // 2000 lines push most into scrollback; scrolling back shows earlier ones.
    sh.terminal.input(&b"\r"[..]);
    let snapshot = sh.wait_for_text("line1999");
    assert_eq!(snapshot.display_offset, 0);
    assert!(
        snapshot.history_size >= 1900,
        "history {}",
        snapshot.history_size
    );
    assert!(!snapshot.text().contains("line1940\n"));

    sh.terminal.scroll(-50);
    let scrolled = sh.terminal.snapshot();
    assert_eq!(scrolled.display_offset, 50);
    assert!(
        scrolled.text().contains("line1940\n"),
        "{}",
        scrolled.text()
    );
    assert!(!scrolled.text().contains("line1999"));
    assert_eq!(scrolled.cursor.shape, fe_terminal::CursorShape::Hidden);

    sh.terminal.scroll(20);
    assert_eq!(sh.terminal.snapshot().display_offset, 30);

    // Typing returns to the bottom.
    sh.terminal.input(&b"x"[..]);
    assert_eq!(sh.terminal.snapshot().display_offset, 0);
}

#[test]
fn selection_and_drop() {
    let dir = temp_dir();
    let mut session = Session::sh(
        "printf 'alpha beta gamma\\nsecond line here\\n\u{754c}x\\n'; read -r x",
        dir.path(),
        80,
        24,
    );
    session.wait_for_text("second line here");

    session
        .terminal
        .select(SelectionKind::Word, 0, 8, Side::Left);
    assert_eq!(session.terminal.selected_text().as_deref(), Some("beta"));
    let spans = session.terminal.snapshot().selection;
    assert_eq!(spans.len(), 1);
    assert_eq!(
        (spans[0].row, spans[0].start_col, spans[0].end_col),
        (0, 6, 10)
    );

    session
        .terminal
        .select(SelectionKind::Line, 1, 3, Side::Left);
    assert_eq!(
        session
            .terminal
            .selected_text()
            .as_deref()
            .map(str::trim_end),
        Some("second line here")
    );

    session
        .terminal
        .select(SelectionKind::Simple, 0, 0, Side::Left);
    session.terminal.extend_selection(0, 4, Side::Right);
    assert_eq!(session.terminal.selected_text().as_deref(), Some("alpha"));

    session.terminal.clear_selection();
    assert_eq!(session.terminal.selected_text(), None);
    assert!(session.terminal.snapshot().selection.is_empty());

    // Dropping the terminal hangs up the child, which is reaped off this thread.
    // Selecting the right half of a wide character selects all of it.
    session
        .terminal
        .select(SelectionKind::Simple, 2, 1, Side::Left);
    session.terminal.extend_selection(2, 1, Side::Right);
    assert_eq!(
        session.terminal.selected_text().as_deref(),
        Some("\u{754c}")
    );
    let spans = session.terminal.snapshot().selection;
    assert_eq!(spans.len(), 1);
    assert_eq!(
        (spans[0].row, spans[0].start_col, spans[0].end_col),
        (2, 0, 2)
    );

    let Session {
        terminal, events, ..
    } = session;
    let pid = terminal.pid() as libc::pid_t;
    let dropped = Instant::now();
    drop(terminal);
    assert!(
        dropped.elapsed() < Duration::from_millis(500),
        "drop blocked"
    );
    let start = Instant::now();
    loop {
        // SAFETY: signal 0 only checks that the process exists.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
            break;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "child {pid} still alive after drop"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    // The session reports its end even though the child never exited on its
    // own.
    let start = Instant::now();
    loop {
        match events.try_recv() {
            Ok(TerminalEvent::Exited(code)) => {
                assert_eq!(code, None);
                break;
            }
            Ok(_) => {}
            Err(async_channel::TryRecvError::Empty) => {
                assert!(start.elapsed() < DEADLINE, "no Exited after drop");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("no Exited after drop: {error}"),
        }
    }
}

#[test]
fn refuses_a_cwd_the_user_cannot_enter() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
    // Root enters any directory.
    if std::fs::read_dir(&locked).is_ok() && unsafe { libc::geteuid() } == 0 {
        return;
    }
    let options = TerminalOptions {
        cwd: locked.clone(),
        shell: Some((
            "/bin/sh".to_owned(),
            vec!["-c".to_owned(), "pwd".to_owned()],
        )),
        scrollback: TerminalOptions::DEFAULT_SCROLLBACK,
        env: Default::default(),
    };
    let size = GridSize {
        cols: 80,
        rows: 24,
        cell_width: 8,
        cell_height: 16,
    };
    let error = Terminal::spawn(options, size)
        .err()
        .expect("spawn should fail");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn output_of_an_unfinished_synchronized_update_survives_exit() {
    let dir = temp_dir();
    let mut session = Session::sh("printf 'before\\n\\033[?2026hhello'", dir.path(), 80, 10);

    let snapshot = session.wait_for("Exited", |_, session| session.exited.is_some());
    assert_eq!(snapshot.row_text(0), "before");
    assert_eq!(snapshot.row_text(1), "hello");
}

/// Runs `script` to its end and returns every event the session sent.
fn events_of(script: &str) -> (Vec<TerminalEvent>, Snapshot) {
    let dir = temp_dir();
    let mut session = Session::sh(script, dir.path(), 80, 10);
    let mut events = Vec::new();
    let start = Instant::now();
    while session.exited.is_none() {
        assert!(start.elapsed() < DEADLINE, "no Exited");
        match session.events.try_recv() {
            Ok(TerminalEvent::Exited(code)) => session.exited = Some(code),
            Ok(event) => events.push(event),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    while let Ok(event) = session.events.try_recv() {
        events.push(event);
    }
    (events, session.terminal.snapshot())
}

#[test]
fn an_unterminated_title_or_clipboard_command_is_not_dispatched_on_exit() {
    for script in [
        "printf '\\033]0;title'",
        "printf '\\033]52;c;aGVsbG8='",
        "printf '\\033[?2026h\\033]0;title'",
    ] {
        let (events, _) = events_of(script);
        assert!(
            !events.iter().any(|event| matches!(
                event,
                TerminalEvent::Title(_) | TerminalEvent::ClipboardStore(_)
            )),
            "{script}: {events:?}"
        );
    }
}

#[test]
fn a_long_unfinished_synchronized_update_survives_exit() {
    // Far more than one locked read (65,535 bytes) of held-back output.
    let (_, snapshot) =
        events_of("printf '\\033[?2026h'; head -c 300000 /dev/zero | tr '\\0' x; printf '\\nend'");
    assert!(snapshot.text().contains("end"), "{}", snapshot.text());
}

#[test]
fn output_text_has_the_scrollback_and_whole_lines() {
    let dir = temp_dir();
    let long = "x".repeat(100);
    let script = format!(
        "echo first; printf '{long}\\n'; \
         i=0; while [ $i -lt 200 ]; do echo line$i; i=$((i+1)); done; read -r x"
    );
    let mut session = Session::sh(&script, dir.path(), 40, 10);
    let snapshot = session.wait_for_text("line199");
    assert!(
        !snapshot.text().contains("first"),
        "scrolled off the screen"
    );
    assert!(!snapshot.text().contains(&long), "wrapped, if it's shown");

    // From anywhere the view is scrolled.
    session.terminal.scroll(-5);
    let text = session.terminal.output_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[..3], ["first", long.as_str(), "line0"], "{text}");
    assert!(lines.contains(&"line199"), "{text}");
    assert_eq!(session.terminal.snapshot().display_offset, 5);
}
