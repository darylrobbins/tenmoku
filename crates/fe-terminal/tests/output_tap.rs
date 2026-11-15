// A session's output tap hears every byte the child writes, whatever the
// grid's scrollback keeps (Terminal::spawn_tapped).
#![cfg(unix)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fe_terminal::{GridSize, OutputTap, Terminal, TerminalEvent, TerminalOptions};

#[test]
fn the_tap_hears_output_the_scrollback_drops() {
    let dir = tempfile::tempdir().unwrap();
    let script = "i=0; while [ $i -lt 3000 ]; do echo line $i; i=$((i+1)); done";
    let options = TerminalOptions {
        cwd: dir.path().to_owned(),
        shell: Some((
            "/bin/sh".to_owned(),
            vec!["-c".to_owned(), script.to_owned()],
        )),
        scrollback: 0,
        env: Default::default(),
    };
    let size = GridSize {
        cols: 20,
        rows: 5,
        cell_width: 8,
        cell_height: 16,
    };
    let heard = Arc::new(Mutex::new(Vec::new()));
    let tap: OutputTap = Arc::new({
        let heard = heard.clone();
        move |bytes: &[u8]| heard.lock().unwrap().extend_from_slice(bytes)
    });
    let (terminal, events) = Terminal::spawn_tapped(options, size, Some(tap)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match events.try_recv() {
            Ok(TerminalEvent::Exited(code)) => {
                assert_eq!(code, Some(0));
                break;
            }
            Ok(_) => {}
            Err(_) => {
                assert!(Instant::now() < deadline, "the shell never exited");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    let text = String::from_utf8(heard.lock().unwrap().clone()).unwrap();
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    assert_eq!(lines.first(), Some(&"line 0"));
    assert_eq!(lines.len(), 3000);
    assert_eq!(lines.last(), Some(&"line 2999"));
    assert!(
        !terminal.output_text().contains("line 0\n"),
        "the grid dropped it"
    );
}
