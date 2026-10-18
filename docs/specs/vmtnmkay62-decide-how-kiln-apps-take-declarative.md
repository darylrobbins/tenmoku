---
task_uid: "01m55bz31axzdj95vmtnmkay62"
task: "T839"
rev: 5
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003cthis task's key\u003e-extensions-and-plugins.md exists, status proposed, with every section of the spec's draft: the two stages, the extension.toml format, discovery and install locations (user dir; workspace dir only for trusted roots), the precedence table, hot reload, runtime grammar loading (WASM via tree-sitter's wasm feature vs dylib, trade-offs, licence check), the stage-2 API surface, sandbox and capabilities, versioning, WASM vs mlua with the recommendation, out of scope, and the stage-1 and later cards"
    required: true
    manual: false
  - id: "C2"
    text: "Every file, symbol and line number the record cites exists at the base, or is corrected; the finish notes say no Zed code, extension API or docs were read"
    required: true
    manual: false
  - id: "C3"
    text: "The record ends with its open questions for the user, and the finish notes ask the USER to sign off the final record before any stage-2 card is drafted"
    required: true
    manual: false
  - id: "C4"
    text: "The record's stage-2 section states the user's 2026-10-17 security decisions as decided, each with its design: net:\u003chost\u003e and process.spawn:\u003cprogram\u003e scoped by default (host-enforced, program resolution and argument passing said); net:* and process.spawn:* allowed only behind a separate extreme warning and explicit acceptance, and marked full access; re-prompt when an update asks for new or wider capabilities; plain-words combination warnings; per-plugin host-held secrets and no inherited environment. It also has a table of VS Code extension risks (full Node access, malicious or hijacked marketplace extensions and auto-update, data exfiltration, webviews, workspace config that runs code) with this design's answer to each"
    required: true
    manual: false
---

# Decide how kiln apps take declarative extensions (stage 1) and WASM code plugins (stage 2), in a decision record the user signs off

# Decide how kiln apps take extensions and plugins, in a decision record

## Problem

fastedit has no way to add a theme, a grammar, a language server, keys or menus without
rebuilding it, and no way for anything outside the binary to run code in it. It is the code
surface of an AI-first product whose own features, and third parties, will want both.

What exists today (Mac main 92c57c57):

- **Themes.** kiln owns theme files, the bundled dark/light pair and one user file;
  `app-framework.md` §8: "There's no themes directory or picker by name: one user file and a
  mode." (`kiln/crates/kiln/src/config.rs` `UserTheme`, `kiln-core/src/theme/mod.rs`
  `ThemeData::from_file`, always laid over the bundled base.)
- **Keys and menus.** `kiln_core::KeymapStore::with_defaults` already layers several default
  layers under the user's file (`kiln-core/src/load.rs`); `kiln/src/contributions.rs`
  `shipped()`/`resolve()` re-resolve keymap and menus when commands are contributed after init;
  `MenuBar::extend` merges a layer (`kiln-core/src/menu.rs:222`).
- **Grammars** are compiled in: one Cargo feature per grammar, a `static DEFS` table and
  `&'static Language` everywhere (`crates/fe-syntax/src/language.rs`). Nothing can add one at
  runtime. tree-sitter 0.27 (Cargo.lock) has a `wasm` feature: `WasmStore::load_language`
  over `wasmtime-c-api-impl` 48.0.1 (`binding_rust/wasm_language.rs`).
- **Language servers**: `crates/fe-lsp/src/registry.rs` has built-in specs, the user's `[lsp]`
  settings over them, and host specs (`LspManager::register_spec`) that win over both.
- **Snippets**: there is no snippet engine. fe-lsp advertises `snippetSupport: false`
  (`crates/fe-lsp/src/server.rs:2038`, `completion.rs:11`).
