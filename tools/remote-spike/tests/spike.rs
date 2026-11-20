//! End to end: the spike binary's server mode as a child, over its stdio.

use remote_spike::client::{Conn, Event};
use remote_spike::frame::{self, Body, CHUNK, Hello, Kind, Op, PROTOCOL, Status};
use remote_spike::transport::{ChildStdio, Delayed, Link, Transport};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BUILD: &str = remote_spike::bench::BUILD;
const WINDOW: u32 = 4 * CHUNK as u32;

fn child(one_way_ms: u64) -> Delayed<ChildStdio> {
    Delayed {
        inner: ChildStdio {
            program: env!("CARGO_BIN_EXE_remote-spike").into(),
            args: vec!["server".into()],
        },
        one_way: Duration::from_millis(one_way_ms),
        bytes_per_sec: None,
    }
}

fn echo(conn: &Conn, chan: u16) -> Vec<u8> {
    let mut s = conn.request(chan, &Op::Echo(b"hi".to_vec())).unwrap();
    let mut out = Vec::new();
    while let Some(ev) = s.recv() {
        match ev {
            Event::Data(Body::Bytes(b), _) => out.extend(b),
            Event::End(st) => assert_eq!(st, Status::Ok),
            Event::Data(..) => panic!("entries from an echo"),
        }
    }
    out
}

#[test]
fn a_stalled_channel_does_not_block_another_and_cancel_stops_within_its_window() {
    let conn = Conn::connect(&child(0), BUILD, WINDOW).unwrap();
    // Channel 2 streams forever and nobody reads it, so it runs out of credit.
    let mut bulk = conn.request(2, &Op::Bulk { bytes: u64::MAX }).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // Channel 1 still answers.
    assert_eq!(echo(&conn, 1), b"hi");
    // Everything channel 2 ever got, before and after the cancel, fits its window.
    let (st, bytes, _) = bulk.cancel().unwrap();
    assert_eq!(st, Some(Status::Cancelled));
    assert!(
        bytes <= WINDOW as usize,
        "{bytes} bytes past a {WINDOW} window"
    );
    assert!(bytes >= CHUNK, "the stream never started");
    // Credit came back: the channel works again.
    assert_eq!(echo(&conn, 2), b"hi");
}

