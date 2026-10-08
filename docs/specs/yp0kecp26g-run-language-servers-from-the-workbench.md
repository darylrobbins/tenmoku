---
task_uid: "01m49phezx4204rxyp0kecp26g"
task: "T143"
rev: 6
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::lsp::opening_a_file_starts_its_server: in a workbench rooted at a tempdir with a FakeLauncher installed through set_server_launcher, nothing launches until src/main.rs is opened; opening it launches rust-analyzer once with that root and the fake receives didOpen with the file's text; opening a second .rs file, and src/main.rs again in a split pane, launches nothing more and sends no second didOpen for the same buffer; a workbench with no root launches nothing"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::lsp::edits_and_saves_reach_the_server: typing in the editor reaches the fake as didChange after the next frame, without the 250 ms backstop; cmd-s reaches it as didSave after that change; closing a split copy while another view shows the buffer sends nothing; closing the last view sends didClose"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::lsp::status_item_follows_the_server: for the active editor's server, the status bar shows rust-analyzer: starting…, then rust-analyzer: indexing \u003ctitle\u003e while the fake's progress is open, no LSP item once Ready, rust-analyzer: restarting (2/5) after two crashes, and rust-analyzer: stopped: rust-analyzer not found when the launcher returns NotFound; switching to a Python editor shows pyright's state; clicking the item runs lsp.show_log"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::lsp::restart_and_show_log: lsp.restart makes the fake see shutdown, exit and then a new initialize, and starts a Stopped server again; lsp.show_log opens a tab whose text holds a stderr line and a window/logMessage line the fake sent; window/showMessage text shows in the status item until the next status change"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::lsp::settings_and_shutdown: a user settings reload with lsp.enabled = false shuts running servers down (shutdown then exit at the fake); closing the workbench window sends shutdown and exit to every running server that has finished initializing. A server still awaiting its initialize reply at quit, or a host launch blocked past the quit deadline, may outlive quit: the user accepted both on 2026-10-08, and T209 and T210 track the fix (cancel and force-stop from fe-lsp)"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::lsp::host_launcher_replaces_local: with set_server_launcher called before the workbench opens, opening a file calls that launcher; without it the workbench's manager uses LocalLauncher"
    required: true
    manual: false
  - id: "C7"
    text: "lsp.restart and lsp.show_log are registered through the command registry in the workbench context, known to fe_workbench::command_catalog(), and listed in the View menu of crates/fe-keymap/menus/default-macos.toml with no default key binding; rg 'KeyBinding::new|actions!' finds nothing in crates/fe-workbench/src/lsp.rs; crates/fastedit/tests/menus.rs passes unchanged with no diagnostics"
    required: true
    manual: false
  - id: "C8"
    text: "Events reach the UI through async_channel send_blocking into one cx.spawn loop per workbench; LspManager::changed is called at most once per frame per buffer; Cargo.lock changes only fe-workbench's dependency list (gains fe-lsp, no new packages); scripts/check-no-gpui.sh still prints ok: fe-lsp; T81's golden screenshot check passes unchanged"
    required: true
    manual: false
  - id: "C9"
    text: "Workspace trust: with the workbench root untrusted (never decided, or Don't Trust), opening a Rust file launches no server; set_root_trust(true, cx) then launches it and sends didOpen for the files already open; untrusting it stops servers starting for later opens (test in a new file wired with #[path])"
    required: true
    manual: false
---

# Run language servers from the workbench: one LspManager per window, buffer sync, a status bar item, and restart and log commands

# Run language servers from the workbench: one LspManager per window, buffer sync, a status bar item, and restart and log commands

Card 5 of `docs/decisions/lsp-client.md` §8 (GPUI). The design is:

- §2.2 "How results reach the UI thread without GPUI"
- §3.4 "Start" and "Shutdown"
- §3.5 "What the user sees"
- §4.1 "Who drains"
- §7's "Lifecycle and status" row

It builds on cards 1–4. It also builds on T107 (`Workbench::navigation()`, the nav plumbing), T125
(`StatusBar::set_item` and `StatusItem` with a click command) and T117's menu table.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q1 (a):** fastedit spawns servers by default, through `LocalLauncher`, and a host may replace the launcher.
- **Q2 (a):** nothing is installed. A missing server shows as stopped, and navigation stays by
  search.
