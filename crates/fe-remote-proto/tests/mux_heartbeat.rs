//! The heartbeat (D § 2.5): a ping every 10 s, the link lost after two
//! missed pongs or at EOF, and the round trip exposed for the pill and the
//! bulk window. Timing runs on an injected clock, never on sleeps.

use fe_remote_proto::clock::{Clock, ManualClock};
use fe_remote_proto::heartbeat::{Beat, HEARTBEAT, Heartbeat, MISSED_PONGS, Rtt};
use fe_remote_proto::testing::{self, Latency, RawPeer};
use fe_remote_proto::window::INITIAL_WINDOW;
use fe_remote_proto::{
    BUILD, Config, Event, FsOp, Hello, Kind, Link, Lost, Mux, MuxError, PROTOCOL, PathBytes,
    SERVER_ORIGINATED, SessionToken, Status, Welcome, chan, encode,
};
use std::thread;
use std::time::{Duration, Instant};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_heartbeat_pings_at_the_start_and_every_ten_seconds() {
    assert_eq!(HEARTBEAT, Duration::from_secs(10));
    let t0 = Instant::now();
    let mut hb = Heartbeat::new(t0, HEARTBEAT, 0);
    assert_eq!(hb.tick(t0), Some(Beat::Ping(1)));
    assert_eq!(hb.tick(t0 + ms(1)), None);
    hb.pong(1, t0 + ms(30));
    assert_eq!(hb.due(), t0 + HEARTBEAT);
    assert_eq!(hb.tick(t0 + HEARTBEAT - ms(1)), None);
    assert_eq!(hb.tick(t0 + HEARTBEAT), Some(Beat::Ping(2)));
    hb.pong(2, t0 + HEARTBEAT + ms(30));
    assert_eq!(hb.tick(t0 + 2 * HEARTBEAT), Some(Beat::Ping(3)));
}

#[test]
fn two_missed_pongs_in_a_row_lose_the_link() {
    assert_eq!(MISSED_PONGS, 2);
    let t0 = Instant::now();
    let mut hb = Heartbeat::new(t0, HEARTBEAT, 0);
    assert_eq!(hb.tick(t0), Some(Beat::Ping(1)));
    assert_eq!(hb.tick(t0 + HEARTBEAT), Some(Beat::Ping(2)));
    assert_eq!(hb.tick(t0 + 2 * HEARTBEAT), Some(Beat::Lost));
}

#[test]
fn a_pong_resets_the_missed_count() {
    let t0 = Instant::now();
    let mut hb = Heartbeat::new(t0, HEARTBEAT, 0);
    hb.tick(t0);
    hb.tick(t0 + HEARTBEAT);
    // The first ping's pong, late but still waited for.
    assert_eq!(hb.pong(1, t0 + ms(12_000)), Some(ms(12_000)));
    assert_eq!(hb.tick(t0 + 2 * HEARTBEAT), Some(Beat::Ping(3)));
    // The second and third pings go unanswered: two in a row.
    assert_eq!(hb.tick(t0 + 3 * HEARTBEAT), Some(Beat::Lost));
}

#[test]
fn the_rtt_is_the_latest_round_trip_and_the_least() {
    let t0 = Instant::now();
    let mut hb = Heartbeat::new(t0, HEARTBEAT, 0);
    assert_eq!(hb.rtt(), None);
    for (i, rtt) in [50, 30, 40].into_iter().enumerate() {
        let at = t0 + HEARTBEAT * i as u32;
        let Some(Beat::Ping(nonce)) = hb.tick(at) else {
            panic!("no ping");
        };
        assert_eq!(hb.pong(nonce, at + ms(rtt)), Some(ms(rtt)));
    }
    assert_eq!(
        hb.rtt(),
        Some(Rtt {
            latest: ms(40),
            min: ms(30)
        })
    );
}

#[test]
fn a_pong_nobody_waits_for_is_ignored() {
    let t0 = Instant::now();
    let mut hb = Heartbeat::new(t0, HEARTBEAT, 0);
    hb.tick(t0);
    assert_eq!(hb.pong(99, t0 + ms(5)), None);
    assert_eq!(hb.pong(1, t0 + ms(5)), Some(ms(5)));
    // Answered once is enough.
    assert_eq!(hb.pong(1, t0 + ms(6)), None);
    assert_eq!(hb.rtt().unwrap().latest, ms(5));
}

#[test]
fn a_timer_that_wakes_late_owes_one_ping() {
    let t0 = Instant::now();
    let mut hb = Heartbeat::new(t0, HEARTBEAT, 0);
    hb.tick(t0);
    hb.pong(1, t0 + ms(1));
    assert_eq!(hb.tick(t0 + ms(35_000)), Some(Beat::Ping(2)));
    assert_eq!(hb.due(), t0 + 4 * HEARTBEAT);
    assert_eq!(hb.tick(t0 + ms(35_001)), None);
}

