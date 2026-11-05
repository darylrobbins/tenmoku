// SPDX-License-Identifier: Apache-2.0

//! The `fetch` import's policy, over a fake backend: which hosts a plugin's `net:` grants reach,
//! on the first request and on every redirect.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::kiln::extension::net::{Method, Request, Response};
use kiln_plugin::bindings::{Error, Extension, HostState};
use kiln_plugin::limits::{Limits, MAX_READ_BYTES};
use kiln_plugin::net::{DEFAULT_TIMEOUT, MAX_REDIRECTS, NetBackend, Outgoing, Received, State};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

type Reply = Box<dyn FnMut(&Outgoing) -> Received + Send>;

/// Records what it is sent and answers with `reply`.
struct Fake {
    sent: Arc<Mutex<Vec<Outgoing>>>,
    reply: Reply,
}

impl NetBackend for Fake {
    fn send(&mut self, request: &Outgoing) -> Result<Received, Error> {
        self.sent.lock().unwrap().push(request.clone());
        Ok((self.reply)(request))
    }
}

/// A net slice with `grants` over a fake that answers with `reply`, and what the fake was sent.
fn slice(
    grants: &[&str],
    reply: impl FnMut(&Outgoing) -> Received + Send + 'static,
) -> (State, Arc<Mutex<Vec<Outgoing>>>) {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        sent: Arc::clone(&sent),
        reply: Box::new(reply),
    };
    let state = State::new(&granted(grants), &Limits::default(), Box::new(fake));
    (state, sent)
}

fn granted(capabilities: &[&str]) -> Grants {
    capabilities
        .iter()
        .map(|capability| Capability::parse(capability).unwrap())
        .collect()
}

fn get(url: &str) -> Request {
    Request {
        method: Method::Get,
        url: url.to_owned(),
        headers: Vec::new(),
        body: None,
        timeout_ms: None,
    }
}

/// A 200 from the host that was asked.
fn ok(request: &Outgoing) -> Received {
    Received {
        status: 200,
        headers: vec![("content-type".into(), "text/plain".into())],
        body: b"ok".to_vec(),
        peer: request.host.clone(),
    }
}

fn redirect(request: &Outgoing, status: u16, location: &str) -> Received {
    Received {
        status,
        headers: vec![("Location".into(), location.into())],
        body: Vec::new(),
        peer: request.host.clone(),
    }
}

fn urls(sent: &Mutex<Vec<Outgoing>>) -> Vec<String> {
    sent.lock()
        .unwrap()
        .iter()
        .map(|request| request.url.clone())
        .collect()
}

#[track_caller]
fn assert_denied(result: Result<Response, Error>) {
    assert!(matches!(result, Err(Error::Denied(_))), "{result:?}");
}

#[track_caller]
fn assert_invalid(result: Result<Response, Error>) {
    assert!(
        matches!(result, Err(Error::InvalidArgument(_))),
        "{result:?}"
    );
}

#[track_caller]
fn assert_failed(result: Result<Response, Error>) {
    assert!(matches!(result, Err(Error::Failed(_))), "{result:?}");
}

#[test]
fn a_granted_host_is_reached() {
    let (mut net, sent) = slice(&["net:api.example.com"], ok);
    let response = net.fetch(get("https://api.example.com/x")).unwrap();
    assert_eq!(
        response,
        Response {
            status: 200,
            headers: vec![("content-type".into(), "text/plain".into())],
            body: b"ok".to_vec(),
        }
    );
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(sent[0].url, "https://api.example.com/x");
    assert_eq!(sent[0].host, "api.example.com");
    assert_eq!(sent[0].max_body_bytes, MAX_READ_BYTES);
    assert!(sent[0].timeout <= DEFAULT_TIMEOUT && sent[0].timeout > DEFAULT_TIMEOUT / 2);
}

#[test]
fn an_ungranted_host_is_refused_before_any_backend_call() {
    let (mut net, sent) = slice(&["net:api.example.com"], ok);
    assert_denied(net.fetch(get("https://other.example.com/x")));
    let (mut none, none_sent) = slice(&[], ok);
    assert_denied(none.fetch(get("https://api.example.com/x")));
    assert!(sent.lock().unwrap().is_empty());
    assert!(none_sent.lock().unwrap().is_empty());
}

