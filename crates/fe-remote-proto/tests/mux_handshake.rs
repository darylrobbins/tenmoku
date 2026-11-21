//! The handshake (D § 2.5): protocol and build must both match, and both
//! peers check. Ported from `tools/remote-spike/tests/spike.rs`, against
//! the `fe-remote-proto-peer` binary over its stdio.

use fe_remote_proto::testing::{self, RawPeer};
use fe_remote_proto::window::INITIAL_WINDOW;
use fe_remote_proto::{
    BUILD, Config, ControlOp, Event, FsData, FsOp, HandshakeError, Kind, Link, Mux, OpenTarget,
    Opened, PROTOCOL, PathBytes, Refused, SessionToken, Status, Welcome, body, chan,
};
use std::io;
use std::process::Command;
use std::thread;

fn peer(args: &[&str]) -> Link {
    Link::spawn(Command::new(env!("CARGO_BIN_EXE_fe-remote-proto-peer")).args(args)).unwrap()
}

fn connect(link: Link, config: Config) -> Result<Mux, HandshakeError> {
    Mux::client(link, config, testing::location(), None).map(|(mux, _)| mux)
}

fn build(build: &str) -> Config {
    Config {
        build: build.into(),
        ..Config::default()
    }
}

#[test]
fn a_different_build_is_refused_at_the_handshake() {
    let err = connect(peer(&[]), build("9.9.9")).err().unwrap();
    let HandshakeError::Refused(why) = &err else {
        panic!("{err}");
    };
    assert!(why.contains("\"9.9.9\"") && why.contains(BUILD), "{why}");
    assert_eq!(
        io::Error::from(err).kind(),
        io::ErrorKind::ConnectionRefused
    );
}

#[test]
fn a_different_protocol_is_refused_at_the_handshake() {
    let config = Config {
        protocol: PROTOCOL + 1,
        ..Config::default()
    };
    let err = connect(peer(&[]), config).err().unwrap();
    let HandshakeError::Refused(why) = &err else {
        panic!("{err}");
    };
    assert!(
        why.contains(&format!("client protocol {}", PROTOCOL + 1)),
        "{why}"
    );
}

/// A fake server that accepts any Hello and answers as `build` speaking
/// `protocol`, then holds the link until the client hangs up. Returns
/// whether it saw the hang-up.
fn lying_server(link: Link, protocol: u32, build: &str) -> thread::JoinHandle<bool> {
    let build = build.to_string();
    thread::spawn(move || {
        let mut raw = RawPeer::new(link);
        let (h, _) = raw.recv().unwrap();
        assert_eq!(h.kind, Kind::Hello);
        let welcome = Welcome {
            protocol,
            build,
            window: INITIAL_WINDOW,
            session: SessionToken([1; 16]),
            streams: Vec::new(),
        };
        raw.send(Kind::Welcome, chan::CONTROL, 0, &welcome);
        raw.recv().is_none()
    })
}

#[test]
fn a_server_answering_a_different_build_is_refused_by_the_client() {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = lying_server(theirs, PROTOCOL, "9.9.9+other");
    let err = connect(ours, Config::default()).err().unwrap();
    assert!(
        matches!(&err, HandshakeError::Mismatch { build, .. } if build == "9.9.9+other"),
        "{err}"
    );
    assert!(err.to_string().contains("9.9.9+other"), "{err}");
    assert_eq!(io::Error::from(err).kind(), io::ErrorKind::InvalidData);
    assert!(server.join().unwrap(), "the client kept the link open");
}

#[test]
fn a_server_answering_a_different_protocol_is_refused_by_the_client() {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = lying_server(theirs, PROTOCOL + 1, BUILD);
    let err = connect(ours, Config::default()).err().unwrap();
    assert!(
        matches!(&err, HandshakeError::Mismatch { protocol, .. } if *protocol == PROTOCOL + 1),
        "{err}"
    );
    assert!(server.join().unwrap(), "the client kept the link open");
}

