---
task_uid: "01m5f68mwbw32f0qqt57vjbbqd"
task: "T248"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::registry::outside_root::files_outside_the_root_get_their_marker_roots_server passes: a file under trusted /t/other starts a second server whose initialize names /t/other; a second file joins it; files under /t/w use the window root's server; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit tests tests::registry::outside_root::untrusted_marker_roots_wait_for_trust and tests::registry::outside_root::no_window_root_still_routes pass"
    required: true
    manual: false
  - id: "C3"
    text: "Files under the window root are routed exactly as at the base: the existing fe-lsp tests pass unchanged, except that tests::registry::lazy_start_per_server_and_root drops its block asserting files outside the root or with no root get no server (the behaviour this card replaces, now covered by outside_root.rs); tests/registry.rs otherwise only gains its mod line"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec and C3 name; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: start a server at a file's marker root for files outside the window root or with no root, under the same trust rule, instead of giving them none (Mac T959)

## Problem

LSP completeness track, item 10 (user-approved 2026-10-17). Once the card this depends on lands, each server
spec has root markers and `fe_lsp::server_root(path, window_root, markers)` finds a file's root. `LspManager::open`
(`crates/fe-lsp/src/manager.rs:938-955`) still drops a document with no root, or whose path isn't under the root
given (`root.filter(|root| is_under(path, root))`), and servers are keyed by (name, root) with the one folder
sent in `initialize` (`crates/fe-lsp/src/server.rs:1983-1988`).

## Change

1. `crates/fe-lsp/src/manager.rs` `open`: the server for the document's language is chosen as today; its root
   is `server_root(path, root, &spec.root_markers)` instead of requiring `path` under `root`. So a file under
   the window root is routed exactly as today, and one outside it (or with no window root) goes to a server
   at its marker root, started if none runs there for that name. Trust is unchanged: a root the trust
   predicate (`set_trust`) rejects keeps its documents without a server until trusted, as today for the
   window root. Its doc comment describes the rule.
2. Servers at several roots under one manager are already keyed by (name, root); `servers_at`, statuses and
   restarts work per key as today. Settings changes and `update_settings` apply to every root's servers. The
   kill switch and shutdown stop them all.
3. Tests: new `crates/fe-lsp/src/tests/registry/outside_root.rs`, declared by `mod outside_root;` in
   `crates/fe-lsp/src/tests/registry.rs`, with the fake launcher:
   - `files_outside_the_root_get_their_marker_roots_server`: with window root `/t/w` and trust allowing
     `/t/other`, opening `/t/other/Cargo.toml`'s sibling `src/main.rs` starts a second server whose
     `initialize` names `/t/other` as root and workspace folder; a second file under `/t/other` joins it; a
     file under `/t/w` uses the window root's server.
   - `untrusted_marker_roots_wait_for_trust`: with trust allowing only `/t/w`, a file under `/t/other` gets no
     server and is kept; `set_trust` allowing it starts one and opens the file.
   - `no_window_root_still_routes`: `open(None, ..)` for a file with a marker root starts a server there.

Order: after T844 and T850 (both in `manager.rs`). Linux-OK.

## Implementation notes

- `Route` keeps the window root and path it was opened with; its `root` is `server_root` of them and
  the markers of the spec its language has. `open` computes it (no spec yet: no markers, so the window
  root or the file's folder), and `reconcile` re-derives it for every routed document before servers
  are restarted or waiting documents placed, so a document that only gets a server later, or whose
  server's `root_markers` change (which restarts it, `needs_restart`), lands at the root those markers
  give. A document whose language no server lists keeps its root. `is_under` is gone: `server_root`
  resolves `..` lexically, so a path that climbs out of the window root goes to its marker root.
- `tests::registry::lazy_start_per_server_and_root` asserted that files outside the root, or opened
  with no root, get no server, which is what this card replaces and which `no_window_root_still_routes`
  contradicts. That block is removed (C3 and C4 were respecced to say so); `outside_root.rs` covers the
  new rule.
- The workbench (`fe-workbench/src/lsp.rs`) still only opens buffers whose path is under the window
  root, so the app routes outside files once that filter is lifted; that is a card of its own.
