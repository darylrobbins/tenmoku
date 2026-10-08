---
task_uid: "01m4bqn3q2zhw3dcan8bs2s3nf"
task: "T177"
rev: 4
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-config test workspace_trust::nearest_decision_wins (tests/workspace_trust.rs): an empty TrustStore::in_memory() gives Undecided for /a; after set(/a, true), /a and /a/b/c are Trusted and /ab and / are Undecided; after set(/a/b, false), /a/b and /a/b/c are Untrusted and /a is still Trusted; after set(/a/b/c, true), /a/b/c/d is Trusted"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test workspace_trust::persists_across_loads: TrustStore::load of a missing trust.toml in a missing directory is empty and creates nothing; after set(x, true) and set(y, false) on canonical temp folders, the directory exists with mode 0700, trust.toml has mode 0600 and parses as TOML, and a new load gives x Trusted and y Untrusted; set(x, false) then a new load gives x Untrusted; canonical() of a symlink to x is x, and trust_of of it is Trusted"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test workspace_trust::bad_files_fail_closed: for a trust.toml with a syntax error and for one with [folders] \"/a\" = 3, load gives every folder Undecided; set(/a, true) returns Err, makes /a Trusted in that store, and leaves the file byte for byte unchanged, with no other file left in its directory"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config test workspace_trust::state_dir_resolution: paths::state_dir_from gives $FASTEDIT_STATE_DIR, else $XDG_STATE_HOME/fastedit when absolute, else $HOME/.local/state/fastedit, else None; empty variables and a relative XDG_STATE_HOME count as unset; trust_path() is \u003cstate_dir\u003e/trust.toml; crates/fe-config/src/lib.rs changes by the one pub mod trust; line"
    required: true
    manual: false
  - id: "C5"
    text: "fe-lsp test trust_tests::untrusted_roots_start_nothing (src/trust_tests.rs): with set_trust(|_| false), opening a .rs and a .py document under the root launches nothing, server_of is None for both, and a document_request answers Unavailable; an edit is made to the .rs buffer; then set_trust(|_| true) launches rust-analyzer and pyright once each with that root, and the fake's replica of each document equals its buffer's current text, edit included; a document under a second root that the predicate still rejects gets no server"
    required: true
    manual: false
  - id: "C6"
    text: "fe-lsp test trust_tests::untrusting_stops_servers: with rust-analyzer running for the root and two .rs documents open, set_trust(|_| false) makes the fake see shutdown then exit, and server_of is None for both; while untrusted, update_settings with a changed rust-analyzer command, register_spec and LspManager::restart(server, root) launch nothing (launches stays 1); a crash followed at once by set_trust(|_| false) launches nothing even after the ManualClock passes T142's restart delay; set_trust(|_| true) launches it again (launches 2) and the fake sees didOpen for both documents; a manager whose set_trust was never called launches as before, and fe-lsp's existing tests pass unchanged"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test trust_tests::first_open_prompts (src/trust_tests.rs): after set_trust_file(\u003cstate\u003e/trust.toml), a workbench launched on an undecided folder has root_trusted() false and, once parked, one pending prompt whose detail names the folder's path; simulate_prompt_answer(\"Trust Folder\") emits TrustChanged(true) once, root_trusted() is true, and once parked a new TrustStore::load of the file gives the folder's canonical path Trusted; a new workbench on that folder or a subfolder shows no prompt and is trusted once parked; for another folder, \"Don't Trust\" leaves it untrusted with no TrustChanged, is written to the file, and a new workbench there shows no prompt"
    required: true
    manual: false
  - id: "C8"
    text: "fe-workbench test trust_tests::when_not_to_prompt: no prompt is ever pending, and root_trusted() is false, for: a launch with a file whose undecided parent is the root; a workbench with no root; a workbench on an undecided folder when set_trust_file was never called; and with set_trust_file, a folder under a trusted folder or reached through a symlink to one is trusted with no prompt"
    required: true
    manual: false
  - id: "C9"
    text: "fe-workbench test trust_tests::open_folder_prompts_for_the_new_root: set_root to an undecided folder A prompts for A; set_root to undecided B while that prompt shows doesn't open a second; answering \"Trust Folder\" trusts A, not B, and B is then prompted for; set_root from a trusted folder to an untrusted one emits TrustChanged(false), and back emits TrustChanged(true)"
    required: true
    manual: false
  - id: "C10"
    text: "fe-workbench test trust_tests::commands_change_trust: on a trusted root, dispatching workbench.untrust_folder emits TrustChanged(false) and, once parked, the file records the folder as untrusted; workbench.trust_folder reverses it; a second workbench window on the same root emits the same TrustChanged each time; with no root both change nothing; with set_trust_file pointing under a path whose parent is a regular file, so the write fails, workbench.trust_folder still makes root_trusted() true for the session"
    required: true
    manual: false
  - id: "C11"
    text: "Both commands are in fe_workbench::command_catalog() and listed in the File menu of crates/fe-keymap/menus/default-macos.toml with no key binding; crates/fastedit/tests/menus.rs passes unchanged with no diagnostics; crates/fe-keymap/tests/menu.rs changes only by widening its File-menu prefix assertion for the two new items (as T123 did); scripts/check-no-gpui.sh prints ok: fe-config and ok: fe-lsp; no Cargo.toml or Cargo.lock change; nothing under crates/fe-gpui, crates/fe-lsp/src/tests, crates/fe-config/src/tests or crates/fe-workbench/src/tests changes; T81's golden screenshot check passes unchanged; docs/decisions/lsp-client.md Q7 carries the revised decision"
    required: true
    manual: false
  - id: "C12"
    text: "crates/fastedit/src/main.rs calls fe_workbench::set_trust_file with fe_config::paths::trust_path() before the main window opens. The hands-on check in the running app (trust sheet on first launch, Trust Folder, relaunch, File \u003e Untrust This Folder, relaunch, trust.toml contents) moved to a separate post-landing check task for the user (user's decision, 2026-10-08), and is not part of this task"
    required: true
    manual: false
