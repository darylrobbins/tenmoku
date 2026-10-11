---
task_uid: "01m4nxtv96ahf8n9z9mv0cn8a3"
task: "T42"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test tests::lsp::lsp_defaults_and_user_overrides: Settings::default().lsp.typescript_server is TypescriptServer::TypescriptGo and serializes as \"typescript-go\", and a user file with lsp.typescript_server = \"typescript-language-server\" loads with no diagnostics and selects TypescriptLanguageServer"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test tests::settings::schema_matches_golden passes, with crates/fe-config/settings.schema.json giving the lsp default typescript_server as \"typescript-go\""
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::registry::builtin_specs: under default settings javascript, typescript and tsx go to typescript-go and typescript-language-server is not among the specs; with typescript_server = TypescriptLanguageServer they go to typescript-language-server and typescript-go is not among them"
    required: true
    manual: false
  - id: "C4"
    text: "FE_LSP_REQUIRE_SERVERS=1 scripts/lsp-real.sh passes on Linux, with real_server_typescript_go running under the default typescript_server (no override) and real_server_typescript_language_server selecting TypescriptLanguageServer explicitly; the decision record notes the exit-code-1 stop and lowercased tsconfig.json path re-checked"
    required: true
    manual: false
  - id: "C5"
    text: "docs/decisions/lsp-client.md records Q5 \"Changed (user, 2026-10-11): (b)\" with T305 parked, and §1, §3.3 (default, no fallback, pre-7 tsc) and §10 name typescript-go the default; no file outside the scope changes, apart from the task's own spec under docs/specs/ and its decision record; Cargo.toml and Cargo.lock are unchanged"
    required: true
    manual: false
---

# Make typescript-go the default TypeScript server, with typescript-language-server a setting (Mac T360)

# Make typescript-go the default TypeScript server

## Problem

Found by T148's real-server tests (2026-10-09, `docs/decisions/lsp-client.md` §10, "Which servers
version their diagnostics"): under the default `lsp.typescript_server`, typescript-language-server
6.0.1, TypeScript and JavaScript files show **no diagnostics**. It never sets `version` on
`publishDiagnostics`, and fe-lsp drops unversioned publications (§4.1). typescript-go 7.0.2 pushes
none for source files, but answers `textDocument/diagnostic`, which T306 makes fe-lsp send.

The user's decision (2026-10-11), choosing option B: "B". Once T306 lands, typescript-go is the
default TypeScript server. typescript-language-server stays a configurable alternative. T305 (pulling
typescript-language-server's diagnostics through `typescript.tsserverRequest`) is parked, not
abandoned.

This reverses Q5's recommendation and decision ((c), 2026-10-06: typescript-language-server the
default, typescript-go a switch; `lsp-client.md` §1 item 5, Q5 near line 1779).

## Change

### The default

`crates/fe-config/src/settings.rs`: `TypescriptServer`'s `#[default]` moves to `TypescriptGo`. The
variants, their kebab-case names and the `lsp.typescript_server` key are unchanged, so a user file
with `typescript_server = "typescript-language-server"` keeps working. Regenerate
`crates/fe-config/settings.schema.json` with `UPDATE_GOLDEN=1` (its `lsp` default becomes
`"typescript_server": "typescript-go"`). `settings-template.toml` has no `[lsp]` section and stays
unchanged.

`crates/fe-lsp/src/registry.rs` needs no logic change: `typescript_server()` already maps each choice
to (chosen, not chosen), and the not-chosen server never runs.

### No fallback

There is still no fallback between the two. When `tsc` isn't on the launcher's `PATH`, the server
stops with "typescript-go not found" (Q2 (a), `server.rs` `launch_error`), and navigation stays
search-based, exactly as a missing typescript-language-server does today. typescript-language-server
doesn't start in its place. A `tsc` older than TypeScript 7 (the JavaScript compiler, no `--lsp`) fails
`initialize` and goes through §3.5's crash budget to `Stopped`. Document both in §3.3, with the fixes:
install `typescript@7` (or set `lsp.servers.typescript-go.command`), or set
`lsp.typescript_server = "typescript-language-server"`.

### typescript-go quirks (§3.4, §10): no new handling

- **Exit code 1 after a clean shutdown**: already a normal stop (§3.4); `real_server.rs` checks it.
- **Lowercased `tsconfig.json` path**: it's in the one empty, unversioned push for a file that is never
  opened. fe-lsp drops it (unversioned), and T306's pulled replies belong to the request, not the
  reply's URI. Definitions and references into another file come back at the right path
  (`real_server_typescript_go`).

Re-check both in the real-server run and say so in the decision record.

### Tests

- `crates/fe-config/src/tests/lsp.rs` `lsp_defaults_and_user_overrides`: the default is
  `TypescriptGo`, serialized as `"typescript-go"`; add a user file setting
  `typescript_server = "typescript-language-server"`, which loads with no diagnostics and selects it.
- `crates/fe-lsp/src/tests/registry.rs` `builtin_specs`: default settings send javascript,
  typescript and tsx to typescript-go and leave typescript-language-server out; the explicit
  `TypescriptLanguageServer` choice does the reverse. The manager loop already covers both commands.
- `crates/fe-lsp/tests/real_server.rs` `settings()`: today it sets `typescript_server` only for
  typescript-go and relies on the default for typescript-language-server. Flip it: set
  `TypescriptLanguageServer` for typescript-language-server, and run typescript-go under the default.

### Docs

`docs/decisions/lsp-client.md`:

- Q5: add "**Changed (user, 2026-10-11): (b)**", with why (no diagnostics under
  typescript-language-server; typescript-go answers pulls once T306 lands) and that T305 is parked.
- §1: the TypeScript bullet and question 5 name typescript-go as the default and
  typescript-language-server as the alternative.
- §3.3: the built-in list says which TypeScript server runs by default, plus the no-fallback and
  old-`tsc` notes above.
- §10: the "show no diagnostics under the default server" line now refers to typescript-language-server
  by name, not as the default.

`TESTING.md` already names both servers and needs no change.

## Out of scope

Automatic fallback between the TypeScript servers, detecting a pre-7 `tsc`, installing or bundling
servers (Q2), T305's tsserver pull, and any UI change.
