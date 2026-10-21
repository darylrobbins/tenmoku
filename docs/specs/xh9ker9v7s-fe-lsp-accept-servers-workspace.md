---
task_uid: "01m55v3cxc1802s4xh9ker9v7s"
task: "T182"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::registration::watched_files::registered_globs_get_matching_changes_only passes: with **/*.rs and a relative cfg/*.toml registered, files_changed sends one didChangeWatchedFiles with src/a.rs (1) and cfg/x.toml (2) only; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit tests tests::registration::watched_files::kind_bits_filter_events, tests::registration::watched_files::unregistering_stops_them and tests::registration::watched_files::watched_files_are_advertised pass"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test tests::registration::dynamic_registration_policy passes with its refused method changed to textDocument/onTypeFormatting (the one existing test this card edits); tests::cancel's option-less watched-files registration is still refused"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names, except tests::lifecycle::initialize_advertises_m4_capabilities in crates/fe-lsp/src/tests/lifecycle.rs, which may exempt capabilities.workspace.didChangeWatchedFiles.dynamicRegistration from its all-false loop (that registration is advertised true); cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: accept servers' workspace/didChangeWatchedFiles registrations and send them the file changes their globs and kinds match (Mac T942)

Filed on argon from Mac T942 (card ~/coordination/argon-cards/zzr/T942.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txxbjvm5dhzbh6frv6n2h. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T181 (Mac T941). Held in the inbox: waits until Mac T919 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T942)".

## Problem

LSP completeness track, item 5 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
"didChangeWatchedFiles ✗: not advertised; registration refused (registration.rs:22) ... Without it, gopls (and
possibly typescript-go/pyright; unverified) miss git checkouts and codegen"). Servers that don't watch the disk
themselves register `workspace/didChangeWatchedFiles` with globs (rust-analyzer: `**/*.rs`, `**/Cargo.{toml,lock}`;
gopls: `**/*.{go,mod,sum,work}`) and expect the client to tell them of matching changes. fe-lsp:

- doesn't advertise `workspace.didChangeWatchedFiles` (`client_capabilities`, `crates/fe-lsp/src/server.rs:1997`);
- refuses any registration but `workspace/didChangeConfiguration`, all or nothing (`SUPPORTED`,
  `crates/fe-lsp/src/registration.rs:21`), and keeps only id → method, not the options;
- the refusal is pinned by `tests::registration::dynamic_registration_policy`
  (`crates/fe-lsp/src/tests/registration.rs:115-125`).

Once the card this depends on lands, fe-lsp has `LspGlob` and `LspManager::notify_sessions`.

## Change

1. `crates/fe-lsp/src/registration.rs`: `workspace/didChangeWatchedFiles` joins `SUPPORTED`. Its
   `registerOptions` (`DidChangeWatchedFilesRegistrationOptions`) are parsed and each watcher's glob built
   with `LspGlob::new`; missing options, options that don't parse, or a glob that doesn't build refuse the
   whole request as an unsupported method does today (`RequestFailed`, -32803). So the option-less
   registration `crates/fe-lsp/src/tests/cancel.rs:337-346` sends is still refused, and that test passes
   unchanged. `Registrations` keeps, per id, the method
   and, for this method, its watchers: `(LspGlob, WatchKind)` (the kind defaulting to create | change |
   delete). Unregistering an id drops its watchers. Module docs: the new method and its rule.
2. New `crates/fe-lsp/src/watched_files.rs`: `LspManager::files_changed(&self, changes: Vec<(PathBuf,
   lsp_types::FileChangeType)>)`. For each session, through `notify_sessions`: the changes matching any of
   its watchers whose kind includes the change's type, in the order given, each path once (its last type),
   as `file:` URIs; one `workspace/didChangeWatchedFiles` with them, or nothing if none match. A session with
   no such registration gets nothing. (The registrations are read on the actor, where the router keeps them:
   pass the changes in and filter there, or keep a shared copy; either way a registration answered before the
   call is applied.)
3. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `workspace.didChangeWatchedFiles =
   {dynamicRegistration: true, relativePatternSupport: true}`. Registrations already reset with the session
   (`server.rs:1202`).
4. `crates/fe-lsp/src/router.rs`: the registration handler (`:165-171`) is unchanged unless the watchers
   must be shared with the actor (point 2); keep any change to it there.
5. `crates/fe-lsp/src/lib.rs`: `mod watched_files`.
6. Tests:
   - `crates/fe-lsp/src/tests/registration.rs` `dynamic_registration_policy`: the method it expects refused
     becomes `textDocument/onTypeFormatting` (still unsupported); the one existing test this card edits.
     Declare `mod watched_files;` in the same file.
   - New `crates/fe-lsp/src/tests/registration/watched_files.rs`:
     - `registered_globs_get_matching_changes_only`: the fake registers `**/*.rs` and `{baseUri: <root>/cfg,
       pattern: "*.toml"}`; `files_changed` of `src/a.rs` (created), `cfg/x.toml` (changed), `README.md`
       (changed) and `cfg/sub/y.toml` (deleted) sends one notification with the first two, as URIs with types 1
       and 2.
     - `kind_bits_filter_events`: a watcher with `kind: 4` (delete) gets only the deletions.
     - `unregistering_stops_them`: after `client/unregisterCapability` of the id, `files_changed` sends nothing;
       a bad glob in a registration refuses it and records nothing.
     - `watched_files_are_advertised`: the initialize params carry the capability above.

Linux-OK.

## Implementation notes

- Filtering happens on the actor, as point 2 allows. `files_changed` collapses the changes (each path once,
  in its first place, with its last type; non-absolute paths skipped) and queues them for every session
  through `notify_sessions` under an internal method, `fe-lsp/filesChanged`. The actor's `Command::Notify`
  arm calls `watched_files::for_session`, which keeps the changes that its session's `Registrations::watches`
  wants and sends them as `workspace/didChangeWatchedFiles`, or sends nothing. The registrations are read
  under the session mutex that the router's handler writes before replying, so a registration answered
  before the call is applied. The internal method exists because `tests::registration::globs::
  notifications_reach_the_session_in_order` (T181) sends a raw `workspace/didChangeWatchedFiles` through
  `notify_sessions` with no registration and expects it to arrive unfiltered. A raw one is still sent as
  it is. router.rs and manager.rs are unchanged.
- `Registrations` keeps, per id, a `Registered` enum: `Configuration`, or `WatchedFiles(Vec<(LspGlob,
  WatchKind)>)`. lib.rs drops the `#[allow(dead_code)]` on `mod glob` and `mod notify`, and server.rs drops
  the one on `Command::Notify`, since all three are now used.
- `tests::lifecycle::initialize_advertises_m4_capabilities` asserted that every `dynamicRegistration` in
  the client capabilities is false, which point 3 breaks. Rev 2 put `crates/fe-lsp/src/tests/lifecycle.rs`
  in scope, and its all-false loop now exempts `capabilities.workspace.didChangeWatchedFiles.
  dynamicRegistration`, which must be true.
- `tests::registration::globs::notifications_reach_the_session_in_order` (T181) is flaky at the base too
  (1 of 40 runs at 48195b99): it times out after 60s "waiting for initialized". The fake records the
  `initialize` request before `answer` calls `clock.sleep(initialize_delay)`, so the test's
  `fake_clock.advance(1s)` can land between the two, and the deadline is then measured from the advanced
  time. That phase sends nothing, and this card doesn't touch it.
