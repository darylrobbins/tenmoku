---
task_uid: "01m5gdw961a18pmtrx8mm3mr58"
task: "T272"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::open_folder::single_file::files_outside_the_folder_get_a_server_in_trusted_folders passes: with the window on /t/w and /t trusted, /t/other/src/main.rs (with /t/other/Cargo.toml) starts the fake at /t/other and hover works; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test tests::open_folder::single_file::untrusted_roots_get_no_server passes: with /t/other not inside any trusted folder, the same file starts no server and no trust prompt appears, its status item reads \"\u003cserver\u003e: folder not trusted\" with the root's path in its tooltip, and syntax highlighting still works; trusting the folder (TrustChanged) then starts the server"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit test tests::open_folder::single_file::a_window_with_no_folder_gets_servers passes: an empty window opening a file in a trusted folder's project starts a server at its marker root, and one in an untrusted location starts none"
    required: true
    manual: false
  - id: "C4"
    text: "kiln_shell::trust::decided returns the nearest decided ancestor's decision, None if none; the window root's trust and prompt behave as at the base"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench -p kiln-shell passes with no existing test edited beyond what the spec names, plus two whose assertions the change itself reverses: tests::lsp::opening_a_file_starts_its_server (a rootless window now has a manager, and its file in the trusted project gets a server at its own folder) and tests::lsp_resolve_retry_pending::a_save_during_a_failing_root_retry_opens_the_file_after_it (the saved src/main.rs, outside the replaced root in the trusted project, now gets its own launch, so the file under the root is counted across launches); cargo clippy -p fe-workbench -p kiln-shell --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Give files outside the window's folder, and windows with no folder, language servers at their project roots inside trusted folders (Mac T960)

## Problem

LSP completeness track, item 10 (user-approved 2026-10-17: "servers for files outside the folder or with no
folder"). Once the cards this depends on land, fe-lsp routes a document outside the window root to a server at
its marker root, subject to the trust predicate. The workbench never hands it one:

- with no window root there is no manager at all (`crates/fe-workbench/src/lsp.rs:805`, returns early);
- buffers are opened on the manager only if their resolved path is under the root
  (`lsp.rs:1270-1276`, `path.starts_with(root)`);
- the trust predicate is the window root exactly: `manager.set_trust(move |path| Some(path) ==
  root.as_deref())` (`lsp.rs:1057`).

Trust is the policy question here (see below). Folder trust decisions live in kiln-shell
(`kiln/crates/kiln-shell/src/trust.rs`: `decide(folder, trusted)` records a canonical folder's decision for every
window; "Its subfolders are trusted too", per fastedit's prompt, `crates/fe-workbench/src/trust.rs:27-37`), but
there is no lookup of an arbitrary path's decision.

## Change

The rule (decided): **a marker root outside the
window root gets a server only if it is inside a folder the user has trusted** (in any window, now or before).
No new prompt: an untrusted loose file gets no server, as today, and the status item says why.

1. `kiln/crates/kiln-shell/src/trust.rs`: `pub fn decided(path: &Path, cx: &App) -> Option<bool>`: the decision
   recorded for `path` or its nearest decided ancestor (canonical paths), `None` if none.
2. `crates/fe-workbench/src/lsp.rs`:
   - A manager exists in every window, root or not (the root, when there is one, is still resolved first and
     its servers start as today); windows with no root pass `None` as `open`'s root.
   - Buffers whose resolved path is outside the root (or every buffer, with no root) are opened on the
     manager too, with the same alias and ownership rules as buffers under it.
   - The trust predicate: the window root's trust as today, or else `kiln_shell::trust::decided(path) ==
     Some(true)` for the marker root; re-applied when a trust decision changes (`TrustChanged`).
   - The status item for a buffer whose marker root is untrusted reads "<server>: folder not trusted" with
     the root's path in its tooltip.
3. Tests: new `crates/fe-workbench/src/tests/lsp_single_file.rs`, declared at the end of
   `crates/fe-workbench/src/tests/open_folder.rs` as `#[path = "lsp_single_file.rs"] mod single_file;`, with
   the fake of `tests/lsp.rs` and a temp trust file:
   - `files_outside_the_folder_get_a_server_in_trusted_folders`: with the window on `/t/w` and `/t` trusted,
     opening `/t/other/src/main.rs` (with `/t/other/Cargo.toml`) starts the fake with root `/t/other`, and
     hover works there; with `/t/other` untrusted it gets none and the status item says so.
   - `untrusted_roots_get_no_server`: with `/t/other` outside every trusted folder, the same file gets no server and no prompt,
    the status item says the folder isn't trusted, and trusting it (`TrustChanged`) starts the server.
  - `a_window_with_no_folder_gets_servers`: an empty window opening a file in a trusted folder's project
     starts a server at its marker root.

Order: after the routing card (T959); `lsp.rs` is held by T844, T854, T857, T916 and T920, so this card depends on them. Linux-OK.

**Decided (user, 2026-10-18):** a file's own project root outside the window root gets a server only if that root
is in a trusted location (in any window, now or before). Otherwise the file gets syntax highlighting only: no
server, no prompt, and the status item says why. VS Code's emptyWindow trust and a trust prompt for loose files were
considered and rejected: rust-analyzer runs build scripts and proc macros.

## Implementation notes

- **Trust predicate.** fe-lsp's `set_trust` predicate gets no `App`, so kiln-shell gains, beside
  `decided(path, cx)`, `decisions(cx)`: a snapshot of the store as a `Send + Sync` closure. It also gains
  `observe(cx, f)`, which follows every decision in any window. `TrustChanged` only fires when the window
  root's trust changes, so trusting `/t/other` from a window on `/t/w` reaches the workbench through
  `observe`, which re-installs the predicate. The C2 test trusts the folder with `trust::decide`, as Trust
  This Folder in its own window would.
- **Settings gate.** While a trusted window root's settings are being read (the gate is shut), the predicate
  rejects every root, out-of-root ones too, so nothing launches with the filtered settings first. With an
  untrusted root or no root, out-of-root servers run on the filtered settings.
- **Marker root.** fe-lsp has no public way to give a document's marker root (the spec's root markers are
  private), so the predicate records the first root it's asked about. `sync_lsp` clears that record around
  each `open`, under the manager's lock. The root it records is kept on the workbench's `Document`. It
  drives the "folder not trusted" item and its tooltip, and `lsp.restart`/`lsp.show_log` now act on the
  server's own root rather than the window root's.
- **No root.** A rootless window's manager has no `LspNavigation` (it's tied to one root) and no project
  settings. A window root that fails to resolve still gets no manager, as at the base.
- **Existing tests (respec, rev 2).** Two existing tests asserted what the spec reverses:
  `tests::lsp::opening_a_file_starts_its_server` (a rootless window had no manager) and
  `tests::lsp_resolve_retry_pending::a_save_during_a_failing_root_retry_opens_the_file_after_it` (the saved
  `src/main.rs`, outside the replaced root, now gets a launch of its own, so the file under the root is
  counted across launches).