#[test]
fn a_plugin_without_a_grant_is_denied_through_the_world() {
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_file(&engine, "tests/fixtures/strings.wat").unwrap();
    let mut linker = Linker::new(&engine);
    kiln_plugin::bindings::add_to_linker(&mut linker, |state: &mut HostState| state).unwrap();
    let mut store = Store::new(&engine, HostState::default());
    let instance = linker.instantiate(&mut store, &component).unwrap();
    Extension::new(&mut store, &instance).unwrap();
    let (error,) = instance
        .get_typed_func::<(), (Error,)>(&mut store, "try-fetch")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    assert!(matches!(error, Error::Denied(_)), "{error:?}");
}

#[test]
fn a_redirect_to_an_ungranted_host_is_refused() {
    let (mut net, sent) = slice(&["net:api.example.com"], |request| {
        redirect(request, 302, "https://other.example.com/x")
    });
    assert_denied(net.fetch(get("https://api.example.com/x")));
    assert_eq!(urls(&sent), ["https://api.example.com/x"]);
}

#[test]
fn redirects_are_followed_to_a_limit_and_https_never_downgrades() {
    let hosts: Vec<String> = (0..=MAX_REDIRECTS + 1)
        .map(|hop| format!("net:h{hop}.example.com"))
        .collect();
    let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
    // Each hop redirects to the next host until `hops` redirects have been made.
    let chain = |hops: usize| {
        slice(&hosts, move |request| {
            let hop: usize = request.host[1..request.host.find('.').unwrap()]
                .parse()
                .unwrap();
            if hop < hops {
                redirect(request, 302, &format!("https://h{}.example.com/", hop + 1))
            } else {
                ok(request)
            }
        })
    };

    let (mut net, sent) = chain(MAX_REDIRECTS);
    assert_eq!(
        net.fetch(get("https://h0.example.com/")).unwrap().status,
        200
    );
    assert_eq!(sent.lock().unwrap().len(), MAX_REDIRECTS + 1);
    assert_eq!(
        urls(&sent).last().unwrap(),
        &format!("https://h{MAX_REDIRECTS}.example.com/")
    );

    let (mut net, sent) = chain(MAX_REDIRECTS + 1);
    assert_failed(net.fetch(get("https://h0.example.com/")));
    assert_eq!(sent.lock().unwrap().len(), MAX_REDIRECTS + 1);

    // To http, even to a granted loopback address that http may reach on its own.
    for location in ["http://127.0.0.1/", "http://api.example.com/"] {
        let (mut net, sent) = slice(&["net:api.example.com", "net:127.0.0.1"], move |request| {
            redirect(request, 301, location)
        });
        assert_denied(net.fetch(get("https://api.example.com/")));
        assert_eq!(urls(&sent), ["https://api.example.com/"]);
    }
}

#[test]
fn redirects_resolve_locations_and_drop_credentials_and_bodies() {
    let (mut net, sent) =
        slice(
            &["net:api.example.com", "net:cdn.example.com"],
            |request| match request.url.as_str() {
                "https://api.example.com/a/b" => redirect(request, 303, "c?q=1#frag"),
                "https://api.example.com/a/c?q=1" => {
                    redirect(request, 307, "https://cdn.example.com/d")
                }
                _ => ok(request),
            },
        );
    let request = Request {
        method: Method::Post,
        headers: vec![
            ("Authorization".into(), "Bearer t".into()),
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "*/*".into()),
        ],
        body: Some(b"{}".to_vec()),
        ..get("https://api.example.com/a/b")
    };
    assert_eq!(net.fetch(request).unwrap().status, 200);
    let sent = sent.lock().unwrap();
    let summary: Vec<_> = sent
        .iter()
        .map(|request| {
            let headers: Vec<&str> = request
                .headers
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();
            (
                request.method,
                request.url.as_str(),
                headers,
                request.body.is_some(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                Method::Post,
                "https://api.example.com/a/b",
                vec!["Authorization", "Content-Type", "Accept"],
                true
            ),
            // 303: a GET without the body; same origin, so the credentials stay.
            (
                Method::Get,
                "https://api.example.com/a/c?q=1",
                vec!["Authorization", "Accept"],
                false
            ),
            // Another origin: no credentials.
            (
                Method::Get,
                "https://cdn.example.com/d",
                vec!["Accept"],
                false
            ),
        ]
    );
}

