//! Per-channel credit and the send queue (D § 2.4): each channel's window,
//! returned a quarter at a time as the consumer takes data; cancel bounded
//! by the window; urgent frames first, then the channels round robin.

use fe_remote_proto::testing::{self, RawPeer};
use fe_remote_proto::window::{INITIAL_WINDOW, MAX_DATA, MIN_WINDOW};
use fe_remote_proto::{
    CHUNK, Config, Credit, Event, FsChange, FsData, FsOp, HandshakeError, Header, Hello, Kind,
    Link, Lost, Mux, MuxError, PROTOCOL, PathBytes, Refused, Status, Stream, WatchOp, WriteExpect,
    body, chan,
};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// The wire size of one `FsData::Bytes` chunk's body.
fn chunk_body() -> usize {
    postcard::to_allocvec(&FsData::Bytes(vec![0; CHUNK]))
        .unwrap()
        .len()
}

fn quiet() -> Config {
    Config {
        heartbeat: None,
        ..Config::default()
    }
}

fn path(p: &[u8]) -> PathBytes {
    PathBytes(p.to_vec())
}

/// What the test server saw.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    /// A flood ran out of credit: the channel's window is all in flight.
    Full(u16),
    /// A request arrived (and was answered).
    Request(Kind, u16),
}

/// A server mux whose requests are answered on threads of their own:
/// `Read` of `endless` floods chunks, reporting when the window is used
/// up; any other `Read` replies its path; a `Watch` replies one `Rescan`;
/// a `WriteAtomic` is only reported, and its uploads are left unread.
fn server(link: Link, config: Config) -> (Arc<Mux>, Receiver<Seen>) {
    let (tx, rx) = mpsc::channel();
    let (mux, _) = Mux::server(link, config, testing::accept_any).unwrap();
    let mux = Arc::new(mux);
    let accepting = mux.clone();
    thread::spawn(move || {
        while let Some(inc) = accepting.accept() {
            let tx = tx.clone();
            thread::spawn(move || answer(inc, &tx));
        }
    });
    (mux, rx)
}

fn answer(inc: fe_remote_proto::Incoming, seen: &Sender<Seen>) {
    let reply = &inc.stream;
    if inc.header.chan == chan::WATCH {
        let _ = reply.send(&vec![FsChange::Rescan]);
        let _ = reply.end(Status::Ok);
        let _ = seen.send(Seen::Request(Kind::Request, chan::WATCH));
        return;
    }
    match inc.op::<FsOp>().unwrap() {
        FsOp::Read { path } if path.0 == b"endless" => {
            let chunk = FsData::Bytes(vec![0; CHUNK]);
            loop {
                match reply.try_send(&chunk) {
                    Ok(()) => continue,
                    Err(MuxError::Full(chan)) => {
                        let _ = seen.send(Seen::Full(chan));
                        break;
                    }
                    Err(_) => return,
                }
            }
            while reply.send(&chunk).is_ok() {}
            assert!(reply.cancelled());
            let _ = reply.end(Status::Cancelled);
        }
        FsOp::Read { path } => {
            let _ = reply.send(&FsData::Bytes(path.0));
            let _ = reply.end(Status::Ok);
        }
        FsOp::WriteAtomic { .. } => {
            let _ = seen.send(Seen::Request(Kind::Request, chan::FS));
            // Hold the request open, its uploads unread, until the test
            // ends.
            loop {
                thread::park();
            }
        }
        _ => panic!("not a test op"),
    }
}

fn pair() -> (Mux, Arc<Mux>, Receiver<Seen>) {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || server(theirs, quiet()));
    let (client, _) = Mux::client(ours, quiet(), testing::location(), None).unwrap();
    let (server, seen) = server.join().unwrap();
    (client, server, seen)
}

