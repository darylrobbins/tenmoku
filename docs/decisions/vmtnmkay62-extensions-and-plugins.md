# Extensions and plugins for kiln apps

**Status: accepted** (user, 2026-10-21; §12). Task T839.
Amends `app-framework.md` §8 ("There's no themes directory or picker by name")
by reference; that file isn't edited here (T401 holds it).

## Context

fastedit can't gain a theme, grammar, language server, keys or menus without a
rebuild, and nothing outside the binary can run code in it. It is the code
surface of an AI-first product whose own features, and third parties, will want
both. At the base (`b11e4ef`):

- **Themes.** kiln owns theme files, the bundled dark/light pair and one user
  file (`kiln/crates/kiln/src/config.rs:21` `UserTheme`;
  `kiln-core/src/theme/mod.rs:89` `ThemeData::from_file`), always laid over the
  bundled base. `AppPaths::themes_dir` (`kiln-core/src/paths.rs:202`,
  `<config>/themes`) exists but nothing reads it yet; this record doesn't use
  it (extension themes live in extensions, §3).
- **Keys and menus.** `KeymapStore::with_defaults` (`kiln-core/src/load.rs:124`)
  already layers default layers under the user's file;
  `kiln/src/contributions.rs` `shipped()` (:1363) and `resolve()` (:1499)
  re-resolve keymap and menus when commands are contributed after init;
  `MenuBar::extend` (`kiln-core/src/menu.rs:222`) merges a layer.
- **Grammars** are compiled in: a Cargo feature per grammar, a `const DEFS`
  table (`crates/fe-syntax/src/language.rs:231`) and `&'static Language`
  returned by `language_by_name` (:90) from a `static COMPILED` of `OnceLock`s
  (:202). Nothing adds one at runtime. tree-sitter 0.27.0 (`Cargo.lock`) has an
  off-by-default `wasm` feature over `wasmtime-c-api-impl` 48.0.1 (its
  `Cargo.toml`; `binding_rust/wasm_language.rs`). Neither is in our
  `Cargo.lock` today.
- **Language servers**: `crates/fe-lsp/src/registry.rs` has built-in specs and
  the user's `[lsp]` settings over them; host specs
  (`LspManager::register_spec`, `manager.rs:306`) win over both.
  `registry::needs_restart` (:194) says when a changed spec restarts a server.
- **Snippets**: no engine. fe-lsp advertises `snippet_support: Some(false)`
  (`crates/fe-lsp/src/server.rs:2038`; documented at `completion.rs:11`).
- **Config machinery to reuse**: TOML layers with schema checks and positioned
  `Diagnostic`s (`kiln-core/src/layers.rs:232` `Layer::check_schema`); the
  watcher with its periodic re-stat (`kiln-core/src/watch.rs`, `RECONCILE`
  :52, 3 s); the config service's `ConfigSource` trait
  (`kiln-platform/src/config.rs:67`) and `SharedKeymap::update` (:201); folder
  trust (`kiln-core/src/trust.rs:41` `TrustStore`, `kiln-shell/src/trust.rs`).
- **Command model** (`command-model.md`): string IDs, serialisable args,
  context predicates, headless dispatch, `Invocation` with an `Actor`
  (`kiln-command/src/invocation.rs:86`, incl. `Host(String)`) and a `Source`
  (`Source::Script`, :106), and `Registry::observe` (`registry.rs:322`): the
  seam a plugin host dispatches through.

The user approved the staging on 2026-10-17: stage 1 declarative extensions,
then stage 2 code plugins, leaning WASM (wasmtime components) with mlua as the
alternative for user scripting.

Clean room: this record was written from fastedit's and kiln's code and docs,
and the public documentation of tree-sitter, wasmtime and mlua. No Zed code,
extension API or docs were read.

## 1. Decision

Two stages.

