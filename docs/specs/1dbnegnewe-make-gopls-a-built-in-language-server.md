---
task_uid: "01m6j99z4yt1vcs31dbnegnewe"
task: "T440"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::registry::gopls::go_file_gets_gopls_by_default: with default settings a .go document is routed to the built-in server named gopls whose command is gopls with no args, and a .rs document still gets rust-analyzer"
    required: true
    manual: false
  - id: "C2"
    text: "tests::registry::gopls::go_work_beats_go_mod: for a file in a module whose parent folder holds go.work, the server root is the go.work folder; with only go.mod it is the module folder; with neither it is the file folder"
    required: true
    manual: false
  - id: "C3"
    text: "tests::registry::gopls::settings_can_disable_or_override_gopls: [lsp.servers.gopls] enabled = false leaves a .go document with no server, and a command override replaces the built-in command while keeping languages and root markers"
    required: true
    manual: false
  - id: "C4"
    text: "tests::registry::gopls::go_language_id_is_go: the document languageId sent for a .go file is go"
    required: true
    manual: false
  - id: "C5"
    text: "tests::registry::root_markers and tests::registry::language_ids pass unchanged; cargo nextest run -p fe-lsp and cargo clippy -p fe-lsp -p fe-config --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Make gopls a built-in language server for Go: go.work then go.mod as root markers, so Go files get completion, hover, definitions and diagnostics with no config (Mac T1367)

The user (2026-11-03) asked whether Tenmoku supports the Go language server; today it doesn't by default (built-ins are rust-analyzer, pyright, typescript-language-server, typescript-go in crates/fe-lsp/src/registry.rs `builtin_specs`), though a user can add `[lsp.servers.gopls]` by hand. They asked to make gopls a built-in so Go files get completion, hover, definitions and diagnostics with no config whenever `gopls` is on the login shell's PATH.

Do:
- Add a gopls ServerSpec to `builtin_specs` (crates/fe-lsp/src/registry.rs): name "gopls", command "gopls", no args (gopls speaks LSP on stdio by default), languages ["go"], root_markers ["go.work", "go.mod"] in that priority order so a go.work workspace beats a member module (same semantics as rust-analyzer's Cargo.lock over Cargo.toml). Default readiness and diagnostics handling unless gopls needs otherwise (check gopls docs via Context7/web: its pushes carry versions; it supports pull diagnostics only behind a setting, so keep push). Empty settings.
- Go's language name in fe-syntax is "go"; check crates/fe-lsp's languageId mapping sends "go" (tests/registry/language_ids.rs) and add it if missing.
- Update the doc comments in crates/fe-config/src/settings.rs that list the built-in servers (the `servers` field on the lsp settings and LspServerSettings' docs) to name gopls, and any user docs listing built-in servers (grep docs/ for "typescript-go" and "pyright" in a list of built-ins; edit only such lists, named in scope below if found — docs/settings.md / docs/lsp.md if they exist).
- A missing gopls binary must behave like any missing built-in server today (no crash, the existing not-found handling); don't add new UI.

Tests in the new file crates/fe-lsp/src/tests/registry/gopls.rs, declared from crates/fe-lsp/src/tests/registry.rs.
Headless; Linux-OK.