fn read(client: &Mux, p: &[u8]) -> Vec<u8> {
    let mut s = client
        .request(chan::FS, &FsOp::Read { path: path(p) })
        .unwrap();
    let mut got = Vec::new();
    while let Some(ev) = s.recv() {
        match ev {
            Event::Data(b) => match s.decode::<FsData>(&b).unwrap() {
                FsData::Bytes(b) => got.extend(b),
                other => panic!("{other:?}"),
            },
            Event::End(st) => assert_eq!(st, Status::Ok),
        }
    }
    got
}

/// An echo over stream channel `chan`, the server's end on a thread.
fn echo(client: &Mux, server: &Arc<Mux>, chan: u16, bytes: &[u8]) -> Vec<u8> {
    let server = server.clone();
    let far = thread::spawn(move || {
        let mut s = server.stream(chan).unwrap();
        while let Some(Event::Data(b)) = s.recv() {
            s.send_bytes(&b).unwrap();
        }
        s.end(Status::Ok).unwrap();
    });
    let mut s = client.stream(chan).unwrap();
    s.send_bytes(bytes).unwrap();
    let Some(Event::Data(got)) = s.recv() else {
        panic!("no echo");
    };
    s.end(Status::Ok).unwrap();
    assert_eq!(s.recv(), Some(Event::End(Status::Ok)));
    far.join().unwrap();
    got
}

fn wait(seen: &Receiver<Seen>) -> Seen {
    seen.recv_timeout(Duration::from_secs(30))
        .expect("the server stalled")
}

#[test]
fn a_stalled_channel_does_not_block_another_and_cancel_stops_within_its_window() {
    let (client, server, seen) = pair();
    // Channel 1 streams forever and nobody reads it, so it runs out of
    // credit.
    let mut bulk = client
        .request(
            chan::FS,
            &FsOp::Read {
                path: path(b"endless"),
            },
        )
        .unwrap();
    assert_eq!(wait(&seen), Seen::Full(chan::FS));
    // Channel 64 still answers.
    assert_eq!(echo(&client, &server, 64, b"hi"), b"hi");
    // Everything channel 1 got, before and after the cancel, fits its
    // window, and the window was full.
    bulk.cancel();
    let (mut bytes, mut end) = (0, None);
    while let Some(ev) = bulk.recv() {
        match ev {
            Event::Data(b) => bytes += b.len(),
            Event::End(st) => end = Some(st),
        }
    }
    assert_eq!(end, Some(Status::Cancelled));
    let window = INITIAL_WINDOW as usize;
    assert!(bytes <= window, "{bytes} bytes past a {window} window");
    assert!(
        bytes > window - chunk_body(),
        "only {bytes} of {window} in flight"
    );
    // The credit came back: channel 1 works again.
    assert_eq!(read(&client, b"again"), b"again");
}

#[test]
fn the_initial_window_is_1_25_mib_each_way() {
    assert_eq!(INITIAL_WINDOW, 1280 * 1024);
    assert_eq!(Config::default().window, INITIAL_WINDOW);
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || {
        let (mux, hello) = Mux::server(theirs, quiet(), testing::accept_any).unwrap();
        assert_eq!(hello.window, INITIAL_WINDOW);
        mux
    });
    let (client, welcome) = Mux::client(ours, quiet(), testing::location(), None).unwrap();
    assert_eq!(welcome.window, INITIAL_WINDOW);
    let server = server.join().unwrap();
    for c in [chan::CONTROL, chan::FS, chan::SEARCH, 64, 300] {
        assert_eq!(client.send_credit(c), u64::from(INITIAL_WINDOW));
        assert_eq!(server.send_credit(c), u64::from(INITIAL_WINDOW));
        assert_eq!(client.window(c), INITIAL_WINDOW);
        assert_eq!(server.window(c), INITIAL_WINDOW);
    }
}