1. **Stage 1: declarative extensions.** A package of data files: themes,
   keymaps, menus, language-server configs, tree-sitter grammars and queries,
   later snippets. A stage-1 extension has no general-purpose plugin API and
   loads no native code, but it is **not code-free**: a grammar is a
   WebAssembly module whose initializer and scanner run in a wasmtime sandbox
   (§6; a malformed grammar can't crash the editor),
   and an `lsp` contribution names a **program the app then spawns with
   the user's rights, outside any sandbox**. That second one is real execution
   authority. It is the same power as the user writing the `command` in their
   own `[lsp]` settings, but an installed extension is third-party, so an `lsp`
   contribution is shown at install and in the extensions list as its
   **effective invocation** (the resolved absolute executable path, the full
   argument vector, and any `env` or working directory it sets). Consent is
   bound to that whole specification: any change to the executable, its
   arguments, its environment or its working directory, through an update or a
   hot reload of the extension's files, leaves the old server running (or off)
   and asks again before the new one starts (as §7 decision 3 does for plugins).
   An interpreter as the executable (`node`, `python`, `sh`, `bash`, `deno`…) is
   flagged as able to run arbitrary code, since its arguments can be a script or
   `-e`; the prompt says so, and a changed script file the arguments name
   inside the extension's directory counts as a changed specification (consent
   records its content hash). Workspace extensions get this only after folder
   trust (§3). kiln owns the package format, discovery,
   validation, hot reload and the contributions kiln understands (themes,
   keymaps, menus); each app owns the kinds that are its own (fastedit: `lsp`,
   `grammars`, `snippets`), declared to kiln with a JSON schema.
2. **Stage 2: code plugins**, as WebAssembly components on wasmtime (§7). Not
   built until the user signs off this record and its stage-2 cards.

## 2. Package format

An extension is a directory holding `extension.toml` at its top. The archive
form is that directory zipped with `extension.toml` at the root; an install
command unpacks it into the user extensions directory. Stage 1 reads
directories only; installing from an archive is a later card.

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

- **id**: segments as `CommandId`'s (`[a-z][a-z0-9_]*`), at least two, so
  stage 2's commands are `<id>.<command>`. The first segment may not be a
  namespace the app registers commands in (`app`, `window`, `editor`, `vim`,
  `workbench`, …): kiln takes the reserved list from the registered commands at
  init.
- **engine**: one key, the app's `AppId::short` (`kiln-core/src/paths.rs:35`).
  An extension for another app is rejected. The app declares its extension API
  version (`major.minor`); a manifest needing `M.m` loads if `M` equals the
  app's major and `m` is at most its minor. Stage 1 ships API `1.0`; each minor
  adds and never removes; a major breaks.
- **contributes** paths are relative and must resolve (symlinks followed)
  inside the extension's directory: no absolute path, no `..` out, no symlink
  out.
- **Kinds.** kiln knows `themes`, `keymaps` and `menus`. An app registers its
  own kinds by name with a JSON schema (`schemars`), checked through
  `Layer::check_schema`, and receives their tables as written. Any other kind
  is a warning and ignored. `snippets` is registered by fastedit only once it
  has a snippet engine (§9).
- **No plugin code in stage 1.** A manifest with a `[plugin]` table is rejected with
  an error ("code plugins aren't supported by this app") rather than
  half-loaded.
- **Diagnostics** are kiln's config `Diagnostic`s, with file, key and
  line:column, shown where config problems are shown. An `Error` rejects the
  extension (keeping its last good version on a reload); a `Warning` drops only
  what it names.