#[test]
fn a_server_refuses_a_hello_of_another_build_with_the_reason() {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || {
        Mux::server(theirs, Config::default(), testing::accept_any)
            .err()
            .unwrap()
    });
    let mut raw = RawPeer::new(ours);
    let hello = fe_remote_proto::Hello {
        protocol: PROTOCOL,
        build: "0.0.0-other".into(),
        window: INITIAL_WINDOW,
        resume: None,
        location: testing::location(),
    };
    raw.send(Kind::Hello, chan::CONTROL, 0, &hello);
    let (h, b) = raw.recv().unwrap();
    assert_eq!(h.kind, Kind::Refused);
    let refused: Refused = body(&h, &b).unwrap();
    assert_eq!(
        refused.reason,
        format!(
            "client protocol {PROTOCOL} build \"0.0.0-other\", server protocol {PROTOCOL} build {BUILD:?}"
        )
    );
    // ... and closes.
    assert!(raw.recv().is_none());
    assert!(matches!(server.join().unwrap(), HandshakeError::Refused(r) if r == refused.reason));
}

#[test]
fn the_server_can_refuse_a_matching_hello() {
    let (ours, theirs) = Link::pipe().unwrap();
    thread::spawn(move || {
        let _ = Mux::server(theirs, Config::default(), |_| Err("no such session".into()));
    });
    let err = connect(ours, Config::default()).err().unwrap();
    assert!(
        matches!(&err, HandshakeError::Refused(r) if r == "no such session"),
        "{err}"
    );
}

#[test]
fn a_first_frame_that_is_not_a_hello_is_refused() {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || {
        Mux::server(theirs, Config::default(), testing::accept_any)
            .err()
            .unwrap()
    });
    let mut raw = RawPeer::new(ours);
    raw.send(Kind::Ping, chan::CONTROL, 1, &());
    assert!(matches!(
        server.join().unwrap(),
        HandshakeError::Unexpected(Kind::Ping)
    ));
}

#[test]
fn a_matching_build_is_welcomed_and_carries_requests_and_streams() {
    let (mux, welcome) =
        Mux::client(peer(&[]), Config::default(), testing::location(), None).unwrap();
    assert_eq!(
        (welcome.protocol, welcome.build.as_str()),
        (PROTOCOL, BUILD)
    );

    let mut read = mux
        .request(
            chan::FS,
            &FsOp::Read {
                path: PathBytes(b"hello".to_vec()),
            },
        )
        .unwrap();
    let Some(Event::Data(body)) = read.recv() else {
        panic!("no data");
    };
    assert_eq!(
        read.decode::<FsData>(&body).unwrap(),
        FsData::Bytes(b"hello".to_vec())
    );
    assert_eq!(read.recv(), Some(Event::End(Status::Ok)));
    assert_eq!(read.recv(), None);

    let mut open = mux
        .request(
            chan::CONTROL,
            &ControlOp::Open(OpenTarget::Tcp {
                host: "echo".into(),
                port: 0,
            }),
        )
        .unwrap();
    let Some(Event::Data(body)) = open.recv() else {
        panic!("no channel");
    };
    let opened: Opened = open.decode(&body).unwrap();
    assert_eq!(open.recv(), Some(Event::End(Status::Ok)));
    let mut echo = mux.stream(opened.chan).unwrap();
    echo.send_bytes(b"over the child's stdio").unwrap();
    assert_eq!(
        echo.recv(),
        Some(Event::Data(b"over the child's stdio".to_vec()))
    );
    echo.end(Status::Ok).unwrap();
    assert_eq!(echo.recv(), Some(Event::End(Status::Ok)));
    mux.close();
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[test]
fn a_mux_whose_threads_fail_to_start_kills_and_reaps_its_child() {
    for started in [0, 1] {
        let link = peer(&[]);
        let pid = link.child.as_ref().unwrap().id();
        testing::fail_mux_threads_after(started);
        let err = connect(link, Config::default()).err().unwrap();
        assert!(matches!(err, HandshakeError::Io(_)), "{err}");
        assert!(!alive(pid), "child {pid} left after {started} threads");
    }
}

#[test]
fn a_mux_whose_threads_fail_to_start_hangs_up() {
    for started in [0, 1, 2] {
        let (ours, theirs) = Link::pipe().unwrap();
        let server = thread::spawn(move || {
            let mut raw = RawPeer::new(theirs);
            raw.welcome(BUILD, INITIAL_WINDOW);
            while raw.recv().is_some() {}
        });
        testing::fail_mux_threads_after(started);
        let err = connect(ours, Config::default()).err().unwrap();
        assert!(matches!(err, HandshakeError::Io(_)), "{err}");
        // The server sees the link close.
        server.join().unwrap();
    }
}
