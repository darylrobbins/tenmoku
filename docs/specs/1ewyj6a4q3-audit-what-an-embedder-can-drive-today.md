---
task_uid: "01m6c8pky2rg6f0f1ewyj6a4q3"
task: "T282"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/embedder-audit.md has the sections Who embeds and how, What a host can drive today, What forces a fork today, Public surface and stability, and Proposed follow-up cards (ordered, each with title, type, size, exact files and dependencies)"
    required: true
    manual: false
  - id: "C2"
    text: "Each claim under What a host can drive today cites a file and symbol, and names the test or example that proves it or says unverified; the stability section covers every crate in crates/ and kiln/crates/ (pub items, GPUI coupling, licence, publish, proposed tier)"
    required: true
    manual: false
  - id: "C3"
    text: "git diff --stat lists only docs/embedder-audit.md and at most crates/fe-workbench/tests/embedder_command_surface.rs, which, if present, passes cargo test -p fe-workbench --test embedder_command_surface with clippy clean"
    required: true
    manual: false
---

# Audit what an embedder can drive today (commands, AI seams, editor view, workbench, panels, theming, chat, plugins), what forces a fork, and each crate's public surface and stability, with ordered follow-up cards (Mac T1286)

## Problem

Tenmoku's direction (user, 2026-10-31 and 2026-11-01): a native Apache-2.0 editor that AI tool makers embed
instead of forking VS Code. Nobody has written down what an embedder can do today, what would force them to fork,
or how stable the crates they'd depend on are. What exists is scattered:

- `docs/ai-host.md` (84 lines): the AI seams (ghost text, a streamed edit, a proposed change, marking what the AI
  touched, the undo rule) through public fe-buffer, fe-editor and fe-gpui API, with `crates/fe-gpui/examples/ai_demo.rs`
  as the reference and `crates/fe-gpui/tests/ai_demo_host.rs` pinning its host glue headless.
- kiln (`kiln/README.md`, `docs/decisions/kiln-app-layer.md`, `docs/decisions/app-framework.md`,
  `kiln/crates/kiln/examples/host/`): an app framework with menus, keys, windows, commands, a host example.
- Named commands: `docs/decisions/command-model.md`, `fe_command`'s `Invocation`/`Actor`/`Source`,
  `fe_workbench::command_catalog()` and `command_list()`; `chat.review_changes` is dispatched by an
  `Actor::Ai` (`crates/fe-workbench/src/chat_changeset.rs`).
- Extensions and plugins: `docs/decisions/vmtnmkay62-extensions-and-plugins.md`, kiln-plugin, fe-plugin.
- Every crate is `publish = false` and `LicenseRef-Proprietary` today (root `Cargo.toml`; relicensing is T1267
  and T1268, in flight on argon). Nothing is versioned for outside use.

This is the embedder's counterpart of the accessibility audit (Mac T1266, `docs/accessibility-audit.md`):
a baseline and an ordered list of follow-up cards, not code.

## Change

Write `docs/embedder-audit.md` with these sections:

1. **Who embeds and how**: the two shapes, (a) the whole workbench in a host app (a kiln app with Tenmoku's
   panes, docks and commands) and (b) the editor view alone as a component in a host's own GPUI app, and which
   one each AI-tool use case needs.
2. **What a host can drive today**, each claim with the file and symbol that provides it and the test or example
   that proves it (or "unverified"): named commands by ID with an actor (which commands, how a host invokes and
   observes them); the AI seams of `docs/ai-host.md`; the editor view as a component (`fe_gpui::EditorView`,
   `EditorTheme`, a host's own `EditorHost`); the workbench as a component (`fe_workbench::init`, `Workbench`,
   `Launch`, `set_window_opener`, `set_server_launcher`, `set_lsp_settings`, `set_trust_file`); panels and docks
   (`Dock`, `DockPanel`, can a host add its own panel?); theming and branding (themes, app name, icons, menus,
   the title bar); settings and keymaps (can a host ship defaults or lock some?); chat (`fe-chat`,
   `fe-chat-view`, `fe-chat-anthropic`: can a host plug its own model or agent?); plugins and extensions.
3. **What forces a fork today**: each place a host must patch source, with file:line (hard-coded names or
   paths, `pub(crate)` seams, globals, the bin-only `fastedit` crate, the app's fixed config and state directories).
4. **Public surface and stability**: per crate (`crates/*`, `kiln/crates/*`), what is `pub` (count of pub items
   from `lib.rs`), its dependencies on GPUI and the vendored gpui fork, licence, `publish`, and a proposed tier
   (stable for embedders / unstable / internal). Note the GPUI version coupling an embedder inherits.
5. **Proposed follow-up cards**, ordered: each with title, type, size, the exact files, and dependencies.

Cite the file and symbol for every claim. If one claim in section 2 is unpinned and a test is cheap, add at most
one test in a new file, `crates/fe-workbench/tests/embedder_command_surface.rs`, that pins it (for example: every
`command_list()` entry has a unique ID and a title, and the commands the doc names for hosts are in it). Prefer no
test when existing ones (`crates/fe-gpui/tests/ai_demo_host.rs`, `crates/fe-workbench/src/command_catalog_tests.rs`)
already pin the claims; name them instead. No production, Cargo or settings change.
