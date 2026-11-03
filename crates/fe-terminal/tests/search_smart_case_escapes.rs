use std::time::{Duration, Instant};

use fe_terminal::{GridSize, Snapshot, Terminal, TerminalEvent, TerminalOptions, TerminalQuery};

const DEADLINE: Duration = Duration::from_secs(5);

struct Session {
    terminal: Terminal,
    _events: async_channel::Receiver<TerminalEvent>,
    _dir: tempfile::TempDir,
}

impl Session {
    /// Runs `/bin/sh -c script` on a `cols`×`rows` grid.
    fn sh(script: &str, cols: u16, rows: u16) -> Session {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = TerminalOptions {
            cwd: dir.path().to_owned(),
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
            _events: events,
            _dir: dir,
        }
    }

    /// Waits up to 5 s for `done` to hold on the bottom of the grid.
    fn wait_for(&self, what: &str, done: impl Fn(&Snapshot) -> bool) {
        let start = Instant::now();
        loop {
            let snapshot = self.terminal.snapshot();
            if done(&snapshot) {
                return;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "timed out waiting for {what}; screen:\n{}",
                snapshot.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_text(&self, text: &str) {
        self.wait_for(text, |snapshot| snapshot.text().contains(text));
    }

    fn count(&self, query: &TerminalQuery) -> usize {
        self.terminal
            .find_all_visible(query)
            .expect("query compiles")
            .len()
    }
}

#[test]
fn an_escape_letter_does_not_make_a_regex_case_sensitive() {
    let session = Session::sh("printf 'xBAR! Bar bar!\\n'; read -r x", 40, 5);
    session.wait_for_text("bar!");

    assert_eq!(session.count(&TerminalQuery::regex(r"\Sar")), 3);
    assert_eq!(session.count(&TerminalQuery::regex(r"ar\W")), 3);

    // The regex's own uppercase letter still counts.
    assert_eq!(session.count(&TerminalQuery::regex(r"\SAR")), 1);
    // A literal's backslash is ordinary, so the letter after it counts.
    assert_eq!(session.count(&TerminalQuery::literal(r"\Sar")), 0);

    // `case_sensitive` overrides smart case either way.
    let forced = |text: &str, case_sensitive| TerminalQuery {
        case_sensitive: Some(case_sensitive),
        ..TerminalQuery::regex(text)
    };
    assert_eq!(session.count(&forced(r"\Sar", true)), 2);
    assert_eq!(session.count(&forced(r"\SAR", false)), 3);
}
