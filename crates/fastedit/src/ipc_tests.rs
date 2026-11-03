//! The handoff's wire format, socket, and what [`start`] makes of a launch.

use std::{cell::Cell, ffi::OsStr, fs, path::Path, thread};

use super::*;

fn target(path: &str, line: Option<u32>, col: Option<u32>) -> Target {
    Target {
        path: path.into(),
        line,
        col,
    }
}

fn request(targets: Vec<Target>) -> Request {
    Request {
        targets,
        new_window: false,
        wait: false,
        cwd: "/work".into(),
    }
}

/// A launch from a script, in `temp_dir`.
fn how(temp_dir: &Path) -> How {
    How {
        no_ipc: false,
        foreground: false,
        parent_carries: false,
        terminal: false,
        launch_services: false,
        temp_dir: temp_dir.to_owned(),
    }
}

fn no_detach() -> io::Result<()> {
    panic!("this launch doesn't detach")
}

fn served(start: Start) -> Server {
    match start {
        Start::Serve(server) => server,
        Start::Exit => panic!("exited, not serving"),
        Start::Alone => panic!("alone, not serving"),
        Start::ServerLost => panic!("the server went, not serving"),
    }
}

/// A live server under `temp_dir`, which takes every request, and what it took.
fn live_server(temp_dir: &Path) -> async_channel::Receiver<Request> {
    taking(served(start(&request(vec![]), &how(temp_dir), no_detach)))
}

/// Has `server` take every request, and gives what it took.
fn taking(server: Server) -> async_channel::Receiver<Request> {
    let handoffs = server.listen();
    let (taken, requests) = async_channel::unbounded();
    thread::spawn(move || {
        while let Ok(handoff) = handoffs.recv_blocking() {
            taken.send_blocking(handoff.request.clone()).unwrap();
            handoff.answer();
        }
    });
    requests
}

#[test]
fn request_round_trips_json_with_targets_positions_new_window_wait_cwd() {
    let sent = Request {
        targets: vec![
            target("/a b/c.rs", Some(3), Some(7)),
            target("/d\nnew.rs", Some(9), None),
            target("/e.rs", None, None),
            Target {
                // Not UTF-8, as Linux allows.
                path: OsStr::from_bytes(b"/f\xff.rs").into(),
                line: None,
                col: None,
            },
        ],
        new_window: true,
        wait: true,
        cwd: "/work dir".into(),
    };
    let line = sent.to_line();
    assert!(!line.contains('\n'), "one line: {line}");
    assert_eq!(Request::from_line(&line).unwrap(), sent);

    // The protocol's own field names, as another client would write them.
    let json: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(json["targets"][0]["path"], "/a b/c.rs");
    assert_eq!(json["targets"][0]["line"], 3);
    assert_eq!(json["targets"][0]["col"], 7);
    assert_eq!(json["targets"][2]["line"], serde_json::Value::Null);
    assert_eq!(json["new_window"], true);
    assert_eq!(json["wait"], true);
    assert_eq!(json["cwd"], "/work dir");

    assert!(Request::from_line("{\"targets\":[]}").is_err());
}

#[test]
fn stale_socket_with_no_listener_is_removed_and_the_process_becomes_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let socket = socket_path(dir.path());
    secure_dir(&socket).unwrap();
    // What a server that crashed leaves: the file, and nothing listening.
    drop(UnixListener::bind(&socket).unwrap());
    assert!(UnixStream::connect(&socket).is_err(), "nothing listens");

    let server = served(start(&request(vec![]), &how(dir.path()), no_detach));
    assert!(UnixStream::connect(&socket).is_ok(), "the new one listens");
    let requests = taking(server);

    // The next launch is a client of it, and exits.
    let sent = request(vec![target("/a.rs", Some(2), None)]);
    assert!(matches!(
        start(&sent, &how(dir.path()), no_detach),
        Start::Exit
    ));
    assert_eq!(requests.recv_blocking().unwrap(), sent);
}

