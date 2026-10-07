---
task_uid: "01m49phe4gbc4cqyzvmhv90wdb"
task: "T141"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-config test tests::lsp::lsp_defaults_and_user_overrides: Settings::default().lsp has enabled true, max_file_bytes 52428800, typescript_server \"typescript-language-server\" and the §3.5 timeouts; a user file setting command, args, env, languages, readiness, ready_ignores_progress, initialization_options, settings and enabled under [lsp.servers.rust-analyzer] loads them; readiness = \"sometimes\" is an error diagnostic and the layer keeps its last good table; tests::settings::schema_matches_golden passes against the regenerated settings.schema.json"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test tests::lsp::project_layer_may_only_narrow: a project file setting any of command, args, env, settings, initialization_options, languages, readiness, ready_ignores_progress, max_file_bytes, timeouts or typescript_server leaves the effective Settings as without it and yields a warning \"LSP settings in project files are ignored until workspace trust exists\"; lsp.enabled = false and lsp.servers.pyright.enabled = false in the project file take effect; the same holds after SettingsStore::reload of the edited project file, and with no user file at all"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::registry::builtin_specs: rust-analyzer (command rust-analyzer, languages [rust], server-status, ready_ignores_progress [rust-analyzer/flycheck/]), pyright (pyright-langserver --stdio, [python], progress), typescript-language-server (typescript-language-server --stdio, javascript, typescript and tsx, progress) and typescript-go (tsc --lsp --stdio, the same languages, progress) are built in; with default settings a typescript document maps to typescript-language-server, and with lsp.typescript_server = \"typescript-go\" to typescript-go; each language maps to its languageId from docs/decisions/lsp-client.md §3.2"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test tests::registry::user_settings_and_host_specs: a user override of command, args and settings merges onto the built-in spec; a spec registered through LspManager::register_spec with the same name wins over settings; with lsp.enabled = false, or lsp.servers.pyright.enabled = false, opening a Python document launches nothing"
    required: true
    manual: false
  - id: "C5"
    text: "fe-lsp test tests::registry::lazy_start_per_server_and_root: nothing is launched before the first open; opening a .py document under the root launches pyright once and nothing else; a second .py document launches nothing more; closing every .py document leaves pyright running; a document outside the root, and one opened with no root, launch nothing and have no server"
    required: true
    manual: false
  - id: "C6"
    text: "fe-lsp test tests::registry::project_settings_never_reach_the_server: with a SettingsStore whose project file sets rust-analyzer's command, args, env and settings, at load and again after a reload, the FakeLauncher receives the built-in or user command, args and env, and workspace/configuration answers carry no project value; a project file setting lsp.servers.rust-analyzer.enabled = false stops the running server"
    required: true
    manual: false
  - id: "C7"
    text: "fe-lsp test tests::registry::configuration_answers_and_changes: workspace/configuration answers each item with the value at its dotted section in the spec's settings, or null when absent; changing only settings sends workspace/didChangeConfiguration and launches nothing; changing args or command shuts the server down and launches it again"
    required: true
    manual: false
  - id: "C8"
    text: "fe-lsp test tests::registry::bare_commands_use_the_login_shell_path: a bare command resolves against the PATH printed by an injected login shell, which runs once for several launches; an absolute command is used as is; a shell that fails or takes over 2 s leaves the process PATH in use and logs why"
    required: true
    manual: false
  - id: "C9"
    text: "scripts/check-no-gpui.sh prints ok: fe-config and ok: fe-lsp; no Cargo.toml or Cargo.lock change; crates/fe-config/src/lib.rs is unchanged"
    required: true
    manual: false
---

# Choose and start language servers from built-in specs and [lsp] settings, ignoring project-file LSP settings

# Choose and start language servers from built-in specs and [lsp] settings, ignoring project-file LSP settings

Card 3 of `docs/decisions/lsp-client.md` §8. The design is §3.2 (instances), §3.3 (configuration and the
trust boundary), §3.4 "Start: When", and §3.6's `workspace/configuration` row. It builds on card 1's
crate and card 2a's `LspManager::open` and card 2b's identity table.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q2 (a):** fastedit never installs or bundles servers. Commands come from `PATH`, settings or the
  host. A missing command is a launch error: card 1 already stops the server with it, and card 4
  words it as "stopped: rust-analyzer not found".
- **Q5 (c):** both TypeScript servers are configured. typescript-language-server (tsserver) is the
  default, and typescript-go is behind a setting.
- **Q6 (a):** lazy start per (server, root). A server starts at the first document of its language
  opened under the root, and stays up after its last document closes.
- **Q7 (c):** the policy is left to the host. Standalone fastedit starts servers in any workspace.
  Project-layer LSP settings are ignored, except the keys that narrow. A host refuses through its
  launcher.

