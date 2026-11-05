//! The fe-net backend under kiln-plugin's fetch policy, against a loopback HTTP server.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use fe_plugin::net::FeNet;
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::Error;
use kiln_plugin::bindings::kiln::extension::net::{Method, Request};
use kiln_plugin::limits::Limits;
use kiln_plugin::net::{NetBackend as _, Outgoing, State};

fn granted(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn net(grants: &[&str]) -> State {
    let grants = granted(grants);
    State::new(&grants, &Limits::default(), Box::new(FeNet::new(&grants)))
}

/// Reads one request's head, lower-cased, from `stream`.
fn read_head(stream: &TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
            return head.to_ascii_lowercase();
        }
        head.push_str(&line);
    }
}

#[test]
fn the_fe_net_backend_fetches_from_a_local_server() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        // `/x` redirects to `/y`, each on a connection of its own.
        let mut heads = Vec::new();
        for reply in [
            "HTTP/1.1 302 Found\r\nLocation: /y\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nSet-Cookie: a=b\r\nX-Kind: test\r\nContent-Length: 5\r\n\r\nhello",
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            heads.push(read_head(&stream));
            stream.write_all(reply.as_bytes()).unwrap();
        }
        heads
    });

    let request = Request {
        method: Method::Get,
        url: format!("http://127.0.0.1:{port}/x"),
        headers: vec![
            ("X-Api-Key".into(), "k".into()),
            ("Cookie".into(), "c=d".into()),
        ],
        body: None,
        timeout_ms: Some(2_000),
    };
    let response = net(&["net:127.0.0.1"]).fetch(request.clone()).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"hello");
    assert!(response.headers.contains(&("x-kind".into(), "test".into())));
    assert!(
        !response
            .headers
            .iter()
            .any(|(name, _)| name == "set-cookie")
    );
    let heads = server.join().unwrap();
    assert!(heads[0].starts_with("get /x http/1.1\r\n"), "{heads:?}");
    assert!(heads[1].starts_with("get /y http/1.1\r\n"), "{heads:?}");
    for head in &heads {
        assert!(head.contains("x-api-key: k\r\n"), "{head}");
        assert!(!head.contains("cookie"), "{head}");
    }

    // Without the grant nothing connects: kiln refuses the URL, and so does fe-net when it is
    // handed one kiln didn't check.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let request = Request {
        url: format!(
            "http://127.0.0.1:{}/x",
            listener.local_addr().unwrap().port()
        ),
        ..request
    };
    let refused = net(&["net:api.example.com"]).fetch(request.clone());
    assert!(matches!(refused, Err(Error::Denied(_))), "{refused:?}");
    let mut fe_net = FeNet::new(&granted(&["net:api.example.com"]));
    let bypassed = fe_net.send(&Outgoing {
        method: Method::Get,
        url: request.url.clone(),
        host: "127.0.0.1".into(),
        headers: Vec::new(),
        body: None,
        timeout: Duration::from_secs(2),
        max_body_bytes: 16,
    });
    assert!(matches!(bypassed, Err(Error::Denied(_))), "{bypassed:?}");
    assert!(listener.accept().is_err(), "a connection was made");
}

#[test]
fn excessive_request_headers_are_an_invalid_argument_not_a_panic() {
    let headers = (0..40_000)
        .map(|i| (format!("x-h{i}"), "v".to_owned()))
        .collect();
    let request = Request {
        method: Method::Get,
        url: "http://127.0.0.1:9/x".into(),
        headers,
        body: None,
        timeout_ms: Some(1_000),
    };
    let result = net(&["net:127.0.0.1"]).fetch(request);
    assert!(
        matches!(result, Err(Error::InvalidArgument(_))),
        "{result:?}"
    );
}