#[test]
fn an_upload_is_held_to_the_servers_window() {
    let (client, _server, seen) = pair();
    let upload = client
        .request(
            chan::FS,
            &FsOp::WriteAtomic {
                path: path(b"big"),
                len: u64::MAX,
                expect: WriteExpect::Overwrite,
            },
        )
        .unwrap();
    assert_eq!(wait(&seen), Seen::Request(Kind::Request, chan::FS));
    let chunk = FsData::Bytes(vec![1; CHUNK]);
    let mut sent = 0;
    while upload.try_send(&chunk).is_ok() {
        sent += chunk_body();
    }
    let window = INITIAL_WINDOW as usize;
    assert!(sent <= window && sent > window - chunk_body(), "{sent}");
    assert_eq!(upload.try_send(&chunk), Err(MuxError::Full(chan::FS)));
    // Another channel still uploads.
    let stream = client.stream(64).unwrap();
    stream.try_send_bytes(&[2; CHUNK]).unwrap();
}

/// A client mux, and the raw server it handshook with.
fn raw_server(window: u32) -> (Mux, RawPeer) {
    let (ours, theirs) = Link::pipe().unwrap();
    let raw = thread::spawn(move || {
        let mut raw = RawPeer::new(theirs);
        raw.welcome(fe_remote_proto::BUILD, window);
        raw
    });
    let (client, _) = Mux::client(ours, quiet(), testing::location(), None).unwrap();
    (client, raw.join().unwrap())
}

#[test]
fn credit_comes_back_a_quarter_window_at_a_time_and_at_the_end() {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let mut read = client
        .request(chan::FS, &FsOp::Read { path: path(b"f") })
        .unwrap();
    let (req, _) = raw.recv_kind(Kind::Request);
    let id = req.0.id;
    let chunk = FsData::Bytes(vec![0; CHUNK]);
    let n = chunk_body();
    let quarter = (INITIAL_WINDOW / 4) as usize;
    let per_grant = quarter.div_ceil(n);
    for _ in 0..per_grant + 1 {
        raw.send(Kind::Data, chan::FS, id, &chunk);
    }
    // Taking less than a quarter window returns nothing yet.
    for _ in 0..per_grant - 1 {
        assert!(matches!(read.recv(), Some(Event::Data(_))));
    }
    let before = raw.barrier(1);
    assert!(before.iter().all(|h| h.kind != Kind::Credit), "{before:?}");
    // The chunk that makes it a quarter returns all of it.
    assert!(matches!(read.recv(), Some(Event::Data(_))));
    let ((h, b), _) = raw.recv_kind(Kind::Credit);
    assert_eq!(h.chan, chan::FS);
    let grant: Credit = body(&h, &b).unwrap();
    assert_eq!(grant.bytes as usize, per_grant * n);
    assert!(grant.bytes as usize >= quarter);
    // The rest comes back with the request's end, short of a quarter.
    assert!(matches!(read.recv(), Some(Event::Data(_))));
    raw.send(Kind::End, chan::FS, id, &Status::Ok);
    assert_eq!(read.recv(), Some(Event::End(Status::Ok)));
    let ((h, b), _) = raw.recv_kind(Kind::Credit);
    assert_eq!(body::<Credit>(&h, &b).unwrap().bytes as usize, n);
}

#[test]
fn data_past_the_credit_granted_loses_the_link() {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let mut read = client
        .request(chan::FS, &FsOp::Read { path: path(b"f") })
        .unwrap();
    let (req, _) = raw.recv_kind(Kind::Request);
    let chunk = FsData::Bytes(vec![0; CHUNK]);
    let fits = INITIAL_WINDOW as usize / chunk_body();
    for _ in 0..=fits {
        raw.send(Kind::Data, chan::FS, req.0.id, &chunk);
    }
    assert!(matches!(client.wait_lost(), Lost::Protocol(why) if why.contains("past the credit")));
    let mut got = 0;
    while let Some(Event::Data(_)) = read.recv() {
        got += 1;
    }
    assert_eq!(got, fits);
}