#[test]
fn socket_dir_is_0700_and_socket_0600_and_path_under_104_bytes() {
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let dir = tempfile::tempdir().unwrap();
    let socket = socket_path(dir.path());
    let parent = socket.parent().unwrap();
    // A folder left too open is closed.
    fs::create_dir(parent).unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();

    let _server = served(start(&request(vec![]), &how(dir.path()), no_detach));
    assert_eq!(mode(parent), 0o700);
    assert_eq!(mode(&socket), 0o600);
    assert_eq!(socket.file_name().unwrap(), "ipc.sock");

    assert!(socket_path(&std::env::temp_dir()).as_os_str().len() < 104);
    // A $TMPDIR too long to hold one falls back on a short one.
    let long = Path::new("/").join("x".repeat(120));
    let fallback = socket_path(&long);
    assert!(fallback.as_os_str().len() < 104, "{}", fallback.display());
    assert!(fallback.starts_with("/tmp"));
}

#[test]
fn a_folder_that_is_a_symlink_is_refused_and_the_launch_runs_alone() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    let socket = socket_path(dir.path());
    std::os::unix::fs::symlink(&real, socket.parent().unwrap()).unwrap();

    assert!(matches!(
        start(&request(vec![]), &how(dir.path()), no_detach),
        Start::Alone
    ));
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

/// A held lock means a live server: its socket isn't taken from it, however a
/// connection to it fails.
#[test]
fn a_held_lock_keeps_a_server_from_being_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let _requests = live_server(dir.path());
    let socket = socket_path(dir.path());
    let inode = |path: &Path| std::os::unix::fs::MetadataExt::ino(&fs::metadata(path).unwrap());
    let before = inode(&socket);
    assert!(Server::claim(&socket).unwrap().is_none());
    assert_eq!(inode(&socket), before);
}

/// A launch by Finder or `open` may have files coming as open-document events,
/// whatever its arguments say, which a handoff would lose.
#[test]
fn launch_services_is_never_a_client_nor_detaches() {
    let launch_services = |dir: &Path| How {
        launch_services: true,
        terminal: false,
        ..how(dir)
    };
    let dir = tempfile::tempdir().unwrap();
    // No server: it's the one.
    let requests = taking(served(start(
        &request(vec![]),
        &launch_services(dir.path()),
        no_detach,
    )));
    // A live server: it runs by itself, and asks nothing of the server.
    assert!(matches!(
        start(&request(vec![]), &launch_services(dir.path()), no_detach),
        Start::Alone
    ));
    // With paths in its arguments too (`open -na Tenmoku a.rs --args b.rs` is
    // both at once): it stays for the events, its arguments with it.
    let both = request(vec![target("/b.rs", None, None)]);
    assert!(matches!(
        start(&both, &launch_services(dir.path()), no_detach),
        Start::Alone
    ));
    // Serving, if there's no one else to.
    let empty = tempfile::tempdir().unwrap();
    let _server = served(start(&both, &launch_services(empty.path()), no_detach));
    // Nothing reached the server, which still takes the next.
    let next = request(vec![target("/c.rs", None, None)]);
    assert!(matches!(
        start(&next, &how(dir.path()), no_detach),
        Start::Exit
    ));
    assert_eq!(requests.recv_blocking().unwrap(), next);
}

#[test]
fn a_terminal_launch_detaches_and_exits_unless_a_server_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    let terminal = How {
        terminal: true,
        ..how(dir.path())
    };
    let detached = Cell::new(0);
    let detach = || {
        detached.set(detached.get() + 1);
        Ok(())
    };
    assert!(matches!(
        start(&request(vec![]), &terminal, detach),
        Start::Exit
    ));
    assert_eq!(detached.get(), 1);

    // The re-executed process serves, whatever its stdio.
    let foreground = How {
        foreground: true,
        ..terminal.clone()
    };
    let requests = taking(served(start(&request(vec![]), &foreground, no_detach)));

    // And a terminal launch with a live server only hands over.
    assert!(matches!(
        start(&request(vec![]), &terminal, no_detach),
        Start::Exit
    ));
    assert_eq!(requests.recv_blocking().unwrap(), request(vec![]));
}

#[test]
fn a_failed_detach_stays_attached_and_serves() {
    let dir = tempfile::tempdir().unwrap();
    let terminal = How {
        terminal: true,
        ..how(dir.path())
    };
    let _server = served(start(&request(vec![]), &terminal, || {
        Err(io::ErrorKind::PermissionDenied.into())
    }));
}

