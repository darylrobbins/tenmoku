//! The daemon's side of the handshake (D § 2.5): a strict match before any
//! session is made, a fresh session for a resume it can't honour; and the
//! control channel, the only service so far.

#![cfg(unix)]

mod common;

use common::{Host, client_config, handshake, location, run_daemon, socket_link};
use fe_remote_proto::testing::RawPeer;
use fe_remote_proto::{
    BUILD, Config as MuxConfig, ControlOp, Event, FsOp, HandshakeError, IoKind, Kind, Link,
    OpenTarget, PROTOCOL, PathBytes, Resume, SessionToken, Status, StreamOffset, chan,
};
use std::thread;
use tenmoku_server::{Config, serve};

fn refused(result: Result<impl Sized, HandshakeError>) -> String {
    match result {
        Err(HandshakeError::Refused(why)) => why,
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("the Hello was taken"),
    }
}

#[test]
fn a_hello_of_another_build_is_refused_before_any_session_is_made() {
    let host = Host::new();
    let running = run_daemon(&host, Config::default());
    let config = MuxConfig {
        build: "9.9.9+other".into(),
        ..client_config()
    };
    let why = refused(handshake(socket_link(&host.paths()), config, None));
    assert!(
        why.contains("\"9.9.9+other\"") && why.contains(BUILD),
        "{why}"
    );
    let held = running.daemon.snapshot();
    assert!(
        held.sessions.is_empty() && held.torn_down.is_empty(),
        "{held:?}"
    );
}

#[test]
fn a_hello_of_another_protocol_is_refused_before_any_session_is_made() {
    let host = Host::new();
    let running = run_daemon(&host, Config::default());
    let config = MuxConfig {
        protocol: PROTOCOL + 1,
        ..client_config()
    };
    let why = refused(handshake(socket_link(&host.paths()), config, None));
    assert!(
        why.contains(&format!("client protocol {}", PROTOCOL + 1)),
        "{why}"
    );
    let held = running.daemon.snapshot();
    assert!(
        held.sessions.is_empty() && held.torn_down.is_empty(),
        "{held:?}"
    );
}

#[test]
fn resuming_an_unknown_session_starts_a_fresh_one() {
    let host = Host::new();
    let running = run_daemon(&host, Config::default());
    let unknown = SessionToken([9; 16]);
    let resume = Resume {
        session: unknown,
        streams: vec![StreamOffset {
            chan: chan::FIRST_DYNAMIC,
            recv_next: 4096,
        }],
    };
    let (_client, welcome) =
        handshake(socket_link(&host.paths()), client_config(), Some(resume)).unwrap();
    assert_ne!(welcome.session, unknown);
    assert!(welcome.streams.is_empty());
    let held = running.daemon.snapshot();
    let [session] = &held.sessions[..] else {
        panic!("{held:?}");
    };
    assert_eq!(session.token, welcome.session);
    assert!(session.attached);
    assert_eq!(session.destination, location().destination);
    assert_eq!(session.path, location().path);
}

#[test]
fn resuming_a_session_in_its_grace_attaches_to_it() {
    let host = Host::new();
    let running = run_daemon(&host, Config::default());
    let daemon = &running.daemon;
    let (client, welcome) = handshake(socket_link(&host.paths()), client_config(), None).unwrap();
    let token = welcome.session;
    drop(client);
    assert!(daemon.wait_for(|s| s.session(token).is_some_and(|s| !s.attached)));

    let resume = Resume {
        session: token,
        streams: Vec::new(),
    };
    let (_client, again) =
        handshake(socket_link(&host.paths()), client_config(), Some(resume)).unwrap();
    assert_eq!(again.session, token);
    let held = daemon.snapshot();
    assert_eq!(held.sessions.len(), 1, "{held:?}");
    assert!(held.sessions[0].attached);
}