- **Config machinery** to reuse: TOML layers with schema checks and positioned `Diagnostic`s
  (`kiln-core/src/layers.rs` `check_schema`), last-good reload, the config service's
  `ConfigSource` trait (`kiln-platform/src/config.rs`), folder trust (`kiln-core/src/trust.rs`,
  `kiln-shell/src/trust.rs`).
- The command model (`command-model.md`) already gives string IDs, serialisable args, context
  predicates, headless dispatch, `Invocation` records with an `Actor` and `Registry::observe`:
  the seam a plugin host dispatches through.

The user approved the staging on 2026-10-17: **stage 1, declarative extensions** first
(data-only packages: themes, tree-sitter grammars and queries, LSP server configs, snippets,
keymaps, menus; no code execution); **stage 2, code plugins** after this record, leaning WASM
(wasmtime's component model: Apache-2.0, sandboxed, multi-language), with mlua (MIT, Lua) as the
alternative for user scripting.

## Change

Write `docs/decisions/<this task's key>-extensions-and-plugins.md` (new; the key `rgt task show`
prints for this task) from the draft below: check every file, symbol and line it cites at the
base, fix what has moved, and tighten the prose. Don't change its decisions without saying so
in the finish notes. Commit it.

Status line: **proposed**, until the user signs it off.

**The USER signs off the final record** (it sets policy: what third-party code may run in the
app, licence rules for grammars, where packages live). The finish notes list the record's open
questions; the stage-2 cards are not drafted until the user has answered them and approved the
record.

It amends `app-framework.md` §8 ("no themes directory or picker by name") by reference; it
does not edit `app-framework.md`, which T401 holds.

Clean room as every card: read only fastedit's and kiln's code and docs, the docs of
tree-sitter, wasmtime and mlua, and VS Code's public docs on contribution points if wanted.
**Never read Zed's code, its extension API or its docs** (its crates are GPL, `CLEAN_ROOM.md`).
Licences: permissive dependencies only, MPL only unmodified, GPL never (plan, "Decisions";
`deny.toml`).

---

## Draft record

### 1. Decision

Two stages.

1. **Stage 1: declarative extensions.** A package of data files: themes, keymaps, menus,
   language-server configs, tree-sitter grammars and queries, later snippets. Nothing in a
   stage-1 extension runs code: a grammar is a WebAssembly module run by tree-sitter in a
   wasmtime sandbox (§6), and a language server is a command the user's own config could
   already name. kiln owns the package format, discovery, validation, hot reload and the
   contributions kiln understands (themes, keymaps, menus); each app owns the kinds that are
   its own (fastedit: `lsp`, `grammars`, `snippets`), declared to kiln with a JSON schema.
2. **Stage 2: code plugins**, as **WebAssembly components on wasmtime** (§7). Not built until
   the user signs off this record and its stage-2 cards.

### 2. Package format

An extension is a directory (stage 1) holding `extension.toml` at its top. The archive form
is that directory zipped, with `extension.toml` at the archive's root; an install command
unpacks it into the user extensions directory. Stage 1 reads directories only; installing
from an archive is a later card.

```toml
id = "acme.solarized"          # 2+ segments of [a-z][a-z0-9_]*, as a command-ID prefix
name = "Solarized"             # the name people see
version = "1.2.0"              # MAJOR.MINOR.PATCH, numbers only
description = "Solarized themes and Ruff for Python"
license = "MIT"                # SPDX expression; REQUIRED for every extension

[engine]
fastedit = "1.0"               # <app short name> = "<API major>.<minor>" it needs

[contributes]
themes = ["themes/solarized-dark.toml", "themes/solarized-light.toml"]
keymaps = ["keymap.toml"]
menus = ["menus.toml"]

[contributes.lsp.ruff]         # an app kind: fastedit's, checked against its schema
command = "ruff"
args = ["server"]
languages = ["python"]

[[contributes.grammars]]       # an app kind
language = "zig"
wasm = "grammars/tree-sitter-zig.wasm"
extensions = ["zig", "zon"]
highlights = "queries/zig/highlights.scm"
injections = "queries/zig/injections.scm"
license = "MIT"
```

- **id**: segments as `CommandId`'s (`[a-z][a-z0-9_]*`), at least two, so stage 2's commands
  are `<id>.<command>`. The first segment may not be a namespace the app registers commands in
  (`app`, `window`, `editor`, `vim`, `workbench`, …): kiln takes the reserved list from the
  registered commands at init.
- **engine**: one key, the app's `AppId::short`. An extension for another app is rejected.
  The app declares its extension API version (`major.minor`); a manifest needing
  `M.m` loads if `M` equals the app's major and `m` is at most its minor. Stage 1 ships API
  `1.0`; each minor adds and never removes; a major breaks.
- **contributes** paths are relative, and must resolve (symlinks followed) inside the
  extension's directory: no absolute path, no `..` out, no symlink out.
- **Kinds.** kiln knows `themes`, `keymaps` and `menus`. An app registers its own kinds by
  name with a JSON schema (`schemars`), checked through `Layer::check_schema`, and receives
  their tables as written. Any other kind is a warning and ignored. `snippets` is registered
  by fastedit only once it has a snippet engine (§9).
- **No code in stage 1.** A manifest with a `[plugin]` table is rejected with an error
  ("code plugins aren't supported by this app") rather than half-loaded.
- **Diagnostics** are kiln's config `Diagnostic`s, with file, key and line:column, shown where
  config problems are shown. An `Error` rejects the extension (keeping its last good version
  on a reload); a `Warning` drops only what it names.
- Each kind's files use the format the app already reads: a theme file is a kiln theme file
  (with the app's tables, fastedit's `editor` and `syntax`); a keymap file is a keymap layer; a
  menu file is a menu layer.

### 3. Discovery and install locations

- **User extensions** (USER decision 2026-10-17: the STATE dir, not config): `<state dir>/extensions/<any dir>/extension.toml`
  (`AppPaths::extensions_dir`, new; fastedit: `$XDG_STATE_HOME/fastedit/extensions/`, i.e. `~/.local/state/fastedit/extensions/`, and the macOS equivalent AppPaths gives; they stay out of a dotfiles repo). Each
  directory directly under it is one extension; its name needn't match the id. They load in id
  order. Two with one id: the first by directory name loads, the other is an error.
- **Workspace extensions**: `<root>/.<app short>/extensions/` (fastedit:
  `<root>/.fastedit/extensions/`), the launch root only (as the project settings file is), and
  **only once the root is trusted** through fastedit's folder trust (`TrustStore`, the prompt
  the workbench already shows on a folder's first open); an untrusted root's are listed as
  skipped. They may contribute only language support (`grammars`, `lsp`, `snippets`), never
  themes, keymaps or menus: those are the user's and the app's, as a project file's theme is
  ignored today. An id also installed for the user loads the user's copy, with a warning.
  This is a follow-up card after the stage-1 cards.
- Disabling one: a later card adds `[extensions] disabled = ["acme.solarized"]` to the user's
  settings.

### 4. Precedence

Built-ins, then extensions, then the user's own config, in every kind:

| Kind | Order, lowest first | Notes |
|---|---|---|
| Keymap | kiln's defaults, app's defaults, extensions (by id), user `keymap.toml` | An extension binding replaces an app binding with the same keys and context. An extension can't unbind (`command = ""` is a warning). Bindings to unknown commands drop with a warning. |
| Menus | app's table, then each extension's layer (by id) | Add only: items appended or placed `before`/`after` an ID. `remove`, or an item whose ID the app's table has, is a warning and dropped. |
| Themes | bundled base, the selected extension theme, user `theme.toml` | Themes are chosen by name in settings (fastedit: `[theme] dark = "acme.solarized/solarized-dark"`, `light = …`, user file only). The user's `theme.toml` lays over the selected theme of its appearance instead of the bundled base. A selected theme that's gone falls back to the bundled one with a warning. |
| LSP | built-ins, extension servers (by id), user `[lsp]` settings; host-registered specs stay on top | An extension server sharing a built-in's name is a warning and ignored. User settings apply over an extension server as over a built-in (`[lsp.servers.ruff] enabled = false`, `command = …`). A built-in that lists the same language is chosen first; the user turns it off to prefer the extension's. The project layer may still only turn servers off. |
| Grammars | built-in languages, then extension languages | An extension language whose name, extension or file name a built-in has is a warning and ignored for that clash. Query overrides for built-in languages (laid after the built-in query, as `queries/<lang>.scm` overrides are) are a later card that doesn't need §6. |

kiln's `fail_fast` (debug builds failing on the app's own config errors) never fires for
extension diagnostics: they're third-party.