#[test]
fn a_hello_with_a_window_under_256_kib_is_refused() {
    assert_eq!(MIN_WINDOW, 256 * 1024);
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || {
        Mux::server(theirs, quiet(), testing::accept_any)
            .err()
            .unwrap()
    });
    let mut raw = RawPeer::new(ours);
    let hello = Hello {
        protocol: PROTOCOL,
        build: fe_remote_proto::BUILD.into(),
        window: MIN_WINDOW - 1,
        resume: None,
        location: testing::location(),
    };
    raw.send(Kind::Hello, chan::CONTROL, 0, &hello);
    let (h, b) = raw.recv().unwrap();
    assert_eq!(h.kind, Kind::Refused);
    let why = body::<Refused>(&h, &b).unwrap().reason;
    assert!(why.contains(&format!("{}", MIN_WINDOW - 1)), "{why}");
    assert!(matches!(server.join().unwrap(), HandshakeError::Refused(_)));
}

#[test]
fn a_welcome_with_a_window_under_256_kib_is_refused() {
    let (ours, theirs) = Link::pipe().unwrap();
    thread::spawn(move || RawPeer::new(theirs).welcome(fe_remote_proto::BUILD, MIN_WINDOW - 1));
    let err = Mux::client(ours, quiet(), testing::location(), None)
        .err()
        .unwrap();
    assert!(
        matches!(err, HandshakeError::Window(w) if w == MIN_WINDOW - 1),
        "{err}"
    );
}

#[test]
fn a_window_under_256_kib_is_not_offered() {
    let (ours, _theirs) = Link::pipe().unwrap();
    let config = Config {
        window: MIN_WINDOW - 1,
        ..quiet()
    };
    let err = Mux::client(ours, config, testing::location(), None)
        .err()
        .unwrap();
    assert!(matches!(err, HandshakeError::Window(_)), "{err}");
}

#[test]
fn a_window_of_256_kib_holds_a_channel_to_it() {
    let (ours, theirs) = Link::pipe().unwrap();
    let config = Config {
        window: MIN_WINDOW,
        ..quiet()
    };
    let server = thread::spawn(move || server(theirs, quiet()));
    let (client, welcome) = Mux::client(ours, config, testing::location(), None).unwrap();
    let (server, seen) = server.join().unwrap();
    assert_eq!(welcome.window, INITIAL_WINDOW);
    assert_eq!(server.send_credit(chan::FS), u64::from(MIN_WINDOW));
    let _bulk = client
        .request(
            chan::FS,
            &FsOp::Read {
                path: path(b"endless"),
            },
        )
        .unwrap();
    assert_eq!(wait(&seen), Seen::Full(chan::FS));
    assert!(server.send_credit(chan::FS) < chunk_body() as u64);
}

#[test]
fn a_data_body_over_a_chunk_and_its_framing_is_refused() {
    let (client, _server, _seen) = pair();
    let s = client.stream(64).unwrap();
    assert_eq!(
        s.send_bytes(&vec![0; MAX_DATA + 1]),
        Err(MuxError::TooLong(MAX_DATA + 1))
    );
    s.send_bytes(&vec![0; MAX_DATA]).unwrap();
}

/// Starts an endless read on channel 1 from a raw client and waits for
/// the server to use up the window. Nothing has been read past the
/// `Welcome`, so the server's writer is stuck inside its first chunk, the
/// pipe being smaller than one.
fn flooded_raw_client() -> (Arc<Mux>, RawPeer, Receiver<Seen>) {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || server(theirs, quiet()));
    let mut raw = RawPeer::new(ours);
    raw.hello(fe_remote_proto::BUILD, INITIAL_WINDOW);
    let (server, seen) = server.join().unwrap();
    raw.send(
        Kind::Request,
        chan::FS,
        1,
        &FsOp::Read {
            path: path(b"endless"),
        },
    );
    assert_eq!(wait(&seen), Seen::Full(chan::FS));
    (server, raw, seen)
}

/// A watch, on channel 2: above the bulk stream's channel 1, so a
/// scheduler that favoured low channels would starve it.
fn watch(raw: &mut RawPeer, id: u32) {
    let op = WatchOp::Watch { root: path(b"/") };
    raw.send(Kind::Request, chan::WATCH, id, &op);
}

