---
task_uid: "01m55v3cmq92h6myqnr1we17wg"
task: "T181"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit tests tests::registration::globs::lsp_globs_match_as_vs_code and tests::registration::globs::relative_patterns_match_under_their_base pass with the spec's cases (string patterns against absolute paths, relative patterns under their base, an invalid pattern an Err); they fail at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::registration::globs::notifications_reach_the_session_in_order passes: a hover request then notify_sessions arrive at the fake in that order with the closure's params; None sends nothing; nothing before initialize"
    required: true
    manual: false
  - id: "C3"
    text: "globset is a workspace dependency used by fe-lsp only; Cargo.lock changes only by fe-lsp's dependency entry; cargo deny check passes; no protocol change (client capabilities unchanged)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; the Cargo change is the one named, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: match LSP glob patterns as VS Code does and send a notification to each of a manager's servers in order with its requests (Mac T941)

Filed on argon from Mac T941 (card ~/coordination/argon-cards/zzr/T941.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txwqa1ddhe842eapdv5pe. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Held in the inbox: waits until Mac T850, Mac T921, Mac T847 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T941)".

## Problem

LSP completeness track, items 5 and 8 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`
§3: didChangeWatchedFiles, and file operations). Both need two pieces fe-lsp lacks:

- **LSP globs.** Watched-file registrations (`FileSystemWatcher.globPattern`) and file-operation filters
  (`FileOperationPattern.glob`) are LSP glob patterns: `*` within a path segment, `**` across segments, `?`,
  `{a,b}`, `[a-z]` and `[!a-z]`, given as a string or a `RelativePattern { baseUri, pattern }`. Nothing in
  fe-lsp matches them. `globset` is already in `Cargo.lock` (through `ignore` 0.4.33, used by fe-project) but
  isn't a dependency of fe-lsp or of the workspace.
- **Notifications to a root's servers.** The only manager-to-server notification is
  `Command::Configure` (`crates/fe-lsp/src/server.rs:1108-1115`, from `crates/fe-lsp/src/manager.rs:1099`).
  `didChangeWatchedFiles` and `didCreateFiles`/`didRenameFiles`/`didDeleteFiles` concern no one document, and
  must keep their order with requests and document sync, as `Configure` does through the actor.

This card adds both, with no protocol change: nothing calls them until the watched-files and file-operation
cards.

## Change

1. Root `Cargo.toml`: `globset = "0.4"` in `[workspace.dependencies]` (the version `ignore` already pulls
   in); `crates/fe-lsp/Cargo.toml`: `globset.workspace = true`; `Cargo.lock`: fe-lsp's entry gains it and
   nothing else changes. `cargo deny check` passes.
2. New `crates/fe-lsp/src/glob.rs`:
   - `pub(crate) struct LspGlob`, built by `LspGlob::new(pattern: &lsp_types::GlobPattern) ->
     Result<LspGlob, String>` through `globset::GlobBuilder` with `literal_separator(true)` (so `*` stops at
     `/`) and `{a,b}` alternation on. A plain (string) pattern is matched against the whole absolute path,
     as VS Code matches a string pattern, so `**/*.rs` matches at any depth and `*.toml` matches nothing; a
     relative pattern matches paths under its `baseUri`'s path, relative to it. `LspGlob::new` takes no root. Case-sensitive (VS Code's glob matching is too; on a case-insensitive volume a path
     spelled differently from the pattern doesn't match, a documented limit).
   - `pub(crate) fn matches(&self, path: &Path) -> bool`.
3. New `crates/fe-lsp/src/notify.rs` and, in `server.rs`, a new variant `Command::Notify { method: &'static
   str, params: serde_json::Value }`: the actor sends it as `Configure` sends its notification (through the
   socket, in order with queued requests and document sync, counted in `sent`). In `notify.rs`:
   `LspManager::notify_sessions(&self, method, params_for: impl Fn(ServerId, &ServerCapabilities) ->
   Option<Value>)`, which sends, to each running server of the manager in id order, the params the closure
   gives it (none: nothing sent). Servers not yet initialized get nothing.
4. `crates/fe-lsp/src/lib.rs`: `mod glob; mod notify;` (crate-private).
5. Tests: new `crates/fe-lsp/src/tests/registration/globs.rs`, declared by `mod globs;` in
   `crates/fe-lsp/src/tests/registration.rs` (its only change):
   - `lsp_globs_match_as_vs_code`: `**/*.rs` matches `/r/a.rs` and `/r/src/b/c.rs`, not `/r/a.rsx`;
     `**/{Cargo.toml,Cargo.lock}` matches both at any depth; `**/src/?.rs` matches `/r/src/a.rs`, not
     `/r/src/ab.rs`; `**/[!.]*.py` matches `/r/x.py`, not `/r/.x.py`; `**/node_modules/**` matches
     `/r/a/node_modules/b/c.js`; `*.toml` matches no absolute path.
   - `relative_patterns_match_under_their_base`: `{baseUri: file:///r/sub, pattern: "*.go"}` matches
     `/r/sub/a.go`, not `/r/a.go` or `/r/sub/x/a.go`; an invalid pattern (`[`) is an `Err`.
   - `notifications_reach_the_session_in_order`: with the fake started, a `textDocument/hover` request then
     `notify_sessions("workspace/didChangeWatchedFiles", ..)`: the fake records the request, then the
     notification with the closure's params; a closure answering `None` sends nothing; before the fake has
     initialized, nothing is sent.

Linux-OK.

## Implementation notes

- `LspManager.servers` is private to manager.rs, outside the scope, so the manager gets a one-line-body `server_ids()` accessor (dead-code allowed until the watched-files card) and `restart.rs`'s exhaustive `Command` match gets a `Notify` arm. Both are the minimum the scope's files could not do themselves.
- async-lsp types notifications at compile time (`AnyNotification` is `non_exhaustive`), so `notify::send` maps the four method names fe-lsp will send (`didChangeWatchedFiles`, `didCreate/Rename/DeleteFiles`) to JSON-params notification types; any other method is logged and dropped.
- Review round 1: `empty_alternates(true)` and `backslash_escape(false)` follow VS Code; a trailing `/**` matches the directory itself by also trying the path with a `/` appended. `?` on a non-ASCII character is an accepted limit of globset (it matches bytes), documented in glob.rs and pinned by a test.