#[test]
fn resuming_a_session_past_its_grace_starts_a_fresh_one() {
    let host = Host::new();
    let grace = std::time::Duration::from_secs(60);
    // No reaper runs: only the resume itself can see the grace is over.
    let (daemon, clock) = common::claim_daemon(
        &host,
        Config {
            grace,
            ..Config::default()
        },
    );
    let (client, welcome, served) = common::serve_client(&daemon, None);
    let token = welcome.session;
    client.close();
    assert_eq!(served.join().unwrap().unwrap(), token);

    clock.advance(grace);
    let resume = Resume {
        session: token,
        streams: Vec::new(),
    };
    let (_client, again, _served) = common::serve_client(&daemon, Some(resume));
    assert_ne!(again.session, token);
    let held = daemon.snapshot();
    assert!(held.session(token).is_none(), "{held:?}");
    assert!(held.session(again.session).is_some_and(|s| s.attached));
}

#[test]
fn open_is_refused_with_a_clear_status() {
    let host = Host::new();
    let _running = run_daemon(&host, Config::default());
    let (client, _) = handshake(socket_link(&host.paths()), client_config(), None).unwrap();
    let open = ControlOp::Open(OpenTarget::Tcp {
        host: "localhost".into(),
        port: 8080,
    });
    let mut reply = client.request(chan::CONTROL, &open).unwrap();
    let Some(Event::End(Status::Error(e))) = reply.recv() else {
        panic!("Open wasn't refused");
    };
    assert_eq!(e.kind, IoKind::Unsupported);
    assert!(e.message.contains("localhost:8080"), "{}", e.message);
}

#[test]
fn a_service_not_built_yet_is_refused_with_a_clear_status() {
    let host = Host::new();
    let _running = run_daemon(&host, Config::default());
    let (client, _) = handshake(socket_link(&host.paths()), client_config(), None).unwrap();
    let stat = FsOp::Stat {
        path: PathBytes(b"/etc".to_vec()),
        follow: true,
    };
    let mut reply = client.request(chan::FS, &stat).unwrap();
    let Some(Event::End(Status::Error(e))) = reply.recv() else {
        panic!("the fs request wasn't refused");
    };
    assert_eq!(e.kind, IoKind::Unsupported);
    assert!(e.message.contains("no file service"), "{}", e.message);
}

#[test]
fn a_ping_is_answered() {
    let host = Host::new();
    let _running = run_daemon(&host, Config::default());
    let mut peer = RawPeer::new(socket_link(&host.paths()));
    peer.hello(BUILD, fe_remote_proto::window::INITIAL_WINDOW);
    peer.send(Kind::Ping, chan::CONTROL, 41, &());
    let ((pong, _), before) = peer.recv_kind(Kind::Pong);
    assert_eq!(pong.id, 41);
    assert!(before.is_empty(), "{before:?}");
}

#[test]
fn serve_runs_one_session_over_a_link_in_process() {
    let (client_end, server_end) = Link::pipe().unwrap();
    let server = thread::spawn(move || serve(server_end, Config::default()));
    let (client, welcome) = handshake(client_end, client_config(), None).unwrap();
    let mut reply = client
        .request(
            chan::CONTROL,
            &ControlOp::Open(OpenTarget::Tcp {
                host: "localhost".into(),
                port: 1,
            }),
        )
        .unwrap();
    assert!(matches!(reply.recv(), Some(Event::End(Status::Error(_)))));
    client.close();
    assert_eq!(server.join().unwrap().unwrap(), welcome.session);
}

#[test]
fn serve_refuses_another_build() {
    let (client_end, server_end) = Link::pipe().unwrap();
    let server = thread::spawn(move || serve(server_end, Config::default()));
    let config = MuxConfig {
        build: "9.9.9+other".into(),
        ..client_config()
    };
    refused(handshake(client_end, config, None));
    assert!(matches!(
        server.join().unwrap(),
        Err(HandshakeError::Refused(_))
    ));
}