fn bulk(frames: &[Header]) -> usize {
    frames
        .iter()
        .filter(|h| h.kind == Kind::Data && h.chan == chan::FS)
        .count()
}

#[test]
fn a_pong_is_not_queued_behind_bulk() {
    let (_server, mut raw, seen) = flooded_raw_client();
    raw.send(Kind::Ping, chan::CONTROL, 7, &());
    // The server handles frames in order: once it has the request sent
    // after the ping, the pong is queued.
    watch(&mut raw, 2);
    assert_eq!(wait(&seen), Seen::Request(Kind::Request, chan::WATCH));
    let ((pong, _), before) = raw.recv_kind(Kind::Pong);
    assert_eq!(pong.id, 7);
    // At most the chunk the writer was in the middle of.
    assert!(bulk(&before) <= 1, "{before:?}");
}

#[test]
fn a_reply_beside_bulk_waits_at_most_one_chunk_per_frame() {
    let (_server, mut raw, seen) = flooded_raw_client();
    watch(&mut raw, 2);
    assert_eq!(wait(&seen), Seen::Request(Kind::Request, chan::WATCH));
    let mut frames = Vec::new();
    loop {
        let (h, _) = raw.recv().unwrap();
        frames.push(h);
        if h.kind == Kind::End && h.chan == chan::WATCH {
            break;
        }
    }
    // The reply's two frames, `Data` and `End`, each wait at most one
    // chunk: the one being written, then one turn of the round robin.
    let data = frames
        .iter()
        .position(|h| h.kind == Kind::Data && h.chan == chan::WATCH)
        .unwrap();
    assert!(bulk(&frames[..data]) <= 1, "{frames:?}");
    assert!(bulk(&frames[data..]) <= 1, "{frames:?}");
}

#[test]
fn a_stream_can_be_driven_by_polling() {
    let (client, server, _seen) = pair();
    let far = thread::spawn(move || {
        futures::executor::block_on(async {
            let mut s = server.stream(70).unwrap();
            while let Some(Event::Data(b)) = s.recv_async().await {
                s.send_bytes_async(&b).await.unwrap();
            }
            s.end(Status::Ok).unwrap();
        });
    });
    futures::executor::block_on(async {
        let mut s = client.stream(70).unwrap();
        // More than a window each way, so both sides wait on credit.
        let chunk = vec![9; CHUNK];
        let total = 2 * INITIAL_WINDOW as usize;
        let (mut sent, mut got) = (0, 0);
        while got < total {
            if sent < total && s.try_send_bytes(&chunk).is_ok() {
                sent += CHUNK;
                continue;
            }
            match s.recv_async().await {
                Some(Event::Data(b)) => got += b.len(),
                other => panic!("{other:?}"),
            }
        }
        s.end(Status::Ok).unwrap();
        assert_eq!(s.recv_async().await, Some(Event::End(Status::Ok)));
    });
    far.join().unwrap();
}

#[test]
fn requests_are_accepted_by_polling() {
    let (ours, theirs) = Link::pipe().unwrap();
    let server = thread::spawn(move || {
        let (mux, _) = Mux::server(theirs, quiet(), testing::accept_any).unwrap();
        futures::executor::block_on(async {
            let inc = mux.accept_async().await.unwrap();
            let FsOp::Read { path } = inc.op().unwrap() else {
                panic!();
            };
            inc.stream.send_async(&FsData::Bytes(path.0)).await.unwrap();
            inc.stream.end(Status::Ok).unwrap();
            mux.lost_async().await
        })
    });
    let (client, _) = Mux::client(ours, quiet(), testing::location(), None).unwrap();
    assert_eq!(read(&client, b"polled"), b"polled");
    drop(client);
    assert_eq!(server.join().unwrap(), Lost::Eof);
}