#[test]
fn the_servers_pings_have_the_high_bit() {
    let t0 = Instant::now();
    let mut hb = Heartbeat::new(t0, HEARTBEAT, SERVER_ORIGINATED);
    assert_eq!(hb.tick(t0), Some(Beat::Ping(SERVER_ORIGINATED | 1)));
}

/// A client mux on `clock` pinging every 10 s, and the raw server it
/// handshook with.
fn client_on(clock: &ManualClock) -> (Mux, RawPeer) {
    let (ours, theirs) = Link::pipe().unwrap();
    let raw = thread::spawn(move || {
        let mut raw = RawPeer::new(theirs);
        raw.welcome(BUILD, INITIAL_WINDOW);
        raw
    });
    let config = Config {
        clock: Clock::Manual(clock.clone()),
        ..Config::default()
    };
    let (client, _) = Mux::client(ours, config, testing::location(), None).unwrap();
    (client, raw.join().unwrap())
}

#[test]
fn the_mux_pings_on_its_clock_and_exposes_the_rtt() {
    let clock = ManualClock::new();
    let (client, mut raw) = client_on(&clock);
    // The first ping goes at once, so the pill has a round trip early.
    let ((first, _), _) = raw.recv_kind(Kind::Ping);
    assert_eq!(first.chan, chan::CONTROL);
    clock.advance(ms(34));
    raw.send(Kind::Pong, chan::CONTROL, first.id, &());
    raw.barrier(100);
    assert_eq!(
        client.rtt(),
        Some(Rtt {
            latest: ms(34),
            min: ms(34)
        })
    );
    // Nothing more until ten seconds after the first.
    clock.advance(HEARTBEAT - ms(35));
    let before = raw.barrier(101);
    assert!(before.iter().all(|h| h.kind != Kind::Ping), "{before:?}");
    clock.advance(ms(1));
    let ((second, _), _) = raw.recv_kind(Kind::Ping);
    assert_ne!(second.id, first.id);
    clock.advance(ms(50));
    raw.send(Kind::Pong, chan::CONTROL, second.id, &());
    raw.barrier(102);
    assert_eq!(
        client.rtt(),
        Some(Rtt {
            latest: ms(50),
            min: ms(34)
        })
    );
    assert_eq!(client.lost(), None);
}

#[test]
fn a_peer_that_never_pongs_is_lost_after_two_missed_pongs() {
    let clock = ManualClock::new();
    let (client, mut raw) = client_on(&clock);
    raw.recv_kind(Kind::Ping);
    clock.advance(HEARTBEAT);
    // One missed: the next ping goes, and the link holds.
    raw.recv_kind(Kind::Ping);
    raw.barrier(100);
    assert_eq!(client.lost(), None);
    clock.advance(HEARTBEAT);
    assert_eq!(client.wait_lost(), Lost::MissedPongs);
    // ... and the client hangs up.
    while raw.recv().is_some() {}
}

#[test]
fn eof_reports_the_link_lost() {
    let clock = ManualClock::new();
    let (client, mut raw) = client_on(&clock);
    raw.hang_up();
    assert_eq!(client.wait_lost(), Lost::Eof);
}

#[test]
fn a_lost_link_ends_what_is_open_with_link_lost() {
    let clock = ManualClock::new();
    let (client, mut raw) = client_on(&clock);
    let mut read = client
        .request(
            chan::FS,
            &FsOp::Read {
                path: PathBytes(b"f".to_vec()),
            },
        )
        .unwrap();
    let mut stream = client.stream(64).unwrap();
    raw.recv_kind(Kind::Request);
    raw.hang_up();
    assert_eq!(read.recv(), Some(Event::End(Status::LinkLost)));
    assert_eq!(read.recv(), None);
    assert_eq!(stream.recv(), Some(Event::End(Status::LinkLost)));
    assert_eq!(stream.send_bytes(b"x"), Err(MuxError::LinkLost(Lost::Eof)));
    assert!(matches!(
        client.request(
            chan::FS,
            &FsOp::Read {
                path: PathBytes(Vec::new())
            }
        ),
        Err(MuxError::LinkLost(Lost::Eof))
    ));
    assert!(client.accept().is_none());
}

#[test]
fn the_server_pings_too_with_the_high_bit() {
    let clock = ManualClock::new();
    let (ours, theirs) = Link::pipe().unwrap();
    let config = Config {
        clock: Clock::Manual(clock.clone()),
        ..Config::default()
    };
    let server = thread::spawn(move || Mux::server(theirs, config, testing::accept_any).unwrap().0);
    let mut raw = RawPeer::new(ours);
    raw.hello(BUILD, INITIAL_WINDOW);
    let server = server.join().unwrap();
    let ((ping, _), _) = raw.recv_kind(Kind::Ping);
    assert_eq!(ping.id & SERVER_ORIGINATED, SERVER_ORIGINATED);
    clock.advance(ms(7));
    raw.send(Kind::Pong, chan::CONTROL, ping.id, &());
    raw.barrier(1);
    assert_eq!(server.rtt().unwrap().latest, ms(7));
}

