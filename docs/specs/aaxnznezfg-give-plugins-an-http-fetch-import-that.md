---
task_uid: "01m5j50tcxabkrkhaaxnznezfg"
task: "T279"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "C1 net::a_granted_host_is_reached: a fetch of https://api.example.com/x with net:api.example.com reaches the backend once"
    required: true
    manual: false
  - id: "C2"
    text: "C2 net::an_ungranted_host_is_refused_before_any_backend_call: the backend sees no request"
    required: true
    manual: false
  - id: "C3"
    text: "C3 net::a_redirect_to_an_ungranted_host_is_refused: a 302 to another host stops with an error, the backend sees one request"
    required: true
    manual: false
  - id: "C4"
    text: "C4 net::redirects_are_followed_to_a_limit_and_https_never_downgrades: five hops to granted hosts succeed, a sixth fails, and a redirect to http fails"
    required: true
    manual: false
  - id: "C5"
    text: "C5 net::odd_hosts_need_a_literal_grant: IP literals, a trailing dot, upper case, userinfo and punycode forms are matched or refused by their normalized host, never by substring"
    required: true
    manual: false
  - id: "C6"
    text: "C6 net::net_star_reaches_any_host: with net:* every well-formed host passes"
    required: true
    manual: false
  - id: "C7"
    text: "C7 net::the_connected_peer_must_equal_the_checked_host: a backend reporting a different peer name is refused"
    required: true
    manual: false
  - id: "C8"
    text: "C8 net::the_response_is_capped: a body over 16 MiB is an error"
    required: true
    manual: false
  - id: "C9"
    text: "C9 fe-plugin net::the_fe_net_backend_fetches_from_a_local_server: the fe-net backend fetches from a 127.0.0.1 http server with net:127.0.0.1 and is refused without it"
    required: true
    manual: false
  - id: "C10"
    text: "C10 cargo nextest run -p kiln-plugin -p fe-plugin and cargo clippy -p kiln-plugin -p fe-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Give plugins an HTTP fetch import that checks the host against net grants on the first request and on every redirect, over a fake backend and the fe-net client (Mac T1055)

## Problem

`net:<host>` is enforced by the host's HTTP import, not by the plugin: the plugin has no raw socket, and the host checks
the destination host (after DNS, against the resolved name and on each redirect) against the grant (decision 1). `fe-net`
already gives the app a host-checked `Client` that does not follow redirects; the plugin import is the missing layer.

## Change

- `kiln/crates/kiln-plugin/src/net.rs`, `wit/net.wit`: the `fetch(request) -> response` import over a `NetBackend`
  trait (`send(request) -> Response` and the peer name it connected to), so the policy is testable without sockets:
  the URL's host (lower-cased, trailing dot removed, userinfo rejected, IDNA ASCII form) must match a granted
  `net:<host>` or `*`; an IP literal needs a literal grant of that address; redirects are followed by the host up to a
  named limit (5), each `Location` re-checked against the grant, https to http refused, and the response body capped
  at `Limits::max_read_bytes`; the peer name the backend reports must equal the checked host.
- `crates/fe-plugin/src/net.rs`: the `NetBackend` over `fe_net::Client` (pass the plugin's own `Grants` to it so the
  client's own check agrees), headers filtered to a safe set, no cookies, no proxy.

## Notes

Read `crates/fe-net/src/lib.rs` first: it checks the host before DNS and returns 3xx as is, which is what this layer
builds on. Note in the finish notes whether the connection is pinned to the address checked; if fe-net cannot do that,
say so. Tests use a fake backend, plus one fe-plugin test against a local 127.0.0.1 http server (fe-net allows it).

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Implementation notes

- Scope was widened by respec to `kiln-plugin`'s and `fe-plugin`'s `Cargo.toml`, `Cargo.lock`,
  `kiln-plugin/src/bindings.rs` (`net::State` now has fields, so `HostState::new` takes its
  `Default`) and `kiln-plugin/tests/bindings.rs` (fetch left the unimplemented-imports list).
- kiln-plugin parses URLs with the `url` crate (WHATWG, as browsers do; already in the lock),
  which gives the IDNA ASCII form, lower case, percent-decoded hosts and canonical IP text
  (`127.1`, `0x7f.0.0.1` and `2130706433` are all `127.0.0.1`). The backend is handed the
  re-serialized URL, not the plugin's string, so it parses what was checked.
- Grants can't name an IPv6 address (`net:` refuses `:`), so an IPv6 literal is reached only
  with `net:*`. `http` is allowed only to `127.0.0.1` and `[::1]`, as fe-net allows it.
- `State::default()` (what the runner builds today) has no grants and no backend: every fetch
  is `denied`. Wiring `State::new(grants, limits, FeNet)` into plugin loading is the runner's.
- Pinning: the connection is **not** pinned to an address. fe-net resolves the checked name
  itself when it connects and reports no peer, so `FeNet`'s peer is the URL's host as fe-net's
  own parser (`http::Uri`) reads it: the name fe-net connects to and verifies the TLS
  certificate against. That catches a URL the two parsers read differently, but not DNS
  answers; with name grants and https, the certificate check is what binds the peer to the
  name. Pinning needs fe-net to resolve once and expose the address it connected to.
- `fetch` is synchronous, like every kiln import today; a long fetch holds the plugin's
  thread up to its timeout (default 30 s), past the 100 ms call deadline, until imports
  become async.