/// An upload on channel 1 that has queued a window's worth to a raw server
/// that doesn't read, so the client's writer is stuck inside its first
/// chunk and the rest of channel 1's queue waits behind it.
fn stuck_upload(client: &Mux, raw: &mut RawPeer) -> Stream {
    let upload = client
        .request(
            chan::FS,
            &FsOp::WriteAtomic {
                path: path(b"big"),
                len: u64::MAX,
                expect: WriteExpect::Overwrite,
            },
        )
        .unwrap();
    raw.recv_kind(Kind::Request);
    let chunk = FsData::Bytes(vec![1; CHUNK]);
    while upload.try_send(&chunk).is_ok() {}
    upload
}

/// Reads until the request `id` arrives and returns the frames before it.
fn frames_before_request(raw: &mut RawPeer, id: u32) -> Vec<Header> {
    let mut before = Vec::new();
    loop {
        let (h, _) = raw.recv().unwrap();
        if h.kind == Kind::Request && h.id == id {
            return before;
        }
        before.push(h);
    }
}

/// Cancels (or drops) a read still queued behind a stuck upload: it must
/// end here, and the server must never see it.
fn cancel_before_sending(drop_it: bool) {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let upload = stuck_upload(&client, &mut raw);
    let mut read = client
        .request(chan::FS, &FsOp::Read { path: path(b"x") })
        .unwrap();
    let id = read.id();
    if drop_it {
        drop(read);
    } else {
        read.cancel();
        assert_eq!(read.recv(), Some(Event::End(Status::Cancelled)));
        assert_eq!(read.recv(), None);
    }
    upload.cancel();
    // A request queued after it on the same channel comes after it too.
    let after = client
        .request(
            chan::FS,
            &FsOp::Read {
                path: path(b"after"),
            },
        )
        .unwrap();
    let before = frames_before_request(&mut raw, after.id());
    assert!(before.iter().all(|h| h.id != id), "{before:?}");
}

#[test]
fn a_request_cancelled_before_it_is_sent_never_reaches_the_peer() {
    cancel_before_sending(false);
}

#[test]
fn a_request_dropped_before_it_is_sent_never_reaches_the_peer() {
    cancel_before_sending(true);
}

#[test]
fn a_stream_cancelled_before_the_peer_claims_it_stays_cancelled() {
    let (client, server, seen) = pair();
    let ours = client.stream(64).unwrap();
    ours.cancel();
    // The cancel is urgent, so once a later request is in, so is it.
    let _watch = client
        .request(chan::WATCH, &WatchOp::Watch { root: path(b"/") })
        .unwrap();
    assert_eq!(wait(&seen), Seen::Request(Kind::Request, chan::WATCH));
    let theirs = server.stream(64).unwrap();
    assert!(theirs.cancelled());
    assert_eq!(theirs.send_bytes(b"x"), Err(MuxError::Cancelled));
}

#[test]
fn an_end_from_the_peer_wakes_a_blocked_upload() {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let upload = stuck_upload(&client, &mut raw);
    let chunk = FsData::Bytes(vec![1; CHUNK]);
    thread::scope(|s| {
        let blocked = s.spawn(|| upload.send(&chunk));
        let polled = s.spawn(|| futures::executor::block_on(upload.send_async(&chunk)));
        // No credit comes back, only the request's end.
        raw.send(Kind::End, chan::FS, upload.id(), &Status::Ok);
        assert_eq!(blocked.join().unwrap(), Err(MuxError::Ended));
        assert_eq!(polled.join().unwrap(), Err(MuxError::Ended));
    });
}

/// A stream to a raw server that never grants credit, with its window
/// spent.
fn exhausted_stream(client: &Mux) -> Stream {
    let s = client.stream(64).unwrap();
    while s.try_send_bytes(&[3; CHUNK]).is_ok() {}
    s
}

#[test]
fn ending_a_stream_wakes_its_blocked_send() {
    let (client, _raw) = raw_server(INITIAL_WINDOW);
    let s = exhausted_stream(&client);
    thread::scope(|scope| {
        let blocked = scope.spawn(|| s.send_bytes(&[3; CHUNK]));
        s.end(Status::Ok).unwrap();
        assert_eq!(blocked.join().unwrap(), Err(MuxError::Ended));
    });
}

