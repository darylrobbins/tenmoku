---
task_uid: "01m57yv46c4jgnb420knkvbhpq"
task: "T193"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-net tests a_chunked_body_streams_as_it_arrives and dropping_the_body_closes_the_connection (tests/client.rs) pass against a loopback server: the first chunk arrives before the server writes the third, and the server sees the connection close within 2 s of the drop"
    required: true
    manual: false
  - id: "C2"
    text: "fe-net test hosts_outside_the_grants_are_refused (tests/client.rs): a host outside the grants is HostNotAllowed with the loopback server's accept count still 0, and http to a non-loopback host is Scheme"
    required: true
    manual: false
  - id: "C3"
    text: "fe-chat-anthropic test the_anthropic_transport_streams_a_fixture (tests/net.rs): the provider over fe_net::Client to a loopback server serving an SSE fixture yields the same ChatEvents as through the fake transport"
    required: true
    manual: false
  - id: "C4"
    text: "TLS is rustls with ring and rustls-platform-verifier; cargo tree -p fe-net -e normal --target all lists no reqwest, native-tls, openssl, webpki-roots or aws-lc-rs, and lists tokio only as hyper's dependency with the sync feature alone (no runtime, no net, no io features; hyper 1 requires it); cargo deny check passes; deny.toml's only change is one exceptions entry allowing CDLA-Permissive-2.0 for the crate webpki-root-certs (the global allow list and [graph] are unchanged); the finish notes list each new crate and its licence"
    required: true
    manual: false
  - id: "C5"
    text: "tls_to_api_anthropic_com (tests/client.rs) and live_text_reply (fe-chat-anthropic tests/net.rs) exist and are ignored; scripts/check-no-gpui.sh reports ok: fe-net"
    required: true
    manual: false
---

# fe-net: HTTPS over hyper, rustls and async-io that reaches only granted hosts, as the Anthropic provider's transport (Mac T965)

# fe-net: an HTTPS client over async-io that reaches only granted hosts, as the Anthropic provider's transport

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) needs the
repo's first network client (record §6). Today nothing can make a request: Cargo.lock has
`gpui-pre-http-client`'s `HttpClient` trait but no implementation and no TLS crate, and the
Markdown preview blocks remote images for that reason (T847). `fe-chat-anthropic` (the card
this depends on) streams through a `Transport` trait with only a test fake behind it.

## Change