---

# Start language servers only in trusted folders: a per-folder trust prompt on first open, remembered in user state, with Trust and Untrust This Folder commands

# Start language servers only in trusted folders: a per-folder trust prompt on first open, remembered in user state

A push security review of T141 (ce5f1ab) found that `LspManager::open` starts a server for any
document opened under a root. A server can run the project's code by design: rust-analyzer runs
`build.rs` and proc macros, and `cargo check` on flycheck. So opening a hostile repository runs its
code as soon as T143 wires fe-lsp into the workbench.

**Decided (user, 2026-10-07): VS Code-style workspace trust.** This revises
`docs/decisions/lsp-client.md` Q7 from (c) to (b), without (b)'s unlocking of project-layer LSP
settings:

- The first time a workspace folder is opened, fastedit asks whether to trust it.
- An untrusted folder gets no language servers.
- The answer is saved in user state, not in the project.

T143 (one `LspManager` per window) depends on this card. This card provides the gate in fe-lsp and
the trust state in the workbench; T143 connects them.

Read first:

- `crates/fe-lsp/src/manager.rs`: `open`, `place_for`/`target` (where a routed document gets a
  server or waits in the lot), `reconcile`, `place_waiting`, `waiting_for`, and T142's
  `restart(server, root)`
- `crates/fe-config/src/paths.rs`: `config_dir_from`'s environment rules
- `crates/fe-workbench/src/workbench.rs`: `Workbench::new`, `set_root`, `prompt_open` (the
  `prompting` guard), `on_run_command`
- `crates/fe-workbench/src/open.rs`: a command registry whose handlers only name an op
- GPUI's `Window::prompt` (native alert sheet) and `TestAppContext::simulate_prompt_answer`,
  `has_pending_prompt` and `pending_prompt` for tests

## Change

### Trust decisions in user state (`crates/fe-config`, GPUI-free)

- **`src/paths.rs`.** It gains `state_dir()` / `state_dir_from(env)`, mirroring
  `config_dir_from`:
  1. `$FASTEDIT_STATE_DIR`
  2. `$XDG_STATE_HOME/fastedit` (only if absolute)
  3. `$HOME/.local/state/fastedit`

  Empty variables count as unset. `trust_path()` is `<state_dir>/trust.toml`. These are reachable
  as `fe_config::paths::…`; there is no re-export.
