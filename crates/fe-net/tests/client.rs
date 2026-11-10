use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use fe_net::{Client, NetError};
use futures::StreamExt;
use futures::executor::block_on;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use kiln_command::{Capability, Grants};

/// A loopback HTTP/1.1 server for one connection, on a thread.
struct Server<T> {
    addr: SocketAddr,
    /// How many connections it accepted.
    accepts: Arc<AtomicUsize>,
    thread: JoinHandle<T>,
}

impl<T: Send + 'static> Server<T> {
    /// Accepts one connection, reads its request and hands both to `handle`.
    fn start(handle: impl FnOnce(TcpStream, String) -> T + Send + 'static) -> Server<T> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&accepts);
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            counted.fetch_add(1, Ordering::SeqCst);
            let request = read_request(&mut stream);
            handle(stream, request)
        });
        Server {
            addr,
            accepts,
            thread,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn accepts(&self) -> usize {
        self.accepts.load(Ordering::SeqCst)
    }

    fn join(self) -> T {
        self.thread.join().unwrap()
    }
}

/// The request's head and body, as text.
fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap() == 0 {
            break;
        }
        bytes.push(byte[0]);
    }
    let head = String::from_utf8(bytes).unwrap();
    let length = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|n| n.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    head + &String::from_utf8(body).unwrap()
}

fn chunk(stream: &mut TcpStream, data: &str) -> std::io::Result<()> {
    write!(stream, "{:x}\r\n{data}\r\n", data.len())?;
    stream.flush()
}

const CHUNKED_HEAD: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";

fn grants(hosts: &[&str]) -> Grants {
    hosts
        .iter()
        .map(|host| Capability::parse(&format!("net:{host}")).unwrap())
        .collect()
}

fn loopback() -> Client {
    Client::new(&grants(&["127.0.0.1"]))
}

fn get(client: &Client, url: &str) -> Result<fe_net::Response, NetError> {
    block_on(client.request(Method::GET, url, HeaderMap::new(), Vec::new()))
}

#[test]
fn a_chunked_body_streams_as_it_arrives() {
    let third_written = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&third_written);
    let server = Server::start(move |mut stream, request| {
        stream.write_all(CHUNKED_HEAD.as_bytes()).unwrap();
        chunk(&mut stream, "one ").unwrap();
        thread::sleep(Duration::from_millis(200));
        chunk(&mut stream, "two ").unwrap();
        thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::SeqCst);
        chunk(&mut stream, "three").unwrap();
        stream.write_all(b"0\r\n\r\n").unwrap();
        request
    });
    let addr = server.addr;
    let mut response = get(&loopback(), &server.url("/stream?x=1")).unwrap();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "text/event-stream");

    let first = block_on(response.body.next()).unwrap().unwrap();
    assert!(
        !third_written.load(Ordering::SeqCst),
        "the first chunk arrived before the server wrote the third"
    );
    let mut text = String::from_utf8(first).unwrap();
    while let Some(chunk) = block_on(response.body.next()) {
        text.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
    }
    assert_eq!(text, "one two three");

    let request = server.join();
    assert!(
        request.starts_with("GET /stream?x=1 HTTP/1.1\r\n"),
        "{request}"
    );
    let request = request.to_ascii_lowercase();
    assert!(request.contains(&format!("host: {addr}\r\n")), "{request}");
    assert!(request.contains("connection: close\r\n"), "{request}");
    assert!(
        !request.contains("accept-encoding"),
        "no compression asked for: {request}"
    );
}

#[test]
fn a_content_length_body_streams_and_the_request_body_is_sent() {
    let server = Server::start(|mut stream, request| {
        stream
            .write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nX-Request-Id: r1\r\n\r\nnot ",
            )
            .unwrap();
        thread::sleep(Duration::from_millis(50));
        stream.write_all(b"found").unwrap();
        request
    });
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", HeaderValue::from_static("k"));
    let response = block_on(loopback().request(
        Method::POST,
        &server.url("/v1/messages"),
        headers,
        b"{\"a\":1}".to_vec(),
    ))
    .unwrap();
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.headers["x-request-id"], "r1");
    let body: Vec<u8> = block_on(response.body.map(Result::unwrap).concat());
    assert_eq!(body, b"not found");

    let request = server.join();
    assert!(
        request.starts_with("POST /v1/messages HTTP/1.1\r\n"),
        "{request}"
    );
    let lower = request.to_ascii_lowercase();
    assert!(lower.contains("content-length: 7\r\n"), "{request}");
    assert!(lower.contains("x-api-key: k\r\n"), "{request}");
    assert!(request.ends_with("\r\n\r\n{\"a\":1}"), "{request}");
}