/// With `--wait` the parent stays as the client of the server it detached.
#[test]
fn a_wait_launch_stays_as_the_client_of_the_server_it_starts() {
    let dir = tempfile::tempdir().unwrap();
    let sent = Request {
        wait: true,
        ..request(vec![target("/a.rs", None, None)])
    };
    let (started_tx, started) = async_channel::bounded(1);
    let child = {
        let dir = dir.path().to_owned();
        move || {
            thread::spawn(move || {
                let live = live_server(&dir);
                started_tx.send_blocking(live).unwrap();
            });
            Ok(())
        }
    };
    assert!(matches!(start(&sent, &how(dir.path()), child), Start::Exit));
    assert_eq!(
        started.recv_blocking().unwrap().recv_blocking().unwrap(),
        sent
    );
}

/// A server that won't take a request loses nothing: the launch runs by itself.
#[test]
fn a_refusing_server_leaves_the_launch_alone() {
    let dir = tempfile::tempdir().unwrap();
    let socket = socket_path(dir.path());
    secure_dir(&socket).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_line(&mut stream, Instant::now() + REQUEST_WAIT).unwrap();
        write_line(&mut stream, r#"{"ok":false}"#).unwrap();
    });
    assert!(matches!(
        start(&request(vec![]), &how(dir.path()), no_detach),
        Start::Alone
    ));
}

/// Garbage on the socket is answered and doesn't stop the server.
#[test]
fn a_bad_request_is_refused_and_the_next_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    let requests = live_server(dir.path());
    let mut stream = UnixStream::connect(socket_path(dir.path())).unwrap();
    write_line(&mut stream, "not json").unwrap();
    let reply = read_line(&mut stream, Instant::now() + REPLY_WAIT).unwrap();
    assert_eq!(reply, r#"{"ok":false}"#);

    let sent = request(vec![target("/a.rs", None, None)]);
    assert!(matches!(
        start(&sent, &how(dir.path()), no_detach),
        Start::Exit
    ));
    assert_eq!(requests.recv_blocking().unwrap(), sent);
}

#[test]
fn the_detached_command_is_this_launch_again_in_its_own_group() {
    let args = ["--new-window", "a.rs:3"].map(OsString::from);
    let command = detached_command(Path::new("/bin/tenmoku"), args.clone().into_iter(), false);
    assert_eq!(command.get_program(), "/bin/tenmoku");
    assert_eq!(command.get_args().collect::<Vec<_>>(), args);
    let envs: Vec<_> = command.get_envs().collect();
    assert_eq!(envs, [(OsStr::new(FOREGROUND), Some(OsStr::new("1")))]);

    // The server of a launch that stays as its client is told so.
    let command = detached_command(Path::new("/bin/tenmoku"), args.into_iter(), true);
    let envs: Vec<_> = command.get_envs().collect();
    assert!(envs.contains(&(OsStr::new(PARENT_CARRIES), Some(OsStr::new("1")))));
    assert!(envs.contains(&(OsStr::new(FOREGROUND), Some(OsStr::new("1")))));
}

/// `TENMOKU_FOREGROUND=1 tenmoku --wait file </dev/null >log 2>&1` isn't a
/// detached server, and looks like one: only the marker says who's that.
#[test]
fn a_foreground_wait_with_redirected_stdio_hands_over_or_serves() {
    let foreground = |dir: &Path| How {
        foreground: true,
        terminal: false,
        ..how(dir)
    };
    let sent = Request {
        wait: true,
        ..request(vec![target("/a.rs", None, None)])
    };

    // A live server takes it.
    let live = tempfile::tempdir().unwrap();
    let requests = live_server(live.path());
    assert!(matches!(
        start(&sent, &foreground(live.path()), no_detach),
        Start::Exit
    ));
    assert_eq!(requests.recv_blocking().unwrap(), sent);

    // With none, it serves, and isn't marked to wait for a parent's request.
    let absent = tempfile::tempdir().unwrap();
    let how = foreground(absent.path());
    let _server = served(start(&sent, &how, no_detach));
    assert!(!how.parent_carries);
    assert!(how.waits_by_itself(sent.wait), "no one else is there to");
}

#[test]
fn only_a_launch_with_no_one_else_to_wait_waits_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    let plain = how(dir.path());
    assert!(plain.waits_by_itself(true));
    assert!(!plain.waits_by_itself(false), "it didn't ask to");
    let detached_server = How {
        foreground: true,
        parent_carries: true,
        ..how(dir.path())
    };
    assert!(!detached_server.waits_by_itself(true), "its parent does");
    let launch_services = How {
        launch_services: true,
        ..how(dir.path())
    };
    assert!(!launch_services.waits_by_itself(true), "nothing's attached");
}

