//! [`fe_net::grammar_fetch`] against loopback servers: the body comes back only
//! when its hash is the recorded one, it's cut off at the caller's limit, and
//! redirects go only to granted hosts.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use fe_net::grammar_fetch::MAX_REDIRECTS;
use fe_net::{Client, GrammarFetchError, NetError, grammar_fetch, sha256_hex};
use futures::executor::block_on;
use kiln_command::{Capability, Grants};

const BODY: &[u8] = b"\0asm a grammar's bytes";
const MAX: u64 = 1 << 20;

/// A loopback server answering each connection it accepts, in turn, with what
/// `handle` writes, on a thread that lasts as long as the test.
struct Server {
    addr: SocketAddr,
    /// How many requests it has read.
    requests: Arc<AtomicUsize>,
}

impl Server {
    fn start(handle: impl FnMut(usize, &mut TcpStream) + Send + 'static) -> Server {
        Server::on(TcpListener::bind("127.0.0.1:0").unwrap(), handle)
    }

    fn on(
        listener: TcpListener,
        mut handle: impl FnMut(usize, &mut TcpStream) + Send + 'static,
    ) -> Server {
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

    /// Answers every request with `response`.
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

fn client(hosts: &[&str]) -> Client {
    let grants: Grants = hosts
        .iter()
        .map(|host| Capability::parse(&format!("net:{host}")).unwrap())
        .collect();
    Client::new(&grants).with_idle_timeout(Duration::from_secs(20))
}

fn loopback() -> Client {
    client(&["127.0.0.1"])
}

/// A second loopback host to redirect to, if this machine has IPv6.
fn ipv6_listener() -> Option<TcpListener> {
    TcpListener::bind("[::1]:0").ok()
}

#[test]
fn good_download_is_returned() {
    let server = Server::answering(ok(BODY));
    let fetched = block_on(grammar_fetch(
        &loopback(),
        &server.url("/tree-sitter-x.wasm"),
        &sha256_hex(BODY),
        MAX,
    ));
    assert_eq!(fetched.as_deref(), Ok(BODY));
    assert_eq!(server.requests(), 1);
}

#[test]
fn wrong_hash_is_refused() {
    let server = Server::answering(ok(BODY));
    let expected = sha256_hex(b"other bytes");
    let fetched = block_on(grammar_fetch(
        &loopback(),
        &server.url("/x"),
        &expected,
        MAX,
    ));
    assert_eq!(
        fetched,
        Err(GrammarFetchError::HashMismatch {
            expected,
            actual: sha256_hex(BODY),
        })
    );
}

/// The digest of the empty string and of a known text, so that `sha256_hex` is
/// what `sha256sum` writes in the manifest.
#[test]
fn hashes_are_sha256sums() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn empty_hash_is_refused_without_a_request() {
    let server = Server::answering(ok(BODY));
    let fetched = block_on(grammar_fetch(&loopback(), &server.url("/x"), "", MAX));
    assert_eq!(fetched, Err(GrammarFetchError::NotReleased));
    assert_eq!(server.requests(), 0);
}

#[test]
fn a_hash_that_is_not_a_sha256_is_refused_without_a_request() {
    let server = Server::answering(ok(BODY));
    for hash in [
        "abc".to_owned(),
        sha256_hex(BODY).to_ascii_uppercase(),
        format!("{}g", &sha256_hex(BODY)[1..]),
    ] {
        let fetched = block_on(grammar_fetch(&loopback(), &server.url("/x"), &hash, MAX));
        assert_eq!(fetched, Err(GrammarFetchError::InvalidHash(hash)));
    }
    assert_eq!(server.requests(), 0);
}

#[test]
fn oversize_is_cut_off() {
    let body = vec![b'x'; 1024];
    let hash = sha256_hex(&body);
    let too_large = Err(GrammarFetchError::TooLarge { max_bytes: 1023 });

    // Exactly the limit is fine.
    let server = Server::answering(ok(&body));
    let fetched = block_on(grammar_fetch(&loopback(), &server.url("/x"), &hash, 1024));
    assert_eq!(fetched.as_deref(), Ok(&body[..]));

    // A Content-Length over the limit, then nothing: no body is waited for, or
    // this would wait out the idle timeout and be Net(Timeout).
    let (done, wait) = mpsc::channel::<()>();
    let server = Server::start(move |_, stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\n");
        let _ = wait.recv();
    });
    let fetched = block_on(grammar_fetch(&loopback(), &server.url("/x"), &hash, 1023));
    assert_eq!(fetched, too_large);
    drop(done);

    // No Content-Length: exactly max_bytes + 1 bytes, a chunk at a time, then
    // nothing. Stopping at that byte is the only way not to time out.
    let (done, wait) = mpsc::channel::<()>();
    let server = Server::start(move |_, stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
        for _ in 0..1024 {
            let _ = stream.write_all(b"1\r\nx\r\n");
        }
        let _ = wait.recv();
    });
    let fetched = block_on(grammar_fetch(&loopback(), &server.url("/x"), &hash, 1023));
    assert_eq!(fetched, too_large);
    drop(done);

    // A body with no end: the server's writes fail once the client has gone.
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
    let fetched = block_on(grammar_fetch(&loopback(), &server.url("/x"), &hash, 1023));
    assert_eq!(fetched, too_large);
    assert_eq!(
        written.recv_timeout(Duration::from_secs(30)),
        Ok(true),
        "the connection closes at the limit"
    );
}