### 5. Hot reload

The config service watches the extension directories as it watches the user's files:

- `ConfigSource` gains directory trees: a source names directories, each watched recursively
  (`notify`), with a periodic re-stat (the config watcher's `RECONCILE`, 3 s) because FSEvents
  can drop events silently under a flood. Any change under a root rescans it.
- A changed manifest or contributed file re-reads that extension; a version with errors keeps
  its last good version and reports the errors; a removed directory withdraws its
  contributions. Contributions re-apply as they do on a user-file reload: the keymap store's
  layers are swapped (`SharedKeymap::update`), menus reinstalled, themes reset, the app's
  kinds handed to its callback (fastedit then updates the LSP registry, restarting only servers
  whose spec changed, `registry::needs_restart`).
- Grammars: a changed grammar module loads as a new language for documents opened or
  re-detected afterwards; open documents keep the module they were parsed with until reopened.

### 6. Grammars at runtime

Compiled-in grammars can't take new ones, so extension grammars load at runtime. Options:

- **WebAssembly, through tree-sitter's own `wasm` feature** (recommended). `tree-sitter` 0.27's
  `wasm` feature (MIT) links `wasmtime-c-api-impl` 48 (Apache-2.0 WITH LLVM-exception, already
  allowed in `deny.toml`). A grammar is the `.wasm` file `tree-sitter build --wasm` makes,
  external scanner included. Pros: portable (one file for every OS and CPU), sandboxed (a
  broken or hostile grammar can't touch memory or files outside its instance), no code
  signing issue, keeps stage 1's "no native code" promise, and the same wasmtime is stage 2's
  runtime. Cons: binary size grows by wasmtime with Cranelift (the card measures it); parsing
  is slower than a native grammar and compiling a module costs time at first load (the card
  measures both against the 10k-line reparse budget and caches compiled modules under the
  cache dir if it's slow); each parser needs a `WasmStore`, so fe-syntax keeps stores per
  parse thread; tree-sitter pins the wasmtime version, so stage 2 uses the version tree-sitter
  pins, or links two (the decision for stage 2 must check).
- **Native dylibs** (`libloading`, ISC, already in Cargo.lock). Pros: native speed, no new
  runtime. Cons: arbitrary native code with the app's rights, which breaks stage 1's promise;
  one build per OS and CPU; macOS's hardened runtime refuses libraries not signed by our team
  unless library validation is turned off for the whole app; tree-sitter ABI mismatches crash
  rather than fail. **Rejected.**
- Compiled-in only, with extensions supplying queries: no new languages. Rejected as the
  answer, kept as the later query-override card.

Built-in grammars stay compiled in (startup and speed). Runtime languages join a registry
beside `DEFS`, leaked to `&'static Language` (bounded: one per loaded module version).
The module's size is capped (e.g. 16 MiB) and its load runs off the main thread.

**Licence policy (USER decision 2026-10-17, "option 3").**
- Every extension's manifest must carry an SPDX `license` expression (`MIT`, `Apache-2.0 OR MIT`,
  `GPL-3.0-only`, `LicenseRef-...`); a missing or unparsable one is a load error. The extensions list
  shows it.
- Classification: permissive (the licences `deny.toml` allows), copyleft (GPL/LGPL/AGPL/MPL/EPL/CDDL
  families...), or unknown (`LicenseRef-*`, `NOASSERTION`, anything else). An expression is permissive
  only if every branch a user could take under it is (for `OR`, at least one permissive branch suffices).
- A user's own install of ANY licence loads, since we don't distribute it. The first time a copyleft or
  unknown extension is discovered (per id and version), fastedit shows a one-time notice naming the
  extension and its licence; the acknowledgement is remembered.
- An admin setting `extensions.allowed_licenses` (a list of SPDX ids; unset = allow all) restricts loading:
  an extension whose expression the list doesn't satisfy is refused with a config diagnostic. This is the
  lever for locked-down deployments of the AI product.
- fastedit bundles, pre-installs, recommends or lists in any registry only extensions (grammars and
  queries included) whose licence `deny.toml` allows. The registry case gets a counsel review before any
  registry ships.

### 7. Stage 2: code plugins

**Runtime: wasmtime's component model.** Each plugin is a component targeting a WIT world;
each runs in its own `Store`, off the main thread, never on the paint path.

API surface, by WIT interface (`kiln:extension@1.x` for what any kiln app has,
`fastedit:editor@1.x` for the editor's):

- **Commands**: register commands `<extension id>.<name>` with title, args schema, context
  predicate and `mutates_buffer`; invoke any command through an `Invocation` with
  `actor = Host("<extension id>")` and `source = Script`, so every plugin action is recordable,
  observable and replayable as a user's is.
- **Events**: buffer opened, changed (with edit ranges and version), saved, closed; focus and
  selection changed; config reloaded. Delivered asynchronously and coalesced; a slow plugin
  gets fewer, later events, never blocks the app.
- **Decorations and gutter columns**: a plugin pushes decoration sets and gutter-column
  entries for a buffer at a version, anchored, which the host keeps and maps through edits;
  `DecorationProvider` (called every painted frame) reads the host's copy, never the plugin.
- **Buffer read and transactions**: snapshot text and ranges at a version; edits as one
  transaction with origin `Origin::Host("<extension id>")` (or `Ai { agent, session }` for an
  AI feature), applied against the version read (`Buffer::edit_from`, conflicting edits
  refused), undoable per origin; or a `ProposedChange` the user accepts.
- **Status items**: text, tooltip and a command.
- **Panels later**: a declarative view tree (list, tree, text, buttons) the host renders with
  kiln-ui; no webviews.

**Sandbox and capabilities.** Nothing ambient. The manifest lists capabilities
(`buffer.read`, `buffer.write`, `commands`, `decorations`, `status`, `fs.read:workspace`,
`fs.write:workspace`, `process.spawn:<program>`, `net:<host>`); the user grants them at install, by a prompt that
names them; files reach only preopened directories (WASI); CPU is bounded by epoch interruption,
memory by `StoreLimits`; a call that traps or overruns unloads the plugin with a diagnostic.
The product's own AI features run as plugins under the same rules, with capabilities granted
by the app.

Decided by the USER (2026-10-17); the record must state each of these as decided, with its design:

- **Network and processes are scoped by default.** `net:<host>` names each host a plugin may
  reach (e.g. `net:api.anthropic.com`), enforced by the host's HTTP/socket imports, not by the
  plugin; `process.spawn:<program>` names each program it may run (e.g. `process.spawn:rg`),
  resolved by the host (say how: PATH lookup at grant time, recorded as an absolute path, and
  what happens when it changes), with arguments passed as a vector, never through a shell.
  Language servers stay host-spawned from declarative config, so an LSP plugin needs no spawn
  capability.
- **Wildcards exist but are extreme.** `net:*` and `process.spawn:*` are allowed, but granting
  either shows a separate, extreme warning (it says plainly that the plugin can then send your
  code anywhere / run any program as you, outside the sandbox) and needs an explicit acceptance
  step of its own (not the ordinary install confirm), and the plugin is marked as having full
  access wherever plugins are listed. Scoped grants are the default and the documented norm.
- **Re-prompt on capability escalation.** An update that asks for any capability (or wider
  scope) the installed version didn't have stops and asks again before it loads; an update
  asking for nothing new applies without a prompt. Removing a capability never prompts.
- **Combination warnings.** The install prompt describes risky combinations in plain words,
  not only the list: `buffer.read` or `fs.read:*` together with any `net` grant reads "can read
  your code and send it to <hosts>"; any `process.spawn` grant reads "runs <programs> outside
  the sandbox".
- **Per-plugin secrets, no inherited environment.** A plugin gets its own secret store, held
  by the host (macOS Keychain or the platform's equivalent) and readable only by that plugin;
  plugins never see the app's environment variables, and a spawned program gets a minimal
  environment (say which variables), never the app's.

**Versioning.** WIT packages carry semver; the manifest's `engine` names the API it needs;
the host keeps each supported older minor's world through adapters, and a major can be dropped
only with a deprecation window.

**WASM or mlua.**

| | wasmtime components | mlua (Lua 5.4 or LuaJIT) |
|---|---|---|
| Sandbox | by construction: memory, files, CPU | by stripping `io`, `os`, `debug`, `load`; memory and instruction limits through hooks; LuaJIT can't bound memory well |
| Languages | Rust, C, Go, JS, Python … | Lua |
| API contract | typed WIT, semver'd, checked at link | untyped tables, by convention |
| Cost | size, compile time (cacheable), authors need a toolchain | small, instant start, edit-and-run |
| Fit | third parties and the product's own AI features in one model; shares tree-sitter's runtime | users' personal scripting, as nvim users expect |

**Recommendation: WASM components.** fastedit will run code from people the user didn't write,
including the product's own AI features, and only WASM bounds that by construction with a
typed, versioned contract. mlua stays an option for a later, separate user-scripting layer
(the user's own `init.lua` with the user's rights), if users ask for it.

### 8. Out of scope

- Any code execution in stage 1.
- A marketplace, registry, network install, updates and package signing (stage 2 decides
  signing with the registry).
- Extensions contributing settings, settings defaults, commands (stage 1 keys and menus can
  only name commands that exist), icon themes, localisation, debug adapters, panels or webviews.
- Replacing a built-in language, grammar or language server.
- Workspace extensions contributing themes, keymaps or menus.

### 9. Snippets

fastedit has no snippet engine, so stage 1 doesn't take `contributes.snippets`. Needed first:
a snippet parser (LSP snippet grammar: `$1`, `${1:default}`, `${1|a,b|}`, `$0`, variables), a
tabstop session in fe-editor on fe-buffer anchors, next/previous-tabstop commands and their
vim behaviour, then `snippetSupport: true` in fe-lsp. Then `snippets` becomes a fastedit
kind (`[[contributes.snippets]] prefix, body, description, languages`).

### 10. Cards

Stage 1 (drafted with this record): the manifest and loader (kiln-core); discovery and hot
reload (kiln-core, kiln-platform, kiln); themes; keymaps and menus; LSP servers (fe-lsp,
fe-workbench, fastedit); grammars (fe-syntax, after this record and T401). Later: workspace
extensions behind trust; disabling by setting; installing from an archive; query overrides
for built-in languages; the snippet engine. Stage 2: drafted after sign-off.

### 11. Open questions for the user

1. ~~Grammar licences~~: decided by the user, 2026-10-17: option 3 (see "Licence policy" in section 6).
2. ~~Where installed packages live~~: decided by the user, 2026-10-17: the state directory.
3. ~~Workspace extensions~~: decided by the user, 2026-10-17: allowed in stage 1 once the folder is trusted (as drafted), limited to grammars, LSP configs and snippets.

## Size

A docs card: one record of about 300 lines.

## Out of scope

Implementing any of it; editing `app-framework.md` (T401 holds it).