- **Q6 (a):** servers start lazily, on the first open file of their language.
- **Q7 (c):** standalone starts servers. Project-layer LSP settings are already filtered by card 3, and
  a host may refuse through its launcher.

## Change

### `crates/fe-workbench/src/lsp.rs` (new)

- **The manager.** The workbench owns one `LspManager` for its project root. With no root, it owns
  none and nothing starts.
  - The manager is created with the host's launcher: a GPUI global, set by
    `fe_workbench::set_server_launcher(cx, Arc<dyn ServerLauncher>)` before the workbench opens.
    It defaults to `LocalLauncher`.
  - It is created with the effective `Settings.lsp`. `Workbench::reload_settings` passes changes on
    through `LspManager::update_settings`.
- **Buffers.**
  - When an editor for a file under the root first shows a buffer, call `LspManager::open` with its
    id, text, version, a fresh `Buffer::subscribe()`, path and fe-syntax language.
  - When no view of that buffer is left, call `LspManager::close`. A buffer shown in two panes is
    opened once.
  - **Nudges.** Once per frame (the workbench's deferred post-draw hook, as T78 does), call
    `LspManager::changed` for each open buffer whose version or save count moved. Saves are covered
    by the same check.
- **Events.** The sink calls `async_channel::Sender::send_blocking`, and one `cx.spawn` loop per
  workbench applies the events, exactly as `ProjectPanel` consumes `ProjectWatcher`
  (`project_panel.rs`). This card handles `Status`, `ShowMessage` and log events. It ignores
  diagnostics; card 7 handles them.
- **Status item.** It is added through `StatusBar::set_item`, on the right, for the active editor's
  server and root:
  - "rust-analyzer: starting…"
  - "rust-analyzer: indexing <title>"
  - nothing once ready
  - "rust-analyzer: restarting (2/5)"
  - "rust-analyzer: stopped: <error>"

  Its command is `lsp.show_log`. `window/showMessage` text replaces the item's text until the next
  status change, and is logged.
- **Commands.** They are registered through the registry in the workbench context, and
  `command_catalog()` knows them:
  - `lsp.restart` ("Restart Language Server") restarts the active editor's server
    (`LspManager::restart`), including from `Stopped`
  - `lsp.show_log` ("Language Server Log") opens an untitled tab (`Pane::add` with a new
    `EditorModel`) holding `LspManager::log` for that server
  - both are listed in the View menu of `crates/fe-keymap/menus/default-macos.toml`
  - there is no default key binding
- **Shutdown.** Closing the workbench window, and app quit (`cx.on_app_quit`, registered here),
  shut every server down (§3.4 sequence) before the window goes.

### Wiring

- `crates/fe-workbench/Cargo.toml`:
  - `fe-lsp.workspace = true`, so `Cargo.lock`'s fe-workbench entry changes. There are no new
    packages.
  - In dev-dependencies, `fe-lsp` with `features = ["test-support"]`, for card 1's `FakeLauncher`.
- `crates/fe-workbench/src/workbench.rs`: owns the LSP state, calls the frame hook, forwards
  settings and close. Keep it to a small hunk.
- `crates/fe-workbench/src/lib.rs`: `mod` line, re-export of `set_server_launcher`, `init` and
  `command_catalog()` entries.

### Tests

New file `crates/fe-workbench/src/tests/lsp.rs`, declared in `tests/mod.rs`. It runs headless
GPUI over a `tempfile` root, with `FakeLauncher` installed through `set_server_launcher`.

## Out of scope

- The navigation provider (card 6).
- Diagnostics (card 7).
- Hover (card 8).
- Completion (card 9).
- A toast component.
- Default key bindings for the two commands.

## Decided (lead defaults; the user may revisit)

- **No default bindings** for `lsp.restart` and `lsp.show_log` (VS Code and Zed bind neither). They
  are reachable from the View menu and the status item. If bindings are wanted, add them with
  `crates/fe-keymap/keymaps/default-macos.toml`, `crates/fe-gpui/src/command_tests.rs` and
  `crates/fe-gpui/src/keymap_tests.rs` in scope.
- **`window/showMessage` shows in the status item** until the next status change, not as a toast,
  since fastedit has no toast component yet.
- **The host's launcher** is a GPUI global set before the workbench opens, so `Workbench::new`'s
  signature doesn't change.

## Workspace trust (T177, added by the lead)

The user chose a per-folder trust prompt (push security review of T141). Create the LspManager with `set_trust(|_| false)`. At once if `Workbench::root_trusted()`, and on each `TrustChanged(trusted)`, call `set_trust(move |r| trusted && r == <workbench root>)`. Tests call `set_root_trust(true, cx)` first.

## Implementation notes

- **Settings at creation.** `Workbench::new` gets no `Settings`, so the
  effective `[lsp]` settings reach the manager through a GPUI global,
  `fe_workbench::set_lsp_settings`, set beside `set_server_launcher` before
  the workbench opens. `crates/fastedit/src/main.rs` (outside the scope
  list) gains the one line that sets it; without it `lsp.enabled = false`
  would be ignored until the first reload. Reloads update it too, for a
  manager made later (a new root).
- **`lsp-types` as a dev-dependency.** The fake's `notify` takes an
  `lsp_types::notification::Notification`, and fe-lsp doesn't re-export
  `lsp_types`, so the tests need it to send progress, `window/logMessage`,
  `window/showMessage` and rust-analyzer's `experimental/serverStatus`.
  Cargo.lock's fe-workbench entry gains `fe-lsp` and `lsp-types`; no
  package is new.
- **Shutdown.** Each manager's sink also tracks its running servers and
  reports each stop on a channel of its own. Closing a window (or a new
  root) drops its manager, which asks every server to shut down (§3.4) on
  its thread, and keeps the wait for them in an app global. GPUI waits only
  200 ms for a quit hook's future, so the `on_app_quit` hook `init`
  registers blocks in the hook itself: it drops every open workbench's
  manager, then waits for all their servers and the closed windows' to
  stop, up to 6 s: the longest a `LocalLauncher` launch blocks
  (`LOGIN_SHELL_TIMEOUT`, 2 s; fe-lsp sees a stop once the launch returns
  and kills what it launched), then fe-lsp's sequence (`shutdown` 2 s,
  `exit` 1 s, then its forced stop, `kill`), plus 1 s of margin. So a local
  server that has finished initializing is always stopped or killed before
  quit returns (`quitting_waits_for_a_held_launch`).
