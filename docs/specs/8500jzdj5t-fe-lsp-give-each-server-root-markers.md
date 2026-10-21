---
task_uid: "01m5ej2y52jw25rh8500jzdj5t"
task: "T247"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::registry::root_markers::server_root_follows_markers passes: under the window root the window root; outside it the nearest Cargo.lock's folder over a member's Cargo.toml; the file's folder with no marker; no window root as outside; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::registry::root_markers::root_markers_come_from_builtins_and_settings passes: the built-ins' lists as specified, and [lsp.servers.rust-analyzer] root_markers replaces rust-analyzer's"
    required: true
    manual: false
  - id: "C3"
    text: "Routing is unchanged (open still requires the window root); settings.schema.json is regenerated and its golden test passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp -p fe-config passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp -p fe-config --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: give each server root markers (Cargo.toml, pyproject.toml, tsconfig.json, ...), settable per server, and find a file's root by them (Mac T958)

## Problem

LSP completeness track, item 10 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
"1 root per window; files outside it or with no folder get no server (manager.rs:927); no root markers").
A server is started per (name, window root) and only for files under that root
(`LspManager::open`, `crates/fe-lsp/src/manager.rs:938-955`, `is_under` `:1453`). Opening a single file, or one
outside the folder (a dependency's source, a scratch script), gets no language server. Neovim's lspconfig, Helix
and Zed give each server root markers and run it at the nearest ancestor holding one; VS Code leaves root
finding to its extensions, which do the same (rust-analyzer finds the nearest `Cargo.toml`).

`ServerSpec` (`crates/fe-lsp/src/launcher.rs:26-46`) has no root field; the built-ins are
`crates/fe-lsp/src/registry.rs:14-45`; `[lsp.servers.<name>]` settings (`LspServerSettings`,
`crates/fe-config/src/settings.rs:337`) are applied to specs in `registry.rs` (`:160`). This card adds the
markers and the pure root-finding rule; routing documents by it is the next card.

## Change

1. `crates/fe-lsp/src/launcher.rs` `ServerSpec`: `pub root_markers: Vec<String>`, file or folder names tried
   in priority order (`ServerSpec::new` leaves it empty). Doc: what it's for.
2. `crates/fe-lsp/src/registry.rs` built-ins: rust-analyzer `["Cargo.lock", "Cargo.toml"]`; pyright
   `["pyproject.toml", "setup.py", "setup.cfg", "pyrightconfig.json", "requirements.txt"]`;
   typescript-language-server and typescript-go `["tsconfig.json", "jsconfig.json", "package.json"]`. A
   `[lsp.servers.<name>] root_markers` setting replaces a spec's list (for user servers such as gopls:
   `["go.work", "go.mod"]`).
3. `crates/fe-config/src/settings.rs` `LspServerSettings`: `root_markers: Option<Vec<String>>`, with a doc
   line; `crates/fe-config/settings.schema.json` regenerated.
4. New `pub fn server_root(path: &Path, window_root: Option<&Path>, markers: &[String]) -> PathBuf` in
   `crates/fe-lsp/src/registry.rs`, exported from `lib.rs`: a file under `window_root` gets `window_root` (one
   server per window folder, as today, so a monorepo keeps one rust-analyzer); otherwise, for each marker in
   order, the nearest ancestor of the file holding it (the first marker found anywhere up the tree wins over
   later markers, so `Cargo.lock`'s workspace beats a member's `Cargo.toml`); with none, the file's own
   folder. It only reads directory entries (`Path::exists` per ancestor and marker); no listing.
5. Tests: new `crates/fe-lsp/src/tests/registry/root_markers.rs`, declared by `mod root_markers;` in
   `crates/fe-lsp/src/tests/registry.rs` (its only change), on a temp tree:
   - `server_root_follows_markers`: a file under the window root gets the window root; `/t/ws/Cargo.lock`,
     `/t/ws/crates/a/Cargo.toml` and `/t/ws/crates/a/src/lib.rs` outside it give `/t/ws`; with only the
     member's `Cargo.toml`, `/t/ws/crates/a`; a file with no marker above gives its folder; no window root
     behaves as outside.
   - `root_markers_come_from_builtins_and_settings`: the built-ins' lists are as above;
     `[lsp.servers.rust-analyzer] root_markers = ["x.toml"]` replaces rust-analyzer's.

Order: after T844, which owns `registry.rs` and maps settings to specs there; `launcher.rs` is also in T914's
scope. Linux-OK.

## Implementation notes

- `crates/fe-config/src/tests/lsp.rs` builds `LspServerSettings` with every field, so the new `root_markers` field needed `root_markers: None,` there to compile. That one initializer line is the only edit to an existing test; the scope was respecced to include the file.
- `server_root` resolves `.`/`..` lexically first, so a path climbing out of the window root isn't taken for one under it (as `is_under` does).