#[test]
fn follows_redirects_only_to_granted_hosts() {
    // A granted host, then the asset: another port of the same granted host.
    let asset = Server::answering(ok(BODY));
    let release = Server::answering(redirect(&asset.url("/asset")));
    let fetched = block_on(grammar_fetch(
        &loopback(),
        &release.url("/x"),
        &sha256_hex(BODY),
        MAX,
    ));
    assert_eq!(fetched.as_deref(), Ok(BODY));
    assert_eq!((release.requests(), asset.requests()), (1, 1));

    // A host the grants don't name, which nothing is ever sent to.
    let release = Server::answering(redirect("https://storage.example/asset"));
    let fetched = block_on(grammar_fetch(
        &loopback(),
        &release.url("/x"),
        &sha256_hex(BODY),
        MAX,
    ));
    assert_eq!(
        fetched,
        Err(GrammarFetchError::HostNotAllowed("storage.example".into()))
    );

    if let Some(listener) = ipv6_listener() {
        let other = Server::on(listener, |_, stream| {
            let _ = stream.write_all(&ok(BODY));
        });
        let release = Server::answering(redirect(&other.url("/asset")));

        // Not granted: no connection, let alone a request.
        let fetched = block_on(grammar_fetch(
            &loopback(),
            &release.url("/x"),
            &sha256_hex(BODY),
            MAX,
        ));
        assert_eq!(
            fetched,
            Err(GrammarFetchError::HostNotAllowed("[::1]".into()))
        );
        assert_eq!(other.requests(), 0);

        // Granted (a grant can't spell an IPv6 address, only all hosts): the
        // other host's body comes back.
        let fetched = block_on(grammar_fetch(
            &client(&["*"]),
            &release.url("/x"),
            &sha256_hex(BODY),
            MAX,
        ));
        assert_eq!(fetched.as_deref(), Ok(BODY));
        assert_eq!(other.requests(), 1);
    }
}

#[test]
fn redirects_stop_after_three_hops() {
    /// A server that sends its first `hops` requests back to itself.
    fn hopping(hops: usize) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        Server::on(listener, move |index, stream| {
            let response = if index < hops {
                redirect(&format!("http://{addr}/hop"))
            } else {
                ok(BODY)
            };
            let _ = stream.write_all(&response);
        })
    }
    let fetch = |server: &Server| {
        block_on(grammar_fetch(
            &loopback(),
            &server.url("/x"),
            &sha256_hex(BODY),
            MAX,
        ))
    };

    assert_eq!(MAX_REDIRECTS, 3);
    let server = hopping(3);
    assert_eq!(fetch(&server).as_deref(), Ok(BODY));
    assert_eq!(server.requests(), 4);

    let server = hopping(4);
    assert_eq!(fetch(&server), Err(GrammarFetchError::TooManyRedirects));
    assert_eq!(server.requests(), 4, "the fourth redirect isn't followed");
}

#[test]
fn a_redirect_may_be_relative_to_the_host() {
    let server = Server::start(|index, stream| {
        let response = match index {
            0 => redirect("/asset"),
            _ => ok(BODY),
        };
        let _ = stream.write_all(&response);
    });
    let fetched = block_on(grammar_fetch(
        &loopback(),
        &server.url("/x"),
        &sha256_hex(BODY),
        MAX,
    ));
    assert_eq!(fetched.as_deref(), Ok(BODY));
    assert_eq!(server.requests(), 2);
}

#[test]
fn a_redirect_it_cannot_follow_is_refused() {
    for (response, location) in [
        (
            b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
            "",
        ),
        (redirect("asset.wasm"), "asset.wasm"),
    ] {
        let server = Server::answering(response);
        let fetched = block_on(grammar_fetch(
            &loopback(),
            &server.url("/x"),
            &sha256_hex(BODY),
            MAX,
        ));
        assert_eq!(
            fetched,
            Err(GrammarFetchError::BadRedirect(location.to_owned()))
        );
        assert_eq!(server.requests(), 1);
    }
}

#[test]
fn a_redirect_to_plain_http_elsewhere_is_refused() {
    let release = Server::answering(redirect("http://github.example/asset"));
    let fetched = block_on(grammar_fetch(
        &client(&["127.0.0.1", "github.example"]),
        &release.url("/x"),
        &sha256_hex(BODY),
        MAX,
    ));
    assert_eq!(fetched, Err(GrammarFetchError::Net(NetError::Scheme)));
}

#[test]
fn other_statuses_are_errors() {
    let server = Server::answering(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec());
    let fetched = block_on(grammar_fetch(
        &loopback(),
        &server.url("/x"),
        &sha256_hex(BODY),
        MAX,
    ));
    assert_eq!(fetched, Err(GrammarFetchError::Status(404)));
}

#[test]
fn a_refused_first_url_is_host_not_allowed() {
    let fetched = block_on(grammar_fetch(
        &loopback(),
        "https://storage.example/asset",
        &sha256_hex(BODY),
        MAX,
    ));
    assert_eq!(
        fetched,
        Err(GrammarFetchError::HostNotAllowed("storage.example".into()))
    );
}
