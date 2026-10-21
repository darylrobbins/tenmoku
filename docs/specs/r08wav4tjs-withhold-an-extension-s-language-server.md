---
task_uid: "01m5fg31qa06jj16r08wav4tjs"
task: "T208"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "lsp_consent_tests::invocation_covers_command_args_env_and_script_hash: Invocation::of differs when the resolved executable, an argument, an env var, or the content of a script file inside the extension directory named by an argument differs, and is equal when only languages, settings, initialization_options, root_markers or readiness differ"
    required: true
    manual: false
  - id: "C2"
    text: "lsp_consent_tests::interpreters_are_flagged: node, python3, sh, bash and deno executables report runs_arbitrary_code(), rust-analyzer and zls don't"
    required: true
    manual: false
  - id: "C3"
    text: "lsp_consent_tests::consent_store_round_trips: grant and decline entries keyed by canonical extension directory and server name survive a reload from a tempdir file, a corrupt file reads as empty with a warning, and two writers don't drop each other's entries"
    required: true
    manual: false
  - id: "C4"
    text: "lsp_consent_tests::an_unconsented_server_is_withheld_and_pending: set_extension_servers with a new extension server leaves extension_servers(cx) empty and pending_extension_consents(cx) naming it with its invocation"
    required: true
    manual: false
  - id: "C5"
    text: "lsp_consent_tests::granting_starts_it_and_persists: grant_extension_server makes it appear in extension_servers(cx), pending empties, and a fresh process (new global, same file) has it granted with no pending entry; decline keeps it out and isn't re-asked for the same invocation"
    required: true
    manual: false
  - id: "C6"
    text: "lsp_consent_tests::a_changed_invocation_keeps_the_old_and_asks_again: after a grant, changing args, then env, then the script file's content each leaves extension_servers(cx) with the granted command/args/env and adds a PendingConsent with previous set; changing only languages doesn't ask"
    required: true
    manual: false
  - id: "C7"
    text: "extension_consent::an_installed_lsp_extension_is_pending_until_granted: an extension with [contributes.lsp.zls] in a temp extensions dir, loaded through fastedit's config (as crates/fastedit/tests/extension_servers.rs does), is pending and not in fe_workbench::extension_servers until granted; the existing extension_servers test is updated to grant first and otherwise unchanged"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-workbench -p fastedit, cargo clippy -p fe-workbench -p fastedit --all-targets -- -D warnings and cargo deny check licenses pass"
    required: true
    manual: false
---

# Withhold an extension's language server until the user consents, and ask again when its command, arguments, environment or script change (extensions record §1, Q5) (Mac T1037)

## Problem

An extension's `[contributes.lsp.<name>]` names a program fastedit then spawns with the user's rights,
outside any sandbox (extensions decision record `docs/decisions/vmtnmkay62-extensions-and-plugins.md`
§1, §12 Q5, accepted 2026-10-21). T844 starts such a server as soon as the extension loads, so an
extension dropped into the extensions directory, or an edit to its `extension.toml`, runs a program
nobody approved. The record requires: the server doesn't spawn until the user consents; consent is
bound to the server's whole effective invocation (executable, arguments, environment, and the content
of a script the arguments name inside the extension's directory); any change leaves the old server
running (or off) and asks again; consent persists.

Reuse: `crates/fe-workbench/src/lsp.rs` `set_extension_servers` (:268) and the `ExtensionServers`
global (:152); `fe_lsp::LocalLauncher::resolve` (`crates/fe-lsp/src/launcher.rs:484`) resolves a
command to an absolute path on the login shell's `PATH` (blocking: run it off the main thread);
`crates/fastedit/src/config.rs` `extension_servers` (:251) builds the specs;
`kiln_core::extension::ExtensionSet::license_notices` is the model for a lock-guarded state file
(`kiln_core::trust::{locked, write}`).

## Change

New `crates/fe-workbench/src/lsp_consent.rs` (with `#[cfg(test)] mod lsp_consent_tests;` and the new
file `crates/fe-workbench/src/lsp_consent_tests.rs`):