#[test]
fn dropping_the_body_closes_the_connection() {
    let server = Server::start(|mut stream, _| {
        stream.write_all(CHUNKED_HEAD.as_bytes()).unwrap();
        chunk(&mut stream, "first").unwrap();
        // Keeps writing, as a reply being generated does, until the client
        // has gone.
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            thread::sleep(Duration::from_millis(20));
            if chunk(&mut stream, "more").is_err() {
                return Some(Instant::now());
            }
        }
        None
    });
    let mut response = get(&loopback(), &server.url("/")).unwrap();
    assert_eq!(block_on(response.body.next()).unwrap().unwrap(), b"first");
    drop(response);
    let dropped = Instant::now();

    let closed = server.join().expect("the server saw the connection close");
    assert!(
        closed.duration_since(dropped) < Duration::from_secs(2),
        "closed {:?} after the drop",
        closed.duration_since(dropped)
    );
}

#[test]
fn dropping_the_request_before_its_response_closes_the_connection() {
    let (dropped_tx, dropped_rx) = std::sync::mpsc::channel::<Instant>();
    let server = Server::start(move |mut stream, _| {
        // Never answers; waits for the client to go.
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let read = stream.read(&mut [0; 16]);
        let closed = Instant::now();
        assert!(matches!(read, Ok(0)), "EOF, not {read:?}");
        (closed, dropped_rx.recv().unwrap())
    });
    let client = loopback();
    let url = server.url("/");
    block_on(async {
        let request = client.request(Method::GET, &url, HeaderMap::new(), Vec::new());
        let waited = futures::future::select(
            Box::pin(request),
            async_io::Timer::after(Duration::from_millis(300)),
        )
        .await;
        assert!(
            matches!(waited, futures::future::Either::Right(_)),
            "no response yet"
        );
    });
    dropped_tx.send(Instant::now()).unwrap();
    let (closed, dropped) = server.join();
    assert!(closed.saturating_duration_since(dropped) < Duration::from_secs(2));
}

#[test]
fn hosts_outside_the_grants_are_refused() {
    let server = Server::start(|_, _| ());
    let client = Client::new(&grants(&["api.anthropic.com"]));

    let refused = get(&client, "https://example.com/").unwrap_err();
    assert_eq!(refused, NetError::HostNotAllowed("example.com".to_owned()));
    // The loopback server is outside these grants too.
    let refused = get(&client, &server.url("/")).unwrap_err();
    assert_eq!(refused, NetError::HostNotAllowed("127.0.0.1".to_owned()));
    assert_eq!(server.accepts(), 0);

    assert_eq!(
        get(&client, "http://api.anthropic.com/").unwrap_err(),
        NetError::Scheme
    );
    assert_eq!(
        get(&client, "ftp://api.anthropic.com/").unwrap_err(),
        NetError::Scheme
    );
    // Hosts compare without case, and the grants are for the host alone.
    assert_eq!(
        get(&client, "https://API.anthropic.com.evil.test/").unwrap_err(),
        NetError::HostNotAllowed("api.anthropic.com.evil.test".to_owned())
    );
    assert!(matches!(
        get(&client, "https://user@api.anthropic.com/").unwrap_err(),
        NetError::Http(_)
    ));
    // `localhost` would go through DNS: plain http is for the addresses only.
    let any = Client::new(&grants(&["*"]));
    assert_eq!(
        get(&any, "http://localhost:1/").unwrap_err(),
        NetError::Scheme
    );
    assert_eq!(server.accepts(), 0);
}

#[test]
fn a_refused_connection_is_connect() {
    // Bound, then closed: nothing listens on the port.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let error = get(&loopback(), &format!("http://127.0.0.1:{port}/")).unwrap_err();
    assert!(matches!(error, NetError::Connect(_)), "{error:?}");
}

#[test]
fn a_reply_that_isnt_http_is_an_error() {
    let server = Server::start(|mut stream, _| {
        stream.write_all(b"SSH-2.0-OpenSSH_9.6\r\n").unwrap();
    });
    let error = get(&loopback(), &server.url("/")).unwrap_err();
    assert!(
        matches!(error, NetError::Http(_) | NetError::Io(_)),
        "{error:?}"
    );
    server.join();
}

