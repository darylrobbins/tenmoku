//! [`fe_net::grammar_fetch::fetch_bounded`] against loopback servers: the body
//! comes back with no hash to check, it's cut off at the caller's limit by
//! `Content-Length` and by the bytes read, and redirects go only to granted
//! hosts (T1484).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use fe_net::grammar_fetch::fetch_bounded;
use fe_net::{Client, GrammarFetchError};
use futures::executor::block_on;
use kiln_command::{Capability, Grants};

const BODY: &[u8] = b"{\"schema\": 1}\n";

/// A loopback server answering each connection it accepts with what `handle`
/// writes, on a thread that lasts as long as the test.
struct Server {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
}

impl Server {
    fn start(mut handle: impl FnMut(usize, &mut TcpStream) + Send + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&requests);
        thread::spawn(move || {
            for (index, stream) in listener.incoming().enumerate() {
                let mut stream = stream.unwrap();
                read_head(&mut stream);
                counted.fetch_add(1, Ordering::SeqCst);
                handle(index, &mut stream);
            }
        });
        Server { addr, requests }
    }

    fn answering(response: Vec<u8>) -> Server {
        Server::start(move |_, stream| {
            let _ = stream.write_all(&response);
        })
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn read_head(stream: &mut TcpStream) {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            return;
        }
        head.push(byte[0]);
    }
}

fn ok(body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn redirect(to: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .into_bytes()
}

fn loopback() -> Client {
    let grants: Grants = [Capability::parse("net:127.0.0.1").unwrap()]
        .into_iter()
        .collect();
    Client::new(&grants).with_idle_timeout(Duration::from_secs(20))
}

#[test]
fn bounded_fetch_returns_the_body() {
    let server = Server::answering(ok(BODY));
    let fetched = block_on(fetch_bounded(&loopback(), &server.url("/index.json"), 1024));
    assert_eq!(fetched.as_deref(), Ok(BODY));
    assert_eq!(server.requests(), 1);

    // Exactly the limit is fine, and so is a redirect to a granted host.
    let asset = Server::answering(ok(BODY));
    let release = Server::answering(redirect(&asset.url("/asset")));
    let limit = BODY.len() as u64;
    let fetched = block_on(fetch_bounded(&loopback(), &release.url("/x"), limit));
    assert_eq!(fetched.as_deref(), Ok(BODY));
    assert_eq!((release.requests(), asset.requests()), (1, 1));

    // Other statuses are errors.
    let server = Server::answering(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec());
    let fetched = block_on(fetch_bounded(&loopback(), &server.url("/x"), 1024));
    assert_eq!(fetched, Err(GrammarFetchError::Status(404)));
}

#[test]
fn bounded_fetch_refuses_over_limit_by_header_and_stream() {
    let too_large = Err(GrammarFetchError::TooLarge { max_bytes: 1023 });

    // A Content-Length over the limit, then nothing: no body is waited for, or
    // this would wait out the idle timeout.
    let (done, wait) = mpsc::channel::<()>();
    let server = Server::start(move |_, stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\n");
        let _ = wait.recv();
    });
    let fetched = block_on(fetch_bounded(&loopback(), &server.url("/x"), 1023));
    assert_eq!(fetched, too_large);
    drop(done);

    // No Content-Length: a body with no end, cut off at the limit; the
    // server's writes fail once the client has gone.
    let (wrote, written) = mpsc::channel();
    let server = Server::start(move |_, stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
        let chunk = format!("{:x}\r\n{}\r\n", 1 << 16, "x".repeat(1 << 16));
        for _ in 0..(1 << 16) {
            if stream.write_all(chunk.as_bytes()).is_err() {
                let _ = wrote.send(true);
                return;
            }
        }
        let _ = wrote.send(false);
    });
    let fetched = block_on(fetch_bounded(&loopback(), &server.url("/x"), 1023));
    assert_eq!(fetched, too_large);
    assert_eq!(
        written.recv_timeout(Duration::from_secs(30)),
        Ok(true),
        "the connection closes at the limit"
    );
}

#[test]
fn bounded_fetch_refuses_an_ungranted_redirect() {
    let release = Server::answering(redirect("https://storage.example/index.json"));
    let fetched = block_on(fetch_bounded(&loopback(), &release.url("/x"), 1024));
    assert_eq!(
        fetched,
        Err(GrammarFetchError::HostNotAllowed("storage.example".into()))
    );
    assert_eq!(release.requests(), 1);
}