New crate `crates/fe-net` ("HTTPS requests to the hosts the app grants. Must not depend on
GPUI."), registered in the workspace `Cargo.toml`, with the record §6 stack: **hyper 1**
(`client`, `http1` features; MIT) over **async-io** (already the app's) through **smol-hyper**
(MIT OR Apache-2.0), **rustls** with the **ring** provider through **futures-rustls**, and
**rustls-platform-verifier** for certificates (the OS trust store). Not reqwest, tokio,
webpki-roots, native-tls, openssl or aws-lc-rs. HTTP/1.1 only. Run `cargo deny check`. `rustls-platform-verifier` pulls in `webpki-root-certs`
(CDLA-Permissive-2.0, wasm32-only, linked into no build we ship), which fails
`cargo deny check licenses`. The user decided (record, decision 1) to add exactly one
`exceptions` entry to `deny.toml`, beside the `dwrote` one: `{ allow = ["CDLA-Permissive-2.0"],
crate = "webpki-root-certs" }` with a comment saying why. Do not add CDLA-Permissive-2.0 to the
global allow list, do not change `[graph]`, and make no other `deny.toml` edit; if any other
crate needs a licence `deny.toml` doesn't allow, stop and ask. List every new crate and its
licence in the finish notes.

- `src/lib.rs`: `Client::new(grants: &kiln_command::Grants)`, its allowlist the grants' `net`
  hosts (`net:*` allows any). `Client::request(method, url, headers, body) -> Result<Response,
  NetError>`, where `Response { status, headers, body }` and `body` is a
  `Stream<Item = Result<Vec<u8>, NetError>>` yielding chunks as they arrive (chunked and
  content-length bodies). Dropping the body, or the future before it resolves, closes the
  connection.
- `NetError`: `HostNotAllowed(host)` (returned before any DNS lookup or socket), `Scheme`
  (anything but `https`, except `http` to `127.0.0.1` or `[::1]` for tests), `Connect`, `Tls`,
  `Io`, `Http`, `Timeout`. Connect timeout 10 s; no total timeout (a long reply streams for
  minutes); a read idle timeout of 120 s (the API sends `ping`s).
- No redirects followed; no cookies; no proxy (record §17); no compression requested.
- One connection per request in v1 (no pool): say so in the docs.

In `crates/fe-chat-anthropic`: `src/net.rs` implements its `Transport` for `fe_net::Client`
(add `fe-net` to its `Cargo.toml`), and `AnthropicProvider::with_client(grants, key)` builds
one for `https://api.anthropic.com`.

Tests in `crates/fe-net/tests/client.rs` (top-level fns) against a loopback HTTP/1.1 server
started in the test on `127.0.0.1:0` (std `TcpListener` on a thread is enough):

- `a_chunked_body_streams_as_it_arrives`: the server writes three chunks 200 ms apart; the
  first chunk arrives before the server writes the third.
- `dropping_the_body_closes_the_connection`: after the client drops the body mid-stream, the
  server's next write fails or its read sees EOF within 2 s.
- `hosts_outside_the_grants_are_refused`: with grants `net:api.anthropic.com`, a request to
  `https://example.com/` is `HostNotAllowed` and the loopback server's accept count stays 0
  (the loopback test itself uses grants `net:127.0.0.1`); `http://api.anthropic.com/` is
  `Scheme`.
- `the_anthropic_transport_streams_a_fixture`: fe-chat-anthropic's provider over a `Client` to
  the loopback server serving one of its SSE fixtures gives the same events as its fake
  transport does (`crates/fe-chat-anthropic/tests/net.rs`).

`#[ignore]`d live tests, for a person to run: `tls_to_api_anthropic_com` (a `GET
https://api.anthropic.com/v1/models` with no key returns 401: TLS and the platform verifier
work) in `crates/fe-net/tests/client.rs`, and `live_text_reply` in
`crates/fe-chat-anthropic/tests/net.rs` (with `ANTHROPIC_API_KEY` set, a one-line reply
streams, and a second identical request shows `cache_read_input_tokens > 0` once the prompt
passes 512 tokens: the test pads the question to make it so).

## Implementation notes

**New crates in Cargo.lock, and their licences** (all on `deny.toml`'s allow
list except `webpki-root-certs`, which has the one `exceptions` entry):

| Crate | Version | Licence | Why |
|---|---|---|---|
| hyper | 1.12.0 | MIT | HTTP/1.1 client (`client`, `http1`) |
| httparse | 1.10.1 | MIT OR Apache-2.0 | hyper's HTTP/1 parser |
| want | 0.3.2 | MIT | hyper's client dispatch |
| try-lock | 0.2.5 | MIT | via want |
| http-body-util | 0.1.5 | MIT | the request body (`Full`) |
| smol-hyper | 0.1.1 | MIT OR Apache-2.0 | hyper's I/O traits over futures-io (default features off) |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | TLS, `ring`, `std`, `tls12`, `logging`; default features off |
| ring | 0.17.14 | Apache-2.0 AND ISC | rustls' crypto provider |
| untrusted | 0.9.0 | ISC | via ring |
| rustls-webpki | 0.103.15 | ISC | certificate paths, via rustls |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | via rustls |
| futures-rustls | 0.26.0 | MIT/Apache-2.0 | rustls over futures-io; default features off, `ring`, `tls12`, `logging` |
| rustls-platform-verifier | 0.6.2 | MIT OR Apache-2.0 | the OS verifier and trust store |
| rustls-native-certs | 0.8.5 | Apache-2.0 OR ISC OR MIT | the verifier's roots on Linux and the BSDs |
| openssl-probe | 0.2.1 | MIT OR Apache-2.0 | finds the system's certificate files for rustls-native-certs (Linux); pure Rust, not OpenSSL |
| schannel | 0.1.30 | MIT | the verifier on Windows |
| jni | 0.21.1 | MIT/Apache-2.0 | the verifier on Android only |
| cesu8 | 1.1.0 | Apache-2.0/MIT | via jni (Android only) |
| combine | 4.6.9 | MIT | via jni (Android only) |
| rustls-platform-verifier-android | 0.1.1 | MIT OR Apache-2.0 | the verifier on Android only |
| webpki-root-certs | 1.0.9 | CDLA-Permissive-2.0 | the verifier on wasm32 only; the `deny.toml` exception |
| windows-sys | 0.45.0, 0.52.0 | MIT OR Apache-2.0 | Windows bindings: 0.45 via jni (target-gated in jni's manifest), 0.52 via ring (Windows only) |
| windows-targets and its `windows_*` target crates | 0.42.2 | MIT OR Apache-2.0 | via windows-sys 0.45 (Windows only) |

`cargo deny check` passes (advisories, bans, licences, sources).
`cargo tree -p fe-net -e normal --target all` has no reqwest, native-tls,
openssl, webpki-roots or aws-lc-rs. Two lines in it deserve a word:

- **tokio v1.53.1, `sync` only.** Every hyper 1.x release depends on tokio
  unconditionally with the `sync` feature (its client's dispatch channels),
  which the record's § 6 notes ("hyper brings in tokio with only `sync` (no
  runtime), which is not the reason reqwest is out"). It isn't new: Cargo.lock
  already had it at this version through another crate. Its only dependency
  in the build is pin-project-lite; there is no runtime, reactor or `rt`
  feature, and nothing in fe-net spawns on or blocks in tokio. A crate that
  takes hyper can't drop it.
- **openssl-probe**, a pure-Rust crate that looks for the system's
  certificate bundle paths for rustls-native-certs. It is not OpenSSL and
  links nothing.

`webpki-root-certs` is in no Linux, macOS or Windows target's normal graph
(`cargo tree -p fe-net -e normal` for the host lists none of it, jni, cesu8,
combine or the Android crate).

**Design.** hyper's `Connection` is never spawned: the request future polls
it beside the response, and `Body` polls it beside each frame, so the client
runs under any executor (the tests use `futures::executor::block_on`) and
dropping either closes the socket. The idle timeout (120 s;
`Client::with_idle_timeout` for tests) is on the TCP reads under TLS and
hyper: a read fails once it has waited that long without a byte, so a
trickling head or chunk framing keeps it off, and hyper's connection error
(kept by the body's driver, since hyper tells the body only "connection
error") becomes `NetError::Timeout`. The connect timeout (10 s) covers DNS,
TCP, TLS and hyper's handshake. Every request sends `Connection: close` and a
`Host` from the URL; a caller's `Content-Length` and `Transfer-Encoding` are
dropped, so the body frames itself. An explicit port that isn't 1-65535
(`:abc`, `:65536`, `:0`, an empty `:`) is `NetError::Http`, not the default
port. The TLS configuration (ring,
TLS 1.2 and 1.3, the platform verifier, ALPN `http/1.1`) is built by a
client's first `https` request and shared by its clones. A URL with a user
name, or one that doesn't parse, is `NetError::Http`; `localhost` over `http`
is `Scheme` (only the loopback addresses skip TLS, since a name goes through
DNS).

`tls_to_api_anthropic_com` was run once on Linux and passed (a 401 over TLS
with the platform verifier). `live_text_reply` was not run (it needs a key
and bills it).