#[test]
fn hanging_up_wakes_a_blocked_send() {
    let (client, _raw) = raw_server(INITIAL_WINDOW);
    let s = exhausted_stream(&client);
    thread::scope(|scope| {
        let blocked = scope.spawn(|| s.send_bytes(&[3; CHUNK]));
        drop(client);
        assert_eq!(
            blocked.join().unwrap(),
            Err(MuxError::LinkLost(Lost::Closed))
        );
    });
}

#[test]
fn a_stream_channel_carries_one_stream_for_the_life_of_the_link() {
    let (client, server, _seen) = pair();
    assert_eq!(echo(&client, &server, 64, b"hi"), b"hi");
    assert_eq!(client.stream(64).err(), Some(MuxError::Spent(64)));
    assert_eq!(server.stream(64).err(), Some(MuxError::Spent(64)));
    // One side keeping its finished handle doesn't change that.
    let server2 = server.clone();
    let far = thread::spawn(move || {
        let mut s = server2.stream(65).unwrap();
        assert!(matches!(s.recv(), Some(Event::Data(_))));
        s.end(Status::Ok).unwrap();
        assert_eq!(s.recv(), Some(Event::End(Status::Ok)));
        s
    });
    let mut s = client.stream(65).unwrap();
    s.send_bytes(b"once").unwrap();
    assert_eq!(s.recv(), Some(Event::End(Status::Ok)));
    s.end(Status::Ok).unwrap();
    let kept = far.join().unwrap();
    drop(s);
    assert_eq!(client.stream(65).err(), Some(MuxError::Spent(65)));
    drop(kept);
}

#[test]
fn data_on_a_stream_channel_after_its_end_loses_the_link() {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let s = client.stream(64).unwrap();
    raw.send(Kind::End, 64, 0, &Status::Ok);
    drop(s);
    // The client's End is the last of its stream.
    let ((end, _), _) = raw.recv_kind(Kind::End);
    assert_eq!(end.chan, 64);
    raw.send_raw(Kind::Data, 64, 0, b"late");
    assert!(
        matches!(client.wait_lost(), Lost::Protocol(why) if why.contains("has ended")),
        "{:?}",
        client.lost()
    );
}

#[test]
fn an_end_too_long_to_send_is_refused_and_leaves_the_stream_open() {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let s = client.stream(64).unwrap();
    let huge = Status::Error(fe_remote_proto::IoError {
        kind: fe_remote_proto::IoKind::Other,
        message: "x".repeat(fe_remote_proto::MAX_BODY),
    });
    assert!(matches!(
        s.end(huge),
        Err(MuxError::Frame(fe_remote_proto::FrameError::BodyTooLong(_)))
    ));
    s.end(Status::Ok).unwrap();
    let ((end, b), _) = raw.recv_kind(Kind::End);
    assert_eq!(
        (end.chan, body::<Status>(&end, &b).unwrap()),
        (64, Status::Ok)
    );
}

#[test]
fn an_empty_write_sends_nothing() {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let s = client.stream(64).unwrap();
    for _ in 0..1000 {
        s.send_bytes(&[]).unwrap();
    }
    s.send_bytes(b"x").unwrap();
    let (h, b) = raw.recv().unwrap();
    assert_eq!((h.kind, b.as_slice()), (Kind::Data, &b"x"[..]));
}

#[test]
fn empty_data_from_the_peer_is_dropped() {
    let (client, mut raw) = raw_server(INITIAL_WINDOW);
    let mut s = client.stream(64).unwrap();
    for _ in 0..1000 {
        raw.send_raw(Kind::Data, 64, 0, &[]);
    }
    raw.send_raw(Kind::Data, 64, 0, b"y");
    assert_eq!(s.recv(), Some(Event::Data(b"y".to_vec())));
}