- **Host launchers must bound their launch time** (documented on
  `set_server_launcher`). fe-lsp can't cancel a launch, so a host launch
  still blocked at the deadline (a container starting, say) isn't stopped,
  and its server may outlive fastedit. The user accepted this limit on
  2026-10-08 (review brf7zq's F1, option a).
- **A server awaiting `initialize` at quit** isn't stopped in time either:
  fe-lsp waits for the `initialize` reply (up to its 30 s timeout) before
  shutting it down, so it may outlive quit. The user accepted this limit on
  2026-10-08 too (review yhxzsp's F2; C5, rev 6). T209 and T210 add launch
  cancellation and a force-stop to fe-lsp; both limits are documented on
  `set_server_launcher` and in lsp.rs's module docs.
- **Quit tests.** `TestAppContext::quit()` crashes in
  fe-workbench tests, so `quitting_waits_for_slow_servers` calls the hook's
  function, `lsp::shut_down_all`, directly, with one window closed and one
  open, and servers that don't answer `shutdown` (one exits on `exit`, the
  other must be killed).
- **Paths.** fe-lsp refuses a path through `..`, and the workbench keeps
  the root and files as given (`resolve_launch` keeps `..`). So the manager
  gets the root resolved with `fs::canonicalize`, and each file with its
  folder resolved (so a file not created yet works), both off the main
  thread; buffers wait for them. A file is synced only if its resolved path
  is under the resolved root. URIs sent to servers are those resolved paths
  (`paths_through_parent_folders_start_servers`: a folder launch, a file
  launch and a new root, each through `..`).
- **Per window.** Each workbench keeps the `[lsp]` settings it last
  reloaded, for a new root's manager; the global only seeds new workbenches.
  Each server's `window/showMessage` is kept apart, cleared by that server's
  next status.
- **Tests** run the manager on fe-lsp's `ManualClock` (a cfg(test) global),
  so the 250 ms backstop can't fire unless a test moves it: C2's didChange
  and didSave come from the frame's nudge.