#[test]
fn odd_hosts_need_a_literal_grant() {
    let grants = [
        "net:api.example.com",
        "net:127.0.0.1",
        "net:xn--bcher-kva.example",
    ];
    let cases: &[(&str, Option<(&str, &str)>)] = &[
        // Normalized to a granted host: (the URL sent, the host checked).
        (
            "https://API.Example.COM/x",
            Some(("https://api.example.com/x", "api.example.com")),
        ),
        (
            "https://api.example.com./x",
            Some(("https://api.example.com/x", "api.example.com")),
        ),
        (
            "https://api%2Eexample.com/",
            Some(("https://api.example.com/", "api.example.com")),
        ),
        (
            "https://api.example.com/x#fragment",
            Some(("https://api.example.com/x", "api.example.com")),
        ),
        (
            "https://api.example.com:8443/",
            Some(("https://api.example.com:8443/", "api.example.com")),
        ),
        // A backslash is a slash: the host is api.example.com, as a browser reads it.
        (
            "https://api.example.com\\@other.example.com/",
            Some((
                "https://api.example.com/@other.example.com/",
                "api.example.com",
            )),
        ),
        (
            "https://127.0.0.1/",
            Some(("https://127.0.0.1/", "127.0.0.1")),
        ),
        ("https://127.1/", Some(("https://127.0.0.1/", "127.0.0.1"))),
        (
            "https://0x7f.0.0.1/",
            Some(("https://127.0.0.1/", "127.0.0.1")),
        ),
        (
            "https://2130706433/",
            Some(("https://127.0.0.1/", "127.0.0.1")),
        ),
        (
            "http://127.0.0.1:8080/",
            Some(("http://127.0.0.1:8080/", "127.0.0.1")),
        ),
        (
            "https://bücher.example/",
            Some(("https://xn--bcher-kva.example/", "xn--bcher-kva.example")),
        ),
        (
            "https://XN--BCHER-KVA.example/",
            Some(("https://xn--bcher-kva.example/", "xn--bcher-kva.example")),
        ),
        // Refused: no grant names the host itself.
        ("https://example.com/", None),
        ("https://xapi.example.com/", None),
        ("https://api.example.com.evil.example/", None),
        ("https://evil.example/api.example.com", None),
        ("https://evil.example/?api.example.com", None),
        ("https://127.0.0.2/", None),
        ("https://[::1]/", None),
        ("https://[::ffff:127.0.0.1]/", None),
        ("https://bucher.example/", None),
        ("https://xn--bcher-kva.example.evil/", None),
    ];
    for &(url, expected) in cases {
        let (mut net, sent) = slice(&grants, ok);
        let result = net.fetch(get(url));
        match expected {
            Some((sent_url, host)) => {
                assert_eq!(result.map(|r| r.status), Ok(200), "{url}");
                let sent = sent.lock().unwrap();
                assert_eq!(
                    (sent[0].url.as_str(), sent[0].host.as_str()),
                    (sent_url, host)
                );
            }
            None => {
                assert!(matches!(result, Err(Error::Denied(_))), "{url}: {result:?}");
                assert!(sent.lock().unwrap().is_empty(), "{url}");
            }
        }
    }
    // A user name is refused outright, whichever side of the `@` is granted.
    for url in [
        "https://user@api.example.com/",
        "https://user:pass@api.example.com/",
        "https://api.example.com@other.example.com/",
        "https://other.example.com@api.example.com/",
    ] {
        let (mut net, sent) = slice(&grants, ok);
        assert_invalid(net.fetch(get(url)));
        assert!(sent.lock().unwrap().is_empty(), "{url}");
    }
}

