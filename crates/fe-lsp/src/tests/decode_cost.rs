//! What decoding a language server's locations in unopened files costs
//! (docs/decisions/lsp-client.md §10), against T108's navigation budgets
//! (PERF.md, Navigation). The fake holds its reply on its own clock; the timer
//! starts as the clock releases it and stops when the answer reaches the
//! caller. That interval is the reply's serialisation and transport, its
//! parsing, and the decoding (a read of each file and the offset conversions);
//! the decoding dominates for these sizes.
//!
//! `cargo test --release -p fe-lsp decode_cost -- --ignored --nocapture`

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use lsp_types::PositionEncodingKind;
use serde_json::Value;

use super::WAIT;
use super::navigation::{Nav, location, range};
use crate::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use crate::{ReadinessMode, ServerStatus};
use fe_nav::NavigationProvider;

const RUNS: usize = 10;
/// How long the fake holds a reply, on its own clock.
const HOLD: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
enum Kind {
    Definitions,
    References,
}

struct Case {
    name: &'static str,
    kind: Kind,
    encoding: PositionEncodingKind,
    /// How many files, how many bytes in each, and how many locations in all.
    files: usize,
    bytes: usize,
    locations: usize,
    /// The line each file repeats, and the column range of `target` in it,
    /// in the case's encoding.
    line: &'static str,
}

const ASCII: &str = "fn target() { let x = 1; } // padding\n";
const WIDE: &str = "let é😀 = ü; fn target() {} // ñandú\n";

/// `target`'s columns in `line` under `encoding`.
fn columns(line: &str, encoding: &PositionEncodingKind) -> (usize, usize) {
    let at = line.find("target").unwrap();
    let col = |bytes: usize| match *encoding == PositionEncodingKind::UTF16 {
        true => line[..bytes].encode_utf16().count(),
        false => bytes,
    };
    (col(at), col(at + 6))
}

fn run(case: &Case) -> Vec<Duration> {
    let script = Script {
        initialize: InitializeReply::Result(Box::new(initialize_result(Some(
            case.encoding.clone(),
        )))),
        ..Script::default()
    };
    let s = Nav::with(ReadinessMode::ServerStatus, script, None);
    s.status(true);
    assert_eq!(s.events.status(s.server), ServerStatus::Ready);

    let rows_per_file = case.bytes / case.line.len();
    let file = case.line.repeat(rows_per_file);
    let (start, end) = columns(case.line, &case.encoding);
    let per_file = case.locations / case.files;
    let mut at = Vec::new();
    for n in 0..case.files {
        let rel = format!("src/gen{n}.rs");
        std::fs::write(s.root.join(&rel), &file).unwrap();
        let uri = s.uri(&rel);
        // Spread the locations down the file.
        for k in 0..per_file {
            let row = (k * rows_per_file / per_file) as u32;
            at.push(location(&uri, range(row, start, end)));
        }
    }
    assert_eq!(at.len(), case.locations);
    let method = match case.kind {
        Kind::Definitions => "textDocument/definition",
        Kind::References => "textDocument/references",
    };
    // The fake holds each reply on its own clock until the timer starts, so
    // the interval begins when the reply is released.
    let held = |value: Value| Scripted {
        reply: Reply::Result(value),
        delay: HOLD,
    };
    s.fake.set_reply(method, held(Value::Array(at)));
    if matches!(case.kind, Kind::References) {
        // The companion request that marks definitions.
        s.fake
            .set_reply("textDocument/definition", held(Value::Array(Vec::new())));
    }
    let requests = match case.kind {
        Kind::Definitions => 1,
        Kind::References => 2,
    };
    let req = s.request();
    let lsp = s.lsp.clone();

    (1..=RUNS)
        .map(|run| {
            let (sender, answer) = std::sync::mpsc::channel();
            let (lsp, req) = (lsp.clone(), req.clone());
            let kind = case.kind;
            std::thread::spawn(move || {
                let cancel = AtomicBool::new(false);
                let found = match kind {
                    Kind::Definitions => lsp.definitions(&req, &cancel).unwrap().items.len(),
                    Kind::References => {
                        let mut found = 0;
                        lsp.references(&req, &cancel, &mut |batch| {
                            found += batch.iter().map(|f| f.refs.len()).sum::<usize>();
                        })
                        .unwrap();
                        found
                    }
                };
                let _ = sender.send((found, Instant::now()));
            });
            s.fake.wait_for("this run's requests", |records| {
                let sent = records
                    .iter()
                    .filter(|r| {
                        matches!(r, Record::Request { method: m, .. }
                        if m == method || m == "textDocument/definition")
                    })
                    .count();
                sent >= requests * run
            });
            // Every reply of this run must have armed its hold before the
            // clock moves, or a late one would arm a timer nothing advances.
            let deadline = Instant::now() + WAIT;
            while s
                .fake_clock
                .sleeping()
                .iter()
                .filter(|left| **left == HOLD)
                .count()
                < requests
            {
                assert!(Instant::now() < deadline, "{}: replies not held", case.name);
                std::thread::sleep(Duration::from_millis(1));
            }
            let released = Instant::now();
            s.fake_clock.advance(HOLD);
            let (found, done) = answer.recv_timeout(WAIT).unwrap();
            assert_eq!(found, case.locations, "{}", case.name);
            done - released
        })
        .collect()
}

#[test]
#[ignore = "prints timings; run with --release --ignored --nocapture"]
fn decode_cost() {
    let cases = [
        Case {
            name: "definitions: 50 locations, 50 files of 4 KB",
            kind: Kind::Definitions,
            encoding: PositionEncodingKind::UTF8,
            files: 50,
            bytes: 4 * 1024,
            locations: 50,
            line: ASCII,
        },
        Case {
            name: "references: 1,000 locations, 500 files of 4 KB",
            kind: Kind::References,
            encoding: PositionEncodingKind::UTF8,
            files: 500,
            bytes: 4 * 1024,
            locations: 1000,
            line: ASCII,
        },
        Case {
            name: "references: 1,000 locations, one 10 MB file",
            kind: Kind::References,
            encoding: PositionEncodingKind::UTF8,
            files: 1,
            bytes: 10 * 1024 * 1024,
            locations: 1000,
            line: ASCII,
        },
        Case {
            name: "references: 1,000 locations, one 10 MB non-ASCII file, UTF-16",
            kind: Kind::References,
            encoding: PositionEncodingKind::UTF16,
            files: 1,
            bytes: 10 * 1024 * 1024,
            locations: 1000,
            line: WIDE,
        },
    ];
    for case in &cases {
        let mut times = run(case);
        times.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        println!(
            "{}: fastest {:.2} ms, median {:.2} ms",
            case.name,
            ms(times[0]),
            ms((times[RUNS / 2 - 1] + times[RUNS / 2]) / 2),
        );
    }
}