- `Invocation { command: PathBuf, args: Vec<String>, env: BTreeMap<String,String>, script_hashes:
  BTreeMap<String,String> }` made by `Invocation::of(spec, extension_dir, resolved_command)`: the
  resolved absolute executable (as written if it can't be resolved), the full argument vector, the env
  the spec sets, and a SHA-256 of each argument naming an existing file inside `extension_dir`
  (add `sha2` as a direct `fe-workbench` dependency; it is already in `Cargo.lock`; keep `deny.toml`
  passing). `Invocation::runs_arbitrary_code()` is true when the executable's file name is an
  interpreter (`node`, `nodejs`, `python`, `python3`, `sh`, `bash`, `zsh`, `fish`, `dash`, `deno`,
  `bun`, `ruby`, `perl`, `php`, `lua`, `pwsh`). Not part of it: `languages`, `settings`,
  `initialization_options`, `root_markers`, readiness (they don't change what runs).
- A consent store, `<state dir>/extensions-lsp-consent.toml` (`fe_config::paths::state_dir()`, path
  passed in by `fe_workbench::set_extension_consent_file(cx, path)` so tests use a tempdir), written
  under a lock and atomically. One entry per (canonical extension directory, server name) holding the
  last decision, `granted` or `declined`, and the full `Invocation` it was for.
- `set_extension_servers(cx, servers: Vec<ExtensionServer>)` where `ExtensionServer { extension_id,
  extension_name, extension_dir, spec }`. It computes each `Invocation` on a background task, then
  hands the managers only the servers whose invocation equals a `granted` entry. A server whose
  invocation differs from a granted entry keeps its granted invocation (command, args, env) so a
  running server isn't stopped and nothing new runs; a server with no entry, a `declined` entry for the
  same invocation, or a changed invocation is recorded as a `PendingConsent { extension_id,
  extension_name, extension_dir, server, invocation, previous: Option<Invocation>, declined: bool }`.
- Public API: `pending_extension_consents(cx) -> Vec<PendingConsent>`,
  `grant_extension_server(cx, &PendingConsent)`, `decline_extension_server(cx, &PendingConsent)`
  (a declined invocation is not asked about again until it changes), and a `Global` change
  notification so a view can follow. `extension_servers(cx)` returns the granted ones only.
- `crates/fastedit/src/config.rs`: `extension_servers` produces `ExtensionServer`s (extension id,
  name, dir) and passes them to `set_extension_servers`; `main` sets the consent file path next to
  the other state files.

The prompt and the list of extensions are the next two cards; until they land, consent is given
through the API only (tests), which is the intended order: nothing runs unconsented.

## Out of scope

The prompt UI; the extensions list; workspace extensions; the user's own `[lsp]` settings and
built-in servers (they are the user's own config and unchanged).

## Implementation notes

- `fe_workbench::lsp_consent` holds the state; `lsp.rs`'s old `set_extension_servers` became the
  crate-private `hand_extension_servers`, which `lsp_consent` calls with the granted specs only.
  `ExtensionConsent` is the observable `Global` (`cx.observe_global::<ExtensionConsent>`).
- kiln's `trust::{locked, write}` are crate-private, so `lsp_consent` has its own copies: a
  `<file>.lock` taken with `File::lock`, and a temporary file renamed over the target, both 0600.
  A decision is written on the main thread before `grant_extension_server` /
  `decline_extension_server` return (they return the write's `io::Result`; the decision holds for
  the session either way), so a quit straight after can't lose it. This session's decisions
  overlay the file's, for an evaluation that read the file before.
- Every launch is checked (review round 1): each manager's launcher is wrapped in
  `lsp_consent::ConsentGate`. A launch whose spec has the name, command and arguments of a handed
  extension spec works its invocation out again (resolution and every hash) and runs only if it is
  the granted one, with the command replaced by the granted absolute executable; one that can't be
  bound to an absolute path is refused. Other launches (built-ins, the user's own `[lsp]` servers,
  a user override of an extension server's command or arguments) pass through.
- One grant per server name (round 2): the managers use only the first contribution of a name, so
  only the first granted one is handed and checked; a later extension's server of the same name
  isn't handed (logged), so a launch can't pass on another extension's grant.
- Restarts (rounds 1-2): the gate keeps the last launch it checked of each (server, root), whatever
  came of it (running, ended, refused, or failed in the host's launcher), with the grant it was
  checked against; the process's end only marks it not running. After each hand-off,
  `hand_extension_servers` restarts (`LspManager::restart`) each one running other than what's
  granted, or not running and checked against another grant; each is then taken as checked against
  the current grant, so it's restarted once, not at every reload. A refusal at launch, and a launch
  recorded under a grant replaced while it was under way, wake the app (a channel to a main-thread
  task) to work the servers out again, so a change found only at launch (the `PATH` resolving
  elsewhere) becomes pending and a launch that lost the race is restarted.
- Script hashes: each argument, and the value of each `--flag=value`, is tried as a path,
  absolute or relative to the extension's directory; the executable too, when it's absolute.
  A file whose canonical path is inside the extension's canonical directory is hashed, keyed by
  the name as written, the value being the hash and the canonical path (round 2: a name pointed at
  another named file is a change). A directory argument, and modules a named script imports, aren't hashed (the record
  asks for the named script). fe-lsp starts a server in the workspace folder, so a relative
  argument names a workspace file at run time; consent hashes the extension's file of that name.
- Command resolution uses `fe_lsp::LoginPath::system().resolve` with the spec's `PATH`, off the
  main thread; a relative command (with a `/`) is kept as written, since fe-lsp resolves it under
  the workspace folder. Unit tests don't run the login shell for a bare name.
- A changed invocation: if this session handed the server with its granted invocation, the whole
  spec it was handed stays (so a combined args-and-languages change restarts nothing until the user
  decides); in a later session the granted command, args and env are handed only while
  `Invocation::of` of that spec, worked out again, equals the grant (so a retargeted symlink or a
  newly created named file withholds it). Either way the gate refuses any launch that no longer
  matches. Declining a changed invocation replaces the grant (one entry per server), so what was
  granted stops.
- `PendingConsent.declined` entries stay in `pending_extension_consents` so the extensions list can
  offer them; a prompt asks only about `declined == false`.
- `runs_arbitrary_code` also flags versioned names (`python3.12`, `lua5.4`) and `env`, `npx`,
  `bunx`, `uvx`, `ksh`, `luajit`, `java`, `python2`.
- `config.rs` lists each server's command, arguments and flag values to kiln's `extension_files`,
  so a change to a script the arguments name reloads the extensions and is asked about.
- Carried T144 F1 (partly): each server table is read on its own, and one that doesn't read (such
  as `args = [1]`) keeps its last good settings from the previous load, so a malformed edit no
  longer stops a server that worked or its siblings (`extension_consent::
  a_malformed_edit_keeps_the_last_good_server`). Positioned diagnostics for array items and
  `readiness` need kiln's schema check (`kiln/crates/kiln-core/src/layers.rs`), outside this
  card's scope: a follow-up.