#[test]
fn injected_latency_shows_in_the_round_trip() {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || {
        let (mux, _) = Mux::server(theirs, Config::default(), testing::accept_any).unwrap();
        testing::serve(&mux);
    });
    let ours = testing::delayed(ours, Latency::one_way(ms(20)));
    let t = Instant::now();
    let (client, _) = Mux::client(ours, Config::default(), testing::location(), None).unwrap();
    // The handshake is one round trip.
    assert!(t.elapsed() >= ms(40), "{:?}", t.elapsed());
    // The first ping went at once; requests pace the wait for its pong.
    let mut reads = 0;
    let rtt = loop {
        if let Some(rtt) = client.rtt() {
            break rtt;
        }
        let t = Instant::now();
        let mut read = client
            .request(
                chan::FS,
                &FsOp::Read {
                    path: PathBytes(b"x".to_vec()),
                },
            )
            .unwrap();
        while read.recv().is_some() {}
        assert!(t.elapsed() >= ms(40), "{:?}", t.elapsed());
        reads += 1;
        assert!(reads < 500, "no pong");
    };
    assert!(rtt.min >= ms(40), "{rtt:?}");
    assert!(rtt.min < Duration::from_secs(5), "{rtt:?}");
    drop(client);
    server.join().unwrap();
}

#[test]
fn a_link_lost_to_its_heartbeat_takes_no_more_requests() {
    let clock = ManualClock::new();
    let (client, mut raw) = client_on(&clock);
    // The first ping, before the clock moves, so the misses count from it.
    raw.recv_kind(Kind::Ping);
    let read = FsOp::Read {
        path: PathBytes(b"f".to_vec()),
    };
    raw.send(Kind::Request, chan::FS, SERVER_ORIGINATED | 1, &read);
    raw.barrier(100);
    // The peer keeps its end open but stops answering.
    clock.advance(HEARTBEAT);
    raw.recv_kind(Kind::Ping);
    clock.advance(HEARTBEAT);
    assert_eq!(client.wait_lost(), Lost::MissedPongs);
    // The request that was waiting is dropped, not handed out.
    assert!(client.accept().is_none());
    assert!(futures::executor::block_on(client.accept_async()).is_none());
    // ... and unanswered: nothing more crosses the link.
    let rest: Vec<_> = std::iter::from_fn(|| raw.recv()).map(|(h, _)| h).collect();
    assert!(rest.iter().all(|h| h.kind != Kind::End), "{rest:?}");
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[test]
fn a_child_that_stops_reading_is_killed_and_reaped() {
    let dir = tempfile::tempdir().unwrap();
    let welcome = Welcome {
        protocol: PROTOCOL,
        build: BUILD.into(),
        window: INITIAL_WINDOW,
        session: SessionToken([1; 16]),
        streams: Vec::new(),
    };
    std::fs::write(
        dir.path().join("welcome"),
        encode(Kind::Welcome, chan::CONTROL, 0, &welcome).unwrap(),
    )
    .unwrap();
    let hello = Hello {
        protocol: PROTOCOL,
        build: BUILD.into(),
        window: INITIAL_WINDOW,
        resume: None,
        location: testing::location(),
    };
    let hello_len = encode(Kind::Hello, chan::CONTROL, 0, &hello).unwrap().len();
    // Takes the Hello, answers, then closes its stdin and keeps its stdout
    // open.
    let script = format!("head -c {hello_len} >/dev/null; cat welcome; exec sleep 600 <&-");
    let link = Link::spawn(
        std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir.path()),
    )
    .unwrap();
    let pid = link.child.as_ref().unwrap().id();
    let config = Config {
        heartbeat: None,
        ..Config::default()
    };
    let (client, _) = Mux::client(link, config, testing::location(), None).unwrap();
    // Requests until writing them fails on the closed stdin.
    let mut held = Vec::new();
    while client.lost().is_none() {
        assert!(held.len() < 1_000_000, "the writer never failed");
        if let Ok(s) = client.request(chan::FS, &read_of(b"x")) {
            held.push(s);
        }
        thread::yield_now();
    }
    assert!(
        matches!(client.wait_lost(), Lost::Io(_)),
        "{:?}",
        client.lost()
    );
    // Closing waits for the reader, which the killed child releases.
    client.close();
    assert!(!alive(pid), "child {pid} is still there");
}

fn read_of(path: &[u8]) -> FsOp {
    FsOp::Read {
        path: PathBytes(path.to_vec()),
    }
}