- Each kind's files use the format the app already reads: a theme file is a
  kiln theme file (with the app's `editor` and `syntax` tables), a keymap file
  a keymap layer, a menu file a menu layer.

## 3. Discovery and install locations

- **User extensions** (user decision, 2026-10-17: the state dir, not config):
  `<state dir>/extensions/<any dir>/extension.toml`, from a new
  `AppPaths::extensions_dir` beside `state_dir` (`paths.rs:183`). For fastedit
  that is `$XDG_STATE_HOME/fastedit/extensions/`
  (`~/.local/state/fastedit/extensions/`) and whatever `AppPaths` gives on
  macOS; so they stay out of a dotfiles repo. Each directory directly under it
  is one extension; its name needn't match the id. They load in id order. Two
  with one id: the first by directory name loads, the other is an error.
- **Workspace extensions** (user decision, 2026-10-17): `<root>/.<app short>/
  extensions/` (fastedit: `<root>/.fastedit/extensions/`; `AppPaths::project_dir`,
  `paths.rs:218`), the launch root only (as the project settings file is), and
  **only once the root is trusted** through folder trust (`TrustStore`, the
  prompt the workbench already shows on a folder's first open). An untrusted
  root's are listed as skipped. They may contribute only language support
  (`grammars`, `lsp`, `snippets`), never themes, keymaps or menus: those are
  the user's and the app's, as a project file's theme is ignored today. An id
  also installed for the user loads the user's copy, with a warning. A
  follow-up card after the stage-1 cards.
- **Disabling** one: a later card adds `[extensions] disabled =
  ["acme.solarized"]` to the user's settings.

## 4. Precedence

Built-ins, then extensions, then the user's own config, in every kind.

| Kind | Order, lowest first | Notes |
|---|---|---|
| Keymap | kiln's defaults, app's defaults, extensions (by id), user `keymap.toml` | An extension binding replaces an app binding with the same keys and context. An extension can't unbind (`command = ""` is a warning). Bindings to unknown commands drop with a warning. |
| Menus | app's table, then each extension's layer (by id) | Add only: items appended or placed `before`/`after` an ID. `remove`, or an item whose ID the app's table has, is a warning and dropped (`MenuBar::extend` allows both for a host's layer; extension layers get a stricter merge). |
| Themes | bundled base, the selected extension theme, user `theme.toml` | Chosen by name in settings (fastedit: `[theme] dark = "acme.solarized/solarized-dark"`, `light = …`, user file only). The user's `theme.toml` lays over the selected theme of its appearance instead of the bundled base. A selected theme that's gone falls back to the bundled one with a warning. |
| LSP | built-ins, extension servers (by id), user `[lsp]` settings; host-registered specs stay on top | An extension server sharing a built-in's name is a warning and ignored. User settings apply over an extension server as over a built-in (`[lsp.servers.ruff] enabled = false`, `command = …`). A built-in that lists the same language is chosen first; the user turns it off to prefer the extension's. A project's `[lsp]` table is unchanged from the base: filtered to turning servers off while the root is untrusted, applied unfiltered over the user's once it is trusted (`fe-config/src/settings.rs:518` `trusted_lsp`, `fe-workbench/src/lsp.rs`), so a trusted project's layer sits on top of the extension and user layers. This record doesn't change that. |
| Grammars | built-in languages, then extension languages | An extension language whose name, extension or file name a built-in has is a warning and ignored for that clash. Query overrides for built-in languages (laid after the built-in query, as fe-syntax's own `queries/<lang>.scm` overrides are, `language.rs:213`) are a later card that doesn't need §6. |

kiln's `fail_fast` (debug builds failing on the app's own config errors, see
`Config::check_fail_fast`, `contributions.rs:1507`) never fires for extension
diagnostics: they're third-party.

## 5. Hot reload

The config service watches the extension directories as it watches the user's
files:

- `ConfigSource` (`kiln-platform/src/config.rs:67`) gains directory trees: a
  source names directories, each watched recursively (`notify`), with the
  periodic re-stat (`RECONCILE`, 3 s, `kiln-core/src/watch.rs:52`) because
  FSEvents can drop events silently under a flood. Any change under a root
  rescans it.
- A changed manifest or contributed file re-reads that extension; a version
  with errors keeps its last good version and reports the errors; a removed
  directory withdraws its contributions. Contributions re-apply as they do on a
  user-file reload: the keymap store's layers are swapped
  (`SharedKeymap::update`, `config.rs:201`), menus reinstalled, themes reset,
  the app's kinds handed to its callback (fastedit then updates the LSP
  registry, restarting only servers whose spec changed, `needs_restart`).
- Grammars: a changed grammar module loads as a new language for documents
  opened or re-detected afterwards; open documents keep the module they were
  parsed with until reopened.

## 6. Grammars at runtime

Compiled-in grammars can't take new ones, so extension grammars load at
runtime. Options:

- **WebAssembly, through tree-sitter's own `wasm` feature** (recommended).
  `tree-sitter` 0.27's `wasm` feature (MIT) links `wasmtime-c-api-impl` 48.0.1
  (Apache-2.0 WITH LLVM-exception, already allowed in `deny.toml`). A grammar is
  the `.wasm` file `tree-sitter build --wasm` makes, external scanner included.
  Pros: portable (one file for every OS and CPU); sandboxed (a broken or
  hostile grammar's Wasm code can't touch memory or files outside its
  instance; see the narrowed claim below for its tables); no code
  signing issue; keeps stage 1's "no native code" promise (the module still executes, sandboxed); the same wasmtime is
  stage 2's runtime. Cons: binary size grows by wasmtime with Cranelift (the
  card measures it); parsing is slower than a native grammar and compiling a
  module costs time at first load (the card measures both against the 10k-line
  reparse budget and caches compiled modules under the cache dir if slow); each
  parser needs a `WasmStore`, so fe-syntax keeps stores per parse thread;
  tree-sitter pins the wasmtime version, so stage 2 uses that version or links
  two (stage 2's cards must check).
- **Native dylibs** (`libloading`, ISC, already in `Cargo.lock`). Pros: native
  speed, no new runtime. Cons: arbitrary native code with the app's rights,
  which breaks stage 1's promise; one build per OS and CPU; macOS's hardened
  runtime refuses libraries not signed by our team unless library validation is
  turned off for the whole app; tree-sitter ABI mismatches crash rather than
  fail. **Rejected.**
- **Compiled-in only, extensions supply queries**: no new languages. Rejected
  as the answer, kept as the later query-override card.

Built-in grammars stay compiled in (startup and speed). Runtime languages join
a registry beside `DEFS`, leaked to `&'static Language` (bounded: one per loaded
module version). A module's size is capped (e.g. 16 MiB) and its load runs off
the main thread. Size is not a time limit: a hostile initializer or scanner can
loop forever, and tree-sitter's progress callback cannot interrupt a scanner
that never returns. **Prerequisite of the grammar card:** an enforceable
execution deadline for module instantiation and parsing, with termination and a
diagnostic on overrun (wasmtime epoch interruption or fuel on the engine
tree-sitter's `WasmStore` is built with, which the card must find a way to set;
if tree-sitter's API gives no such hook, the grammar card is blocked until
that is solved, e.g. running parses in a killable helper process). Epoch limits
in §7 are the stage-2 plugins' and are not a substitute.

**Met by T845 and T230.** T845 vendors tree-sitter 0.27 (`vendor/tree-sitter`,
README there) with a patch that does three things. Each store gets an epoch
deadline, so loads and parses stop at their deadline or the caller's budget.
Failed calls into a module become errors instead of aborts. Host callbacks and
returned tokens are checked. T845 left the grammar's static tables trusted (user
decision, 2026-10-19): the parse table, actions, lex modes, alias sequences,
field and supertype maps and symbol names were copied out of the module's
memory after checking only that they lie inside it, and the native parser
indexes them unchecked. T230 (lent from T1005) validates every table at load,
for bounds and cross-references, and a corrupt one is an error from the load.
**The claim holds:** a grammar's Wasm code can't touch host memory, and a
malformed grammar can't crash the editor.

**Licence policy (user decision, 2026-10-17, "option 3").**

- Every extension's manifest must carry an SPDX `license` expression (`MIT`,
  `Apache-2.0 OR MIT`, `GPL-3.0-only`, `LicenseRef-...`); a missing or
  unparsable one is a load error. The extensions list shows it.
- Classification: permissive (the licences `deny.toml` allows), copyleft
  (GPL/LGPL/AGPL/MPL/EPL/CDDL families…), or unknown (`LicenseRef-*`,
  `NOASSERTION`, anything else). An expression is permissive only if every
  branch a user could take under it is (for `OR`, at least one permissive
  branch suffices).
- A user's own install of ANY licence loads, since we don't distribute it. The
  first time a copyleft or unknown extension is discovered (per id and
  version), fastedit shows a one-time notice naming the extension and its
  licence; the acknowledgement is remembered.
- An admin setting `extensions.allowed_licenses` (a list of SPDX ids; unset =
  allow all) restricts loading: an extension whose expression the list doesn't
  satisfy is refused with a config diagnostic. This is the lever for
  locked-down deployments of the AI product.