#[test]
fn a_body_cut_short_is_an_error() {
    let server = Server::start(|mut stream, _| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
            .unwrap();
    });
    let response = get(&loopback(), &server.url("/")).unwrap();
    server.join();
    let chunks: Vec<_> = block_on(response.body.collect());
    assert!(matches!(chunks.last(), Some(Err(_))), "{chunks:?}");
}

#[test]
fn a_trickling_reply_keeps_the_idle_timeout_off_and_silence_trips_it() {
    // Each piece arrives well inside the timeout (a quarter of it, so a loaded
    // machine can oversleep a gap by 3x and still not trip it), but the head
    // and the one chunk's framing and data together take twice as long as it.
    const IDLE: Duration = Duration::from_millis(1000);
    const GAP: Duration = Duration::from_millis(250);
    let pieces: [&[u8]; 8] = [
        b"HTTP/1.1 200 OK\r\n",
        b"Transfer-Encoding: chunked\r\n",
        b"X-Slow: 1\r\n",
        b"\r\n",
        b"b\r\n",
        b"hello",
        b" world",
        b"\r\n",
    ];
    // How long the server stays silent after the last piece. The idle timeout
    // must fire long before it ends, so it bounds the wait however loaded the
    // machine is.
    const SILENCE: Duration = Duration::from_secs(10);
    let server = Server::start(move |mut stream, _| {
        for piece in pieces {
            thread::sleep(GAP);
            stream.write_all(piece).unwrap();
            stream.flush().unwrap();
        }
        // Then nothing, with the connection open.
        thread::sleep(SILENCE);
    });
    let client = loopback().with_idle_timeout(IDLE);
    let started = Instant::now();
    let mut response = get(&client, &server.url("/")).unwrap();
    assert_eq!(response.headers["x-slow"], "1");
    let mut text = Vec::new();
    let error = loop {
        match block_on(response.body.next()) {
            Some(Ok(chunk)) => text.extend(chunk),
            Some(Err(error)) => break error,
            None => panic!("the body ended"),
        }
    };
    assert_eq!(text, b"hello world");
    assert_eq!(error, NetError::Timeout);
    let waited = started.elapsed();
    assert!(
        waited < SILENCE / 2,
        "timed out after {waited:?}, not well inside the server's {SILENCE:?} of silence"
    );
    assert!(block_on(response.body.next()).is_none());
}

#[test]
fn a_silent_server_times_out_before_the_response() {
    let server = Server::start(|_stream, _| thread::sleep(Duration::from_secs(3)));
    let client = loopback().with_idle_timeout(Duration::from_millis(300));
    let started = Instant::now();
    assert_eq!(
        get(&client, &server.url("/")).unwrap_err(),
        NetError::Timeout
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn the_body_frames_the_request_whatever_the_headers_say() {
    let server = Server::start(|mut stream, request| {
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
            .unwrap();
        request
    });
    let mut headers = HeaderMap::new();
    headers.insert("content-length", HeaderValue::from_static("1"));
    headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
    let response =
        block_on(loopback().request(Method::POST, &server.url("/"), headers, b"hello".to_vec()))
            .unwrap();
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let request = server.join();
    let lower = request.to_ascii_lowercase();
    assert!(lower.contains("content-length: 5\r\n"), "{request}");
    assert!(!lower.contains("transfer-encoding"), "{request}");
    assert!(request.ends_with("\r\n\r\nhello"), "{request}");
}

#[test]
fn an_invalid_port_is_refused_not_defaulted() {
    let client = Client::new(&grants(&["api.anthropic.com", "127.0.0.1"]));
    for url in [
        "https://api.anthropic.com:abc/",
        "https://api.anthropic.com:65536/",
        "https://api.anthropic.com:0/",
        "https://api.anthropic.com:/",
        "http://127.0.0.1:99999/",
    ] {
        let error = get(&client, url).unwrap_err();
        assert!(matches!(error, NetError::Http(_)), "{url}: {error:?}");
    }
}

/// For a person to run: TLS to the API and the platform verifier work.
#[test]
#[ignore = "reaches api.anthropic.com"]
fn tls_to_api_anthropic_com() {
    let client = Client::new(&grants(&["api.anthropic.com"]));
    let response = get(&client, "https://api.anthropic.com/v1/models").unwrap();
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    let body: Vec<u8> = block_on(response.body.map(Result::unwrap).concat());
    assert!(String::from_utf8_lossy(&body).contains("authentication_error"));
}