#[test]
fn a_walk_streams_every_entry_and_a_file_reads_whole() {
    let dir = std::env::temp_dir().join(format!("remote-spike-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in 0..5 {
        std::fs::create_dir_all(dir.join(format!("d{d}"))).unwrap();
        for f in 0..300 {
            std::fs::write(dir.join(format!("d{d}/f{f}")), b"").unwrap();
        }
    }
    let big: Vec<u8> = (0..300_000u32).map(|i| i as u8).collect();
    std::fs::write(dir.join("big"), &big).unwrap();

    let conn = Conn::connect(&child(0), BUILD, WINDOW).unwrap();
    let mut walk = conn
        .request(
            1,
            &Op::Walk {
                root: dir.to_string_lossy().into(),
            },
        )
        .unwrap();
    let mut n = 0;
    while let Some(ev) = walk.recv() {
        match ev {
            Event::Data(Body::Entries(e), _) => n += e.len(),
            Event::End(st) => assert_eq!(st, Status::Ok),
            _ => panic!(),
        }
    }
    assert_eq!(n, 5 + 5 * 300 + 1);

    let mut read = conn
        .request(
            2,
            &Op::ReadFile {
                path: dir.join("big").to_string_lossy().into(),
            },
        )
        .unwrap();
    let mut got = Vec::new();
    while let Some(ev) = read.recv() {
        if let Event::Data(Body::Bytes(b), _) = ev {
            got.extend(b);
        }
    }
    assert!(got == big);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn injected_latency_shows_in_the_round_trip() {
    let conn = Conn::connect(&child(20), BUILD, WINDOW).unwrap();
    let t = Instant::now();
    let rtt = conn.ping().unwrap();
    assert!(rtt >= Duration::from_millis(40), "{rtt:?}");
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_different_build_is_refused_at_the_handshake() {
    let err = Conn::connect(&child(0), "9.9.9", WINDOW).err().unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused);
    assert!(err.to_string().contains("9.9.9"), "{err}");
}

#[test]
fn an_echo_larger_than_the_window_completes_whole() {
    let payload: Vec<u8> = (0..300 * 1024u32).map(|i| (i * 7) as u8).collect();
    assert!(payload.len() > WINDOW as usize);
    let (tx, rx) = mpsc::channel();
    let sent = payload.clone();
    std::thread::spawn(move || {
        let conn = Conn::connect(&child(0), BUILD, WINDOW).unwrap();
        let mut s = conn.request(1, &Op::Echo(sent)).unwrap();
        let (mut got, mut largest, mut end) = (Vec::new(), 0, None);
        while let Some(ev) = s.recv() {
            match ev {
                Event::Data(Body::Bytes(b), _) => {
                    largest = largest.max(b.len());
                    got.extend(b);
                }
                Event::End(st) => end = Some(st),
                Event::Data(..) => panic!("entries from an echo"),
            }
        }
        tx.send((got, largest, end)).unwrap();
    });
    let (got, largest, end) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("an echo over the window stalled on credit");
    assert_eq!(end, Some(Status::Ok));
    assert!(largest <= CHUNK, "a {largest}-byte data frame");
    assert!(got == payload);
}

/// A server that accepts any Hello and answers as another build.
struct LyingServer;

impl Transport for LyingServer {
    fn open(&self) -> std::io::Result<Link> {
        let (mut from_client, to_server) = std::io::pipe()?;
        let (from_server, mut to_client) = std::io::pipe()?;
        std::thread::spawn(move || {
            let Ok(Some((_, b))) = frame::read_frame(&mut from_client) else {
                return;
            };
            let hello: Hello = frame::body(&b).unwrap();
            let welcome = Hello {
                protocol: PROTOCOL,
                build: "9.9.9+other".into(),
                window: hello.window,
            };
            let _ = frame::write_frame(
                &mut to_client,
                &frame::encode(Kind::Welcome, 0, 0, &welcome),
            );
            // Hold the pipes until the client hangs up.
            let _ = frame::read_frame(&mut from_client);
        });
        Ok(Link {
            reader: Box::new(from_server),
            writer: Box::new(to_server),
            child: None,
        })
    }
}

#[test]
fn a_server_answering_a_different_build_is_refused_by_the_client() {
    let err = Conn::connect(&LyingServer, BUILD, WINDOW).err().unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("9.9.9+other"), "{err}");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("remote-spike-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn spike(args: &[&str], dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_remote-spike"))
        .args(args)
        .arg("--dir")
        .arg(dir)
        .output()
        .unwrap()
}

#[test]
fn the_fixture_never_deletes_a_directory_it_did_not_make() {
    let dir = scratch("not-a-fixture");
    std::fs::write(dir.join("keep.txt"), b"mine").unwrap();
    std::fs::create_dir(dir.join("tree")).unwrap();
    std::fs::write(dir.join("tree/also-mine"), b"").unwrap();
    for args in [&["fixture"][..], &["bench", "--quick", "--latency", "0"]] {
        let out = spike(args, &dir);
        assert!(
            !out.status.success(),
            "{args:?} accepted a foreign directory"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("not empty"), "{args:?}: {err}");
        assert_eq!(std::fs::read(dir.join("keep.txt")).unwrap(), b"mine");
        assert!(dir.join("tree/also-mine").exists(), "{args:?}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_finished_fixture_is_reused_and_its_other_files_are_kept() {
    let dir = scratch("made-fixture");
    std::fs::write(
        dir.join(".remote-spike-fixture"),
        "made by remote-spike fixture\n",
    )
    .unwrap();
    std::fs::write(dir.join(".complete"), b"").unwrap();
    std::fs::write(dir.join("notes.txt"), b"mine").unwrap();
    let (tree, big) = remote_spike::bench::fixture(&dir).unwrap();
    assert_eq!((tree, big), (dir.join("tree"), dir.join("one-mib.bin")));
    assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"mine");
    std::fs::remove_dir_all(&dir).unwrap();
}