#[test]
fn only_a_value_of_1_sets_the_environment_flags() {
    let how = How::from_env(|name| (name == FOREGROUND).then(|| "1".into()));
    assert!(how.foreground && !how.no_ipc);
    let how = How::from_env(|name| (name == NO_IPC).then(|| "0".into()));
    assert!(!how.no_ipc);
    let how = How::from_env(|name| (name == PARENT_CARRIES).then(|| "1".into()));
    assert!(how.parent_carries && !how.foreground);
}

/// `--wait`'s detached server launches what its client asks for, and answers.
#[test]
fn the_first_request_is_the_launch_of_a_wait_server() {
    let dir = tempfile::tempdir().unwrap();
    let server = served(start(&request(vec![]), &how(dir.path()), no_detach));
    let handoffs = server.listen();
    let sent = Request {
        new_window: true,
        wait: true,
        ..request(vec![target("/a.rs", Some(3), None)])
    };
    let client = {
        let (sent, how) = (sent.clone(), how(dir.path()));
        thread::spawn(move || matches!(start(&sent, &how, no_detach), Start::Exit))
    };
    let mut cli = Cli {
        perf: false,
        new_window: false,
        wait: true,
        targets: vec![],
    };
    let waiter = launch_first_request(&handoffs, &mut cli).expect("it asked to wait");
    assert_eq!(cli.targets, sent.targets);
    assert!(cli.new_window);
    assert!(!client.is_finished(), "it's waiting for the files to close");
    waiter.closed();
    assert!(client.join().unwrap(), "told, and the client exits");
}

/// Another launch got to the new server first: it's only answered, and the
/// parent's wait comes later, like any other request.
#[test]
fn a_first_request_with_no_wait_leaves_no_one_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let handoffs = served(start(&request(vec![]), &how(dir.path()), no_detach)).listen();
    let sent = request(vec![target("/a.rs", None, None)]);
    let client = {
        let (sent, how) = (sent.clone(), how(dir.path()));
        thread::spawn(move || matches!(start(&sent, &how, no_detach), Start::Exit))
    };
    let mut cli = Cli {
        perf: false,
        new_window: false,
        wait: true,
        targets: vec![],
    };
    assert!(launch_first_request(&handoffs, &mut cli).is_none());
    assert_eq!(cli.targets, sent.targets);
    assert!(client.join().unwrap());
}

/// `{ok}` and `{closed}` sent together arrive together, and neither is lost.
#[test]
fn a_wait_answered_at_once_is_read_in_one_go() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    write_line(&mut server, r#"{"ok":true}"#).unwrap();
    write_line(&mut server, r#"{"closed":true}"#).unwrap();
    let mut pending = Vec::new();
    let first = next_line(&mut client, &mut pending, Some(Instant::now() + REPLY_WAIT)).unwrap();
    assert_eq!(first, r#"{"ok":true}"#);
    assert!(matches!(
        wait_for_close(&mut client, &mut pending),
        Handed::Taken
    ));
    assert!(pending.is_empty());
}

/// The server's taken the request, and goes: nothing else can tell the client
/// the files closed, and no other server knows of them.
#[test]
fn a_server_gone_mid_wait_fails_the_launch() {
    let dir = tempfile::tempdir().unwrap();
    let handoffs = served(start(&request(vec![]), &how(dir.path()), no_detach)).listen();
    let sent = Request {
        wait: true,
        ..request(vec![target("/a.rs", None, None)])
    };
    let client = {
        let how = how(dir.path());
        thread::spawn(move || matches!(start(&sent, &how, no_detach), Start::ServerLost))
    };
    drop(handoffs.recv_blocking().unwrap().hold());
    assert!(client.join().unwrap());
}

/// A `--wait` launch's detached server that finds another server has nothing
/// to hand over: its parent is on its way with the same request, and two would
/// open everything twice.
#[test]
fn a_wait_server_that_loses_the_race_exits_without_handing_over() {
    let dir = tempfile::tempdir().unwrap();
    let requests = live_server(dir.path());
    let detached_server = How {
        foreground: true,
        parent_carries: true,
        ..how(dir.path())
    };
    let sent = Request {
        wait: true,
        new_window: true,
        ..request(vec![target("/a.rs", None, None)])
    };
    assert!(matches!(
        start(&sent, &detached_server, no_detach),
        Start::Exit
    ));
    // The next request through is the only one.
    let next = request(vec![target("/b.rs", None, None)]);
    assert!(matches!(
        start(&next, &how(dir.path()), no_detach),
        Start::Exit
    ));
    assert_eq!(requests.recv_blocking().unwrap(), next);
}

/// A connection that closes with no reply is a server gone: this launch serves.
#[test]
fn a_server_dying_mid_request_leaves_the_launch_to_serve() {
    let dir = tempfile::tempdir().unwrap();
    let socket = socket_path(dir.path());
    secure_dir(&socket).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_line(&mut stream, Instant::now() + REQUEST_WAIT).unwrap();
    });
    let _server = served(start(&request(vec![]), &how(dir.path()), no_detach));
}