- **New `src/trust.rs`, declared `pub mod trust;` in `lib.rs`.** That is the only change to
  `lib.rs`.
  - `pub enum Trust { Trusted, Untrusted, Undecided }`.
  - `TrustStore` (`Clone`) maps a folder's canonical path to trusted or untrusted. It has
    `load(path)`, `in_memory()`, `trust_of(&Path) -> Trust` and `set(&Path, bool) -> io::Result<()>`.
    `pub fn canonical(&Path) -> PathBuf` is `fs::canonicalize`, or the path as given if that fails.
    `trust_of` and `set` take canonical paths and do no I/O apart from `set`'s write.
  - **Nearest decision wins.** `trust_of(p)` is the decision of the nearest of `p` and its ancestors
    that has one, compared by components (so `/ab` is not under `/a`), or `Undecided`. Trusting
    a folder trusts its subfolders, as in VS Code, and an `Untrusted` subfolder of a trusted folder
    stays untrusted.
  - **The file** is TOML: a header comment saying fastedit writes it, then `[folders]` mapping each
    path string to `true` or `false`.
    - Missing file: an empty store, and nothing is created until the first `set`.
    - `set` rewrites the whole file through a temporary file in the same directory, renamed over
      the target. It creates the directory with mode 0700 and the file with mode 0600.
    - **Bad file** (unreadable, a syntax error, or a value that isn't a bool): it loads as empty,
      so every folder is undecided, which fails closed. It is never overwritten: `set` updates the
      decision in memory and returns `Err`, leaving the file byte for byte as it was.
    - A canonical path that isn't UTF-8 can't be a TOML key. Its decision applies in memory only,
      and `set` returns `Err`.
- **Tests.** Integration test `tests/workspace_trust.rs`. It needs no registration, and
  `src/tests/mod.rs` doesn't change.

### The gate (`crates/fe-lsp/src/manager.rs`, still GPUI-free)

- **`LspManager::set_trust(&mut self, trusted: impl Fn(&Path) -> bool + Send + Sync + 'static)`.**
  This is the host's answer to whether servers may start for a workspace root.
  - It defaults to every root being trusted, which is T141's behaviour. So hosts that don't call it,
    and every existing test, are unchanged. The workbench (T143) always calls it.
  - It applies like `update_settings`, through `reconcile`.
- **Untrusted root.** A routed document (`open`) whose root the predicate rejects is kept, waiting
  in the lot, exactly as one with no server for its language:
  - nothing launches
  - `server_of` is `None`
  - requests answer `Unavailable(NOT_OPEN)`
  - its subscription is still drained at each `changed`

  `target` checks the predicate before `start_server`, and `waiting_for` skips untrusted roots.
- **Root becomes untrusted.** Each routed server running for that root is retired (shutdown, exit,
  §3.4), and its documents go back to waiting. Nothing relaunches one for that root while it stays
  untrusted: not `update_settings`, not `register_spec`, not T142's crash restart, not
  `LspManager::restart(server, root)`.
- **Root becomes trusted.** Its waiting documents are placed as `update_settings` would place them.
  Their servers start, and each gets `didOpen` at the buffer's current text.
- **Not gated:** `start` and `open_on`. These are the host naming a server explicitly. Say so in
  their docs and in the module doc.
- **Tests.** In new `src/trust_tests.rs`, registered from `manager.rs` with
  `#[cfg(test)] #[path = "trust_tests.rs"] mod trust_tests;`. They build their own manager on
  `crate::fake::FakeServer` with a `ManualClock`, because `src/tests/mod.rs`'s helpers are private
  to that module and the file doesn't change.

### The prompt and commands (`crates/fe-workbench`)

- **New `src/trust.rs`.**
  - **The store global.** `pub fn set_trust_file(cx: &mut App, path: PathBuf)` loads the store on
    the calling thread, as `main.rs` loads settings, into a GPUI global, and turns prompting on.
    Until it is called, the global is `TrustStore::in_memory()`:
    - decisions last the session
    - no prompt is ever shown
    - every folder starts undecided, so it is untrusted

    No existing test can therefore see a prompt or touch a real file.
  - **Writes** go off the main thread, one at a time, and the last decision wins: the file ends at
    the latest decisions, as T170 does for settings. A failed write is logged at error level with
    the path, and the decision still applies for the session.
- **`Workbench`.**
  - **Canonical root.** It resolves its root's canonical path off the main thread (`canonical`)
    after `new` and after each `set_root`. Until then, the root is untrusted.
  - **`pub fn root_trusted(&self) -> bool`.** It is true only when the resolved root is `Trusted`.
  - **`TrustChanged(pub bool)` event.** It is emitted when `root_trusted` changes, from:
    - resolving a new root
    - an answer
    - a command
    - a decision made in another window, observed through the global

    Re-exported from `lib.rs`.
  - **`pub fn set_root_trust(&mut self, trusted: bool, cx)`.** It records the decision for the
    root's canonical path. The commands use it, and so will T143's tests.
- **When it prompts.** Only when prompting is on, the root is resolved, its trust is `Undecided`,
  and the root was opened as a folder: a folder launch, or `set_root` (Open Folder…).
  - **Launch with a file** (`Launch.file` is `Some`; the root is the file's parent): no prompt.
    The parent's existing decision, or its nearest decided ancestor's, applies, and
    `workbench.trust_folder` trusts the parent. This keeps `fastedit some/file` (or `$EDITOR`)
    from asking about every directory.
  - **No root:** no prompt, and the commands do nothing.
- **The prompt.** It is `window.prompt(PromptLevel::Warning, …)`, a native sheet, with:
  - the message "Do you trust the authors of the files in this folder?"
  - a detail naming the folder's path, saying that trusting it lets fastedit start language
    servers there, which can run the project's code (rust-analyzer runs build scripts and proc
    macros), that it covers subfolders, and that Trust This Folder / Untrust This Folder change it
    later
  - the buttons `PromptButton::ok("Trust Folder")` and `PromptButton::cancel("Don't Trust")`

  Escape is "Don't Trust", and it is remembered.
  - **One prompt per window.** GPUI forbids re-entrant prompts. If the root changes while one
    shows, the answer applies to the folder it asked about. The new root is then prompted for if
    it is still undecided.
- **Commands.**
  - `workbench.trust_folder` ("Trust This Folder") and `workbench.untrust_folder` ("Untrust This
    Folder") are registered through an fe-command registry in `crate::KEY_CONTEXT`, as `open.rs`
    does. Their handlers name an op that `on_run_command` runs (`set_root_trust`).
    `command_catalog()` knows them.
  - They are listed in the File menu of `crates/fe-keymap/menus/default-macos.toml`, after Open
    File… and before the separator.
  - There is no key binding, so the keymap and fe-gpui's command and keymap tests don't change.
- **`workbench.rs`.** A small hunk: the trust fields, resolving and prompting in `new` and
  `set_root`, the op in `on_run_command`, and the global observer.
- **`lib.rs`.** It gets:
  - `mod trust;`
  - `trust::init` in `init`
  - the registry in `command_catalog()`
  - re-exports of `set_trust_file` and `TrustChanged`
- **Tests.** In new `src/trust_tests.rs`, registered from `trust.rs` with
  `#[cfg(test)] #[path = "trust_tests.rs"] mod tests;`, as `status_bar.rs` registers
  `status_bar_vim_tests.rs`. They build their own headless workbench over a `tempfile` root, and a
  trust file under a separate temp state dir. `src/tests/mod.rs` doesn't change.

### The host (`crates/fastedit/src/main.rs`)

Before the main window opens, call `fe_workbench::set_trust_file(cx, path)` with
`fe_config::paths::trust_path()` when that is `Some`.

### The decision record (`docs/decisions/lsp-client.md`)

- Under Q7 (§9), add "**Revised (user, 2026-10-07): (b)**" with this card's rules: prompt on first
  open of a folder, remembered per canonical folder in `<state_dir>/trust.toml`, subfolders
  inherited, and file launches not prompting. Project-layer LSP settings stay filtered even when
  trusted.
- In §3.3's "Workspace trust" bullet, point to it.

## How T143 uses this

- Create the window's `LspManager` with `set_trust(|_| false)`.
- On each `TrustChanged(trusted)`, and at once if `root_trusted()`, call
  `set_trust(move |root| trusted && root == <the workbench root passed to open>)`.
- T143's tests call `set_root_trust(true, cx)` first.

## Not in this card

- **Project-layer LSP settings in trusted folders.** Q7 (b) would lift T141's filter for trusted
  roots. It stays: the filter lives in fe-config `settings.rs` (T170 holds it), and its warning
  text ("…until workspace trust exists") is left as is. A later card can lift it.
- **A `security.workspace_trust` setting** to turn the gate off. Settings files are held by T170
  and T157; add it later if wanted.
- **A "Restricted Mode" status bar item or banner.** T143's status item can say why no server
  started.
- **Restricting anything other than language servers.** There are no tasks, terminals,
  debuggers or extensions that run project code yet.
- **Trusting a single file**, as opposed to its folder, and a dialog listing every remembered
  decision (VS Code's Manage Workspace Trust page).
- **Multi-root workspaces.**

## Decided (lead defaults; the user may revisit)

- **The file lives in a state dir** (`~/.local/state/fastedit/trust.toml` by default), not
  `~/.config/fastedit`. It is machine-local state about paths on this machine, and doesn't belong
  in synced dotfiles.
- **fe-lsp's gate defaults to every root trusted.** That keeps Q7 (c)'s embedding hosts and T141's
  tests unchanged. The standalone app is safe because the workbench, the only thing that will run
  fe-lsp, installs a closed gate (T143). The workbench side fails closed: untrusted until decided,
  and undecided on a bad trust file.
- **Escape on the prompt is a remembered "Don't Trust".**
- **A file launch never prompts**, and the rule for every root is the nearest decided ancestor.

## Out of scope files

- `crates/fe-config/src/settings.rs`, `settings.schema.json` and `settings-template.toml` (T170,
  T157)
- `crates/fe-lsp/src/lib.rs`, `registry.rs`, `launcher.rs` (T176) and `src/tests/**`
- `crates/fe-workbench/src/tests/**`, `open.rs`, `status_bar.rs`
- `crates/fe-gpui/**` and the keymap