- fastedit bundles, pre-installs, recommends or lists in any registry only
  extensions (grammars and queries included) whose licence `deny.toml` allows.
  The registry case gets a counsel review before any registry ships.

## 7. Stage 2: code plugins

**Runtime: wasmtime's component model.** Each plugin is a component targeting a
WIT world; each runs in its own `Store`, off the main thread, never on the paint
path.

### API surface

By WIT interface (`kiln:extension@1.x` for what any kiln app has,
`fastedit:editor@1.x` for the editor's):

- **Commands**: register commands `<extension id>.<name>` with title, args
  schema, context predicate and `mutates_buffer`; invoke commands through an
  `Invocation` with `actor = Host("<extension id>")` and `source = Script`, so
  every plugin action is recordable, observable (`Registry::observe`) and
  replayable as a user's is.
  `Actor::Host` is attribution, not authorization: `Registry` checks context
  and arguments, not permissions. So the host authorizes every plugin-origin
  dispatch before it reaches the registry: each command declares the
  capabilities its effects need (`editor.insert` needs `buffer.write`; a command
  that opens a path needs `fs.read:workspace`, and so on) and its argument
  scopes (paths inside the workspace); a plugin may invoke a command only if its
  grants cover them. A command whose authority is not declared is refused to
  plugins by default. The plugin's identity is kept through delegated dispatch
  (a command that dispatches others runs them as the same `Host` actor with the
  same limits). The card for this is a prerequisite of the commands interface.
- **Events**: buffer opened, changed (with edit ranges and version), saved,
  closed; focus and selection changed; config reloaded. Delivered
  asynchronously and coalesced; a slow plugin gets fewer, later events, never
  blocks the app.
- **Decorations and gutter columns**: a plugin pushes decoration sets and
  gutter-column entries for a buffer at a version, anchored, which the host
  keeps and maps through edits; `DecorationProvider`
  (`crates/fe-editor/src/decorations.rs:118`, called every painted frame) reads
  the host's copy, never the plugin.
- **Buffer read and transactions**: snapshot text and ranges at a version;
  edits as one transaction with `Origin::Host("<extension id>")`
  (`crates/fe-buffer/src/buffer.rs:25`; or `Origin::Ai { agent, session }` for
  an AI feature), applied against the version read (`Buffer::edit_from`, :870,
  conflicting edits refused), undoable per origin; or a `ProposedChange`
  (`fe-buffer/src/proposed.rs:82`) the user accepts.
- **Status items**: text, tooltip and a command.
- **Panels later**: a declarative view tree (list, tree, text, buttons) the host
  renders with kiln-ui; no webviews.

### Sandbox and capabilities

Nothing ambient. The manifest lists capabilities (`buffer.read`,
`buffer.write`, `commands`, `decorations`, `status`, `fs.read:workspace`,
`fs.write:workspace`, `process.spawn:<program>`, `net:<host>`); the user grants
them at install, by a prompt that names them. Files reach only preopened
directories (WASI). CPU is bounded by epoch interruption, memory by
`StoreLimits`; a call that traps or overruns unloads the plugin with a
diagnostic. The product's own AI features run as plugins under the same rules,
with capabilities granted by the app.

### Security decisions (user, 2026-10-17: decided)

1. **Network and processes are scoped by default.**
   - `net:<host>` names each host a plugin may reach (e.g.
     `net:api.anthropic.com`). It is enforced by the host's HTTP and socket
     imports, not by the plugin: the plugin has no raw socket, only host imports
     that check the destination host (after DNS, against the resolved name and
     on each redirect) against the grant, and refuse otherwise.
   - `process.spawn:<program>` names each program a plugin may run (e.g.
     `process.spawn:rg`). The host resolves it: at grant time it looks the
     program up on `PATH`, and records the **absolute path** (and, for the
     prompt, its file identity) with the grant. A spawn runs only that recorded
     path, never a name looked up again. If the file at that path changes
     (different identity or content hash) or disappears, the grant is suspended
     and the user is asked again, as for an escalation (decision 3). Arguments
     are passed as a vector to `posix_spawn`/`CreateProcess`, never through a
     shell, so no argument is re-parsed by a shell. A granted program can
     itself run others (`sh -c`, `git -c core.pager=…`), so the prompt names the
     program's own power, and `sh`-like interpreters are flagged as near-wildcard.
   - Language servers stay host-spawned from declarative config, so an LSP
     plugin needs no spawn capability.
2. **Wildcards exist but are extreme.** `net:*` and `process.spawn:*` are
   allowed. Granting either shows a separate, extreme warning (it says plainly
   that the plugin can then send your code anywhere / run any program as you,
   outside the sandbox) and needs an explicit acceptance step of its own, not
   the ordinary install confirm. The plugin is marked as having **full
   access** wherever plugins are listed. Scoped grants are the default and the
   documented norm; a plugin asking for a wildcard when a scoped grant would do
   is a registry-review matter (§8).
3. **Re-prompt on capability escalation.** An update that asks for any
   capability, or a wider scope, the installed version didn't have stops and
   asks again before it loads (the old version stays loaded until the user
   answers or declines). An update asking for nothing new applies without a
   prompt. Removing a capability never prompts.
4. **Combination warnings.** The install prompt describes risky combinations in
   plain words, not only the list: `buffer.read` or `fs.read:*` together with
   any `net` grant reads "can read your code and send it to <hosts>"; any
   `process.spawn` grant reads "runs <programs> outside the sandbox".
5. **Per-plugin secrets, no inherited environment.** A plugin gets its own
   secret store, held by the host (macOS Keychain or the platform's
   equivalent), keyed by plugin id and readable only by that plugin; one plugin
   never reads another's. Plugins never see the app's environment variables. A
   spawned program gets a minimal environment, `PATH` (the app's default
   system path, not the user's), `HOME`, `LANG`/`LC_*`, `TMPDIR` and `TERM`
   only, never the app's own variables, and a secret only if the plugin passes
   it explicitly as an argument or on stdin.

### VS Code extension risks and this design's answer

| VS Code risk | This design |
|---|---|
| Extensions run in a Node host with full Node access: files, network, child processes, as the user | No ambient authority. WASM instance with preopened dirs only, host-checked `net:<host>` and `process.spawn:<program>`, CPU and memory limits. |
| Malicious or hijacked marketplace extensions, and auto-update shipping a bad version | Stage 1 has no plugin API or native code; its execution is sandboxed grammar WASM and host-spawned LSP servers whose effective invocation (executable, arguments, environment, script hash) the user is shown and re-asked about on any change (§1). Stage 2 has no auto-update that widens rights: an update asking for new or wider capabilities stops and asks (decision 3); signing and registry review are decided with the registry (§8). |
| Data exfiltration: read workspace, then send it anywhere | Network is per host and enforced by the host; `buffer.read` plus `net` shows the combination warning (decision 4); `net:*` needs the extreme warning and shows as full access (decision 2). |
| Webviews: arbitrary HTML and script in the editor | No webviews. Panels are a declarative view tree rendered by the host (§7). |
| Workspace config that runs code (tasks, debug launch, workspace-recommended extensions) | Workspace extensions only after folder trust and only for grammars, LSP and snippets (§3); a workspace can't install or grant a code plugin; stage-2 plugins install in the user dir only. |
| Extensions read the app's environment and tokens | Plugins never see the app's environment; per-plugin host-held secrets (decision 5). |

### Versioning

WIT packages carry semver; the manifest's `engine` names the API it needs; the
host keeps each supported older minor's world through adapters, and a major can
be dropped only with a deprecation window.

### WASM or mlua

| | wasmtime components | mlua (Lua 5.4 or LuaJIT) |
|---|---|---|
| Sandbox | by construction: memory, files, CPU | by stripping `io`, `os`, `debug`, `load`; memory and instruction limits through hooks; LuaJIT can't bound memory well |
| Languages | Rust, C, Go, JS, Python … | Lua |
| API contract | typed WIT, semver'd, checked at link | untyped tables, by convention |
| Cost | size, compile time (cacheable), authors need a toolchain | small, instant start, edit-and-run |
| Fit | third parties and the product's own AI features in one model; shares tree-sitter's runtime | users' personal scripting, as nvim users expect |

Licences: wasmtime Apache-2.0 WITH LLVM-exception (allowed); mlua MIT (allowed;
the Lua/LuaJIT it vendors are MIT, to be checked with `cargo deny` if adopted).

**Recommendation: WASM components.** fastedit will run code from people the
user didn't write, including the product's own AI features, and only WASM bounds
that by construction with a typed, versioned contract. mlua stays an option for
a later, separate user-scripting layer (the user's own `init.lua` with the
user's rights), if users ask for it.

## 8. Out of scope

- Any plugin API or native code loading in stage 1.
- A marketplace, registry, network install, updates and package signing (stage
  2 decides signing with the registry).
- Extensions contributing settings, settings defaults, commands (stage 1 keys
  and menus can only name commands that exist), icon themes, localisation,
  debug adapters, panels or webviews.
- Replacing a built-in language, grammar or language server.
- Workspace extensions contributing themes, keymaps or menus.

## 9. Snippets

fastedit had no snippet engine, so stage 1 didn't take `contributes.snippets`.
The engine has landed (T856, T857) and `snippets` is now a stage-1 kind. What it
needed: a snippet parser (LSP snippet grammar: `$1`, `${1:default}`,
`${1|a,b|}`, `$0`, variables), a tabstop session in fe-editor on fe-buffer
anchors, next/previous-tabstop commands and their vim behaviour, then
`snippetSupport: true` in fe-lsp. Then `snippets` becomes a fastedit kind
(`[[contributes.snippets]] prefix, body, description, languages`).

## 10. Cards

Stage 1 (drafted with this record): the manifest and loader (kiln-core);
discovery and hot reload (kiln-core, kiln-platform, kiln); themes; keymaps and
menus; LSP servers (fe-lsp, fe-workbench, fastedit); grammars (fe-syntax, after
this record and T401). Later: workspace extensions behind trust; disabling by
setting; installing from an archive; query overrides for built-in languages.
The snippet engine has landed (T856, T857); workspace extensions, disabling by setting, snippets as a contribution kind, and
the consent prompt and extensions list are being carded. Stage 2: drafted after
stage 1 is finished (user, 2026-10-21: finish stage 1 first).

## 11. Corrections to the spec's draft

Checked at the base; changes, none to a decision:

- `DEFS` is a `const`, not a `static` (`language.rs:231`); the `static` is
  `COMPILED` (:202).
- `RECONCILE` lives in `kiln-core/src/watch.rs:52`, not the config service.
- `SharedKeymap::update` is `kiln-platform/src/config.rs:201`.
- `AppPaths::themes_dir` already exists (unused), so §8 of `app-framework.md` is
  amended in practice only by the by-name selection.
- `wasmtime-c-api-impl` 48.0.1 comes from tree-sitter 0.27.0's own manifest;
  it is not in our `Cargo.lock` until the feature is enabled.

## 12. Open questions for the user

1. ~~Grammar licences~~: decided 2026-10-17, option 3 (§6).
2. ~~Where installed packages live~~: decided 2026-10-17, the state dir (§3).
3. ~~Workspace extensions~~: decided 2026-10-17, allowed once the folder is
   trusted, for grammars, LSP configs and snippets (§3).
4. ~~Network, process, wildcard, escalation, combination and secrets policy~~:
   decided 2026-10-17 (§7).
5. ~~Stage 1 isn't code-free~~: accepted 2026-10-21 (§1, §6). Any change to an `lsp` server's effective
   invocation asks again; the grammar card's execution deadline is T845's.
6. ~~Sign-off~~: approved 2026-10-21; this record is accepted and the stage-1 cards are drafted.
7. ~~WASM or mlua~~: accepted 2026-10-21: WASM components for stage 2; mlua only as a possible later
   user-scripting layer (§7).
8. **Grammar WASM cost.** Is a larger binary and slower first parse an
   acceptable price for runtime grammars, pending the card's measurements, or is
   there a size ceiling? Left open 2026-10-21 pending the grammar card's (T845) size and first-parse
   measurements.
9. **Stage-2 signing and registry.** **Deferred 2026-10-21:** decided with the registry, not before (§8).
10. **Admin `allowed_licenses`.** **Deferred 2026-10-21:** revisited with managed deployments.