#[test]
fn launchd_as_parent_with_no_terminal_is_launch_services() {
    assert!(by_launch_services(false, false, 1));
    assert!(!by_launch_services(true, false, 1), "a terminal's");
    assert!(!by_launch_services(false, true, 1), "the detached server");
    assert!(!by_launch_services(false, false, 4242), "a script's");
}

/// A temp cleaner takes the files from under a server that runs for days. The
/// server puts them back, and launches still reach it, not a second server.
#[test]
fn a_server_restores_its_files_when_a_cleaner_takes_them() {
    let dir = tempfile::tempdir().unwrap();
    let socket = socket_path(dir.path());
    let server = served(start(&request(vec![]), &how(dir.path()), no_detach));
    let (mut keeper, handoffs) = server.serve();
    let reaches_it = |sent: Request| {
        let how = how(dir.path());
        let client = {
            let sent = sent.clone();
            thread::spawn(move || matches!(start(&sent, &how, no_detach), Start::Exit))
        };
        let handoff = handoffs.recv_blocking().unwrap();
        assert_eq!(handoff.request, sent);
        handoff.answer();
        assert!(client.join().unwrap());
    };
    assert!(keeper.keep(), "nothing's missing");
    let bound = keeper.bound;
    assert!(keeper.keep());
    assert_eq!(
        keeper.bound, bound,
        "nothing's rebound with nothing missing"
    );

    // The socket alone: the lock is still this server's.
    fs::remove_file(&socket).unwrap();
    assert!(keeper.keep());
    reaches_it(request(vec![target("/a.rs", None, None)]));

    // The lot.
    fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    assert!(keeper.keep());
    reaches_it(request(vec![target("/b.rs", None, None)]));
    assert!(
        Server::claim(&socket).unwrap().is_none(),
        "the lock is held"
    );

    // Another server got in first: this one lets it be.
    fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    secure_dir(&socket).unwrap();
    let other = Server::claim(&socket).unwrap().unwrap();
    assert!(!keeper.keep());
    drop(other);
}

/// The socket goes just after the server's look at it, with the lock still
/// held. The launch that finds it gone keeps trying, and reaches the same
/// server once its next look, on its schedule, has put the socket back.
#[test]
fn a_launch_outlasts_a_lost_socket_until_the_scheduled_repair() {
    let dir = tempfile::tempdir().unwrap();
    let requests = taking(served(start(&request(vec![]), &how(dir.path()), no_detach)));
    fs::remove_file(socket_path(dir.path())).unwrap();

    let sent = request(vec![target("/a.rs", None, None)]);
    assert!(matches!(
        start(&sent, &how(dir.path()), no_detach),
        Start::Exit
    ));
    assert_eq!(requests.recv_blocking().unwrap(), sent);
}

/// A quick server has answered and closed before its client looks, and the
/// answer is still there: on macOS, setting the timeout then fails.
#[test]
fn a_reply_from_a_server_that_already_closed_is_read() {
    let (mut server, mut client) = UnixStream::pair().unwrap();
    write_line(&mut server, r#"{"ok":true}"#).unwrap();
    drop(server);
    let reply = read_line(&mut client, Instant::now() + REPLY_WAIT).unwrap();
    assert_eq!(reply, r#"{"ok":true}"#);
    let closed = read_line(&mut client, Instant::now() + REPLY_WAIT).unwrap_err();
    assert_eq!(closed.kind(), io::ErrorKind::UnexpectedEof);
    let gone = write_line(&mut client, "late").unwrap_err();
    assert_eq!(gone.kind(), io::ErrorKind::BrokenPipe);
}