## Change

### `fe-config`

`crates/fe-config/src/settings.rs`: `Settings` gains `lsp: LspSettings`. The new types (`LspSettings`,
`LspServerSettings`, `LspTimeouts`) live in `settings.rs`, reachable as `fe_config::settings::…`, so
`crates/fe-config/src/lib.rs` (in T124's scope) doesn't change.

- **`[lsp]`** has:
  - `enabled` (true)
  - `max_file_bytes` (52428800)
  - `typescript_server`: `"typescript-language-server"` (default) or `"typescript-go"`
  - `timeouts` in milliseconds per method, with §3.5's defaults: definition 5000, workspace_symbol
    5000, references 30000, hover 3000, completion 3000, resolve 2000. Card 4 applies them.
- **`[lsp.servers.<name>]`** has `enabled`, `command`, `args`, `env`, `languages`, `readiness`
  (`"server-status"` or `"progress"`), `ready_ignores_progress`, `initialization_options` and a free-form
  `settings` table.
- **The trust filter** (§3.3), applied when layers merge, so hot reload gets it too:
  - from the project layer, only `lsp.enabled = false` and `lsp.servers.<name>.enabled = false`
    apply
  - every other `lsp.*` key there is dropped, with a warning diagnostic: "LSP settings in project
    files are ignored until workspace trust exists"
  - the store must know which layer is the project's, even when there is no user file
- `settings.schema.json` is regenerated (its golden test).

### `fe-lsp`

- **`src/registry.rs` (new).**
  - Built-in specs:
    - `rust-analyzer`: `server-status` readiness, `ready_ignores_progress = ["rust-analyzer/flycheck/"]`
    - `pyright`: `pyright-langserver --stdio`
    - `typescript-language-server`: `typescript-language-server --stdio`, for fe-syntax's
      `javascript`, `typescript` and `tsx`
    - `typescript-go`: `tsc --lsp --stdio`, same languages, used only when `typescript_server =
      "typescript-go"`
  - Specs are merged from the built-ins, the user's settings and host-registered specs. A host spec
    wins by name.
  - The server for a document is the first enabled server whose `languages` holds the document's
    fe-syntax language.
  - The LSP `languageId` comes from §3.2's table.
- **`src/manager.rs`.**
  - `LspManager::open` picks the server by language and starts one instance per (server, root) on
    first use. A document outside the root, or with no root, gets no server.
  - `LspManager::register_spec(spec)` is for hosts.
  - `LspManager::update_settings(&LspSettings)`: a change to a running server's `command`, `args`,
    `env`, `languages`, `readiness` or `initialization_options` restarts it (shutdown, then launch).
    A change under `settings` only sends `workspace/didChangeConfiguration`. Disabling stops it.
- **`src/router.rs`.** `workspace/configuration` returns, for each item, the value at its dotted
  `section` within the spec's `settings`, or `null`.
- **`src/launcher.rs`.** A bare `command` resolves on the login shell's `PATH` (§3.3):
  `$SHELL -l -c 'printf %s "$PATH"'`, run once and cached, with a 2 s timeout. If that fails, the process
  `PATH` is used and the failure is logged. Absolute commands are used as they are. The shell is
  injectable for tests.

## Out of scope

- Readiness behaviour and the mode switch (card 4).
- A workspace-trust UI (Q7: later).
- Multi-root.
- Downloading servers (Q2).
- `crates/fe-config/settings-template.toml` (T124 creates it; add `[lsp]` there in a later card).

## Decided (lead defaults; the user may revisit)

- Q5's "setting" is `lsp.typescript_server`, a user-layer key, not two enabled specs competing by
  order.
- The login-shell `PATH` lookup has a 2 s timeout and falls back to the process `PATH`.

## Implementation notes

- **`open` and `open_on`.** `LspManager::open(root, buffer, …, path, language)` now picks the server
  and returns `Option<ServerId>`. Card 2's explicit-server form is kept as `open_on(server, …)`, so
  `src/tests/sync.rs` (outside the listed files) calls it by that name at three call sites; nothing
  else there changed. `server_of(buffer)` says where a document is now.
- **Documents across restarts.** A restarted or stopped server's documents go to the server they'd get
  if opened now, or wait. Where each routed document is lives in one table shared with the server
  threads (`server::Routes`, with the waiting documents' openings), which the manager updates before
  stopping a server, and `server_of`/`changed` read. An ending server, however it ends (a panic
  too, through drop guards on the thread's command receiver and its actor, and the end of its
  commands, which retiring it brings: the places held its other senders), closes its
  command channel, then forwards each document (open or still queued) to its current place: a
  forward racing a second restart either lands in time to be forwarded again or fails and waits. It
  takes commands until the channel has ended, not just until it reads empty: futures' channel counts
  a send before queueing it, so an open counted before the close may still be landing. Any
  opening found waiting when a place is set to a server goes there. This needed two small methods in
  `src/document.rs` (outside the listed files): `Document::into_opening`, which hands a document on
  at the text drained so far (its subscription keeps what came after), and `Opening::catch_up`,
  which keeps a waiting document's text current on `changed` instead of queueing events.
- **Shutdown, then launch.** A new instance of the same name and root waits, on its own thread, for
  the old instance's thread to end before it launches, and that thread ends only once the process
  has gone: after `exit`, or after a kill, once `ServerProcess::exited` says so. One stopped before
  it launches (while it waits, or after) launches nothing, and still waits for the one before it;
  one stopped while it launches is killed before anything is sent to it, rather than waiting for
  an `initialize` answer. So changes made back to back launch only the last spec, or kill the ones
  launched meanwhile at once.
  A server found gone is waited for the same way, as its process may still be being killed, and a
  local process dropped without having exited (a panic) takes its process group with it.
  A restart with no documents launches the server again, as it was running.
- **Waiting documents.** A document with no server (none for its language, `[lsp]` turning it off,
  or a server that failed to launch or crashed) waits, keeping the size limit it was opened with, and
  gets one at the next `update_settings` or `register_spec`, or when another document starts its
  server. A placement pass that started an instance ends by putting the documents still waiting on
  the instances running now (starting none), so one that came back from an instance that failed
  while others were placed doesn't wait beside its replacement. A document
  stays on a server that keeps running. A request on a document still on its way to its new server
  answers "not open". `LspManager::stop` lets go of a server's documents, routed ones too, as if
  closed.
- **Settings sections.** `lsp.servers.<name>.settings` is the whole `workspace/configuration` tree:
  rust-analyzer's section is `[lsp.servers.rust-analyzer.settings.rust-analyzer]`, pyright's
  `…settings.python`. §3.3's example puts `cargo.features` directly under `settings`, but pyright and
  typescript-go ask for several sections, so one rule for every server seemed better.
- **Server tables are checked by the schema too.** fe-config's checker doesn't descend into a schema
  map (`additionalProperties`), so each layer is checked against the schema with its own server
  names listed: `command = ""` is an error, a misspelled key a warning.
- **Trust filter.** The project layer is narrowed before it's type-checked, so an invalid value in a
  key that's ignored anyway (`typescript_server = "deno"`) yields the warning, not an error that would
  reject the project's other settings and its opt-out. `lsp.enabled` or `….enabled` set to anything
  but `false` is dropped with the warning too.
- **Login shell.** Every local launch (absolute commands too) gets the login shell's `PATH` as its
  `PATH` unless the spec's `env` sets one, since servers run other programs (`#!/usr/bin/env node`).
  The shell runs `printf "%s\n" "$PATH" >> "$FASTEDIT_LOGIN_PATH_FILE"` rather than the launcher
  bullet's bare `printf %s "$PATH"`: the `PATH` goes to a new file of fastedit's (mode 600, randomly
  named, in the temporary directory or else `/tmp`, removed afterwards), not the shell's output, so
  nothing a profile or logout script prints can pass for it, and the final line break shows the
  whole `PATH` was written. It's used as written, relative-only or empty too. A shell that ends
  without writing it counts as failed. The shell runs on a thread of its own, so the 2 s hold even
  when it can't be started or reaped (a hung file system); an unset or relative `SHELL` means
  `/bin/sh`, which is logged. A bare command (one with no `/`, as `execvp` has it) is looked up on
  one `PATH`, in order: the spec's if set, else the login shell's, or fastedit's own only if the
  shell failed. Relative and empty entries (an empty `PATH` is one) are directories under the
  workspace root, the server's working directory, as in a shell started there, and so is a relative
  command. A leading `~` in a command or an entry is the home directory, as bash has it (other
  shells don't expand it there, but then such an entry finds nothing), never a `~` directory in the
  workspace. As in a shell, a file there that fastedit may not run (`faccessat(X_OK, AT_EACCESS)`: an
  execute bit only for others, an ACL denying it) is passed over for the next. rustix's `fs` module
  comes with fe-lsp's async-io and async-process, so Cargo.toml doesn't change. Not found, the launch
  fails as "<command> not found". A relative entry or command can name a file in the workspace, as
  it would in a terminal; only user settings and the host choose them (Q7), and workspace trust may
  narrow that later.
- **Out of the card:** `timeouts` are only settings until card 4 applies them; `max_file_bytes`
  applies to documents opened after it changes.