#[test]
fn net_star_reaches_any_host() {
    for (url, host) in [
        ("https://api.example.com/", "api.example.com"),
        ("https://anything.example.org./", "anything.example.org"),
        ("https://10.1.2.3/", "10.1.2.3"),
        ("https://[2001:DB8::1]/", "2001:db8::1"),
        ("http://[::1]:8080/", "::1"),
        ("https://bücher.example/", "xn--bcher-kva.example"),
    ] {
        let (mut net, sent) = slice(&["net:*"], ok);
        assert_eq!(net.fetch(get(url)).map(|r| r.status), Ok(200), "{url}");
        assert_eq!(sent.lock().unwrap()[0].host, host);
    }
    // Still not a malformed URL, a user name, or a scheme other than https.
    for url in [
        "https://a..b/",
        "https://user@api.example.com/",
        "http://api.example.com/",
        "ftp://api.example.com/",
        "file:///etc/passwd",
        "api.example.com",
        "https://",
    ] {
        let (mut net, sent) = slice(&["net:*"], ok);
        assert_invalid(net.fetch(get(url)));
        assert!(sent.lock().unwrap().is_empty(), "{url}");
    }
}

#[test]
fn the_connected_peer_must_equal_the_checked_host() {
    let (mut net, _) = slice(
        &["net:api.example.com", "net:other.example.com"],
        |request| Received {
            peer: "other.example.com".into(),
            ..ok(request)
        },
    );
    assert_denied(net.fetch(get("https://api.example.com/")));

    // A redirect's next hop is held to its own host.
    let (mut net, sent) = slice(&["net:api.example.com", "net:cdn.example.com"], |request| {
        if request.host == "api.example.com" {
            redirect(request, 302, "https://cdn.example.com/")
        } else {
            Received {
                peer: "api.example.com".into(),
                ..ok(request)
            }
        }
    });
    assert_denied(net.fetch(get("https://api.example.com/")));
    assert_eq!(sent.lock().unwrap().len(), 2);
}

#[test]
fn the_response_is_capped() {
    let body = |len: usize| {
        move |request: &Outgoing| Received {
            body: vec![b'x'; len],
            ..ok(request)
        }
    };
    let (mut net, _) = slice(&["net:api.example.com"], body(MAX_READ_BYTES + 1));
    assert_failed(net.fetch(get("https://api.example.com/")));
    let (mut net, _) = slice(&["net:api.example.com"], body(MAX_READ_BYTES));
    assert_eq!(
        net.fetch(get("https://api.example.com/"))
            .unwrap()
            .body
            .len(),
        MAX_READ_BYTES
    );

    // The cap is the plugin's read limit, and the backend is told it.
    let sent = Arc::new(Mutex::new(Vec::new()));
    let fake = Fake {
        sent: Arc::clone(&sent),
        reply: Box::new(body(11)),
    };
    let limits = Limits {
        max_read_bytes: 10,
        ..Limits::default()
    };
    let mut net = State::new(&granted(&["net:api.example.com"]), &limits, Box::new(fake));
    assert_failed(net.fetch(get("https://api.example.com/")));
    assert_eq!(sent.lock().unwrap()[0].max_body_bytes, 10);
}

#[test]
fn the_timeout_covers_the_whole_fetch() {
    let (mut net, sent) = slice(&["net:api.example.com"], ok);
    let request = Request {
        timeout_ms: Some(5_000),
        ..get("https://api.example.com/")
    };
    net.fetch(request).unwrap();
    let timeout = sent.lock().unwrap()[0].timeout;
    assert!(timeout <= Duration::from_secs(5) && timeout > Duration::from_secs(4));

    let (mut net, sent) = slice(&["net:api.example.com"], ok);
    let request = Request {
        timeout_ms: Some(0),
        ..get("https://api.example.com/")
    };
    assert_failed(net.fetch(request));
    assert!(sent.lock().unwrap().is_empty());
}
