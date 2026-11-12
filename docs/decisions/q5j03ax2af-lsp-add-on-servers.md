# Add-on language servers: css-variables-language-server beside a language's primary server

T1645. A design card: this document is the only change. It plans how a document
is synced to its primary language server and to N *add-on* servers at once,
how each LSP feature is routed and merged across them, and the cards that make
css-variables-language-server a built-in add-on for CSS, SCSS, Less and the
languages that embed CSS.

## 1. Status

**Status: approved by the user 2026-11-12.** Approver: the user (Daryl Robbins), who on 2026-11-08
approved making css-variables-language-server a built-in server. No code
changed: `git diff --stat` against the lane's base (e0a0e43, the commit adding this card's spec on 14277d5) lists only this file. No
implementation card is filed by this card. When it lands the lead brings this
document to the user, and the cards in §8 are filed from it once it is approved.

Once approved, this supersedes "M4 has one server per language" in
[lsp-client.md §3.2](lsp-client.md#32-instances). That line is left as it is
here, because this card's scope is the new document only.

**Decision:** approved by the user 2026-11-12; the cards in §8 may be filed.

## 2. What css-variables-language-server needs

**Source read.** I read
[vunguyentuan/vscode-css-variables](https://github.com/vunguyentuan/vscode-css-variables)
at commit `65fe469ddd18deecc3ab2b0f4401490c0aea53ec` (2026-03-09, "Fix order of
variables with numeric components"), in `packages/css-variables-language-server`.
That commit is the newest one touching the package, one commit after the tag
`css-variables-language-server@2.8.4`, which is npm 2.8.4 (2026-02-10, gitHead
`d1065d4`). The only difference is the `sortText` line in `src/index.ts`. The
licence is MIT.

- **Launch.** `bin/index.js` is `#!/usr/bin/env node` and
  `require("../dist/index.js")`. `src/index.ts:30` calls
  `createConnection(ProposedFeatures.all)` and picks no transport itself;
  vscode-languageserver chooses it from argv, so `--stdio` gives stdio. Node
  must be `>=20`.
- **Runtime dependencies.** postcss, postcss-scss, postcss-less, less, sass,
  fast-glob, culori, axios, line-column, vscode-languageserver 9,
  vscode-languageserver-textdocument and vscode-uri.

### 2.1 Advertised capabilities (`src/index.ts:58-77`)

| Capability | Value | What it does |
|---|---|---|
| `textDocumentSync` | `Incremental` | — |
| `completionProvider` | `{ resolveProvider: true }`, no `triggerCharacters` | Every known variable on every request, with no prefix filtering by the server (lines 180-230). Each item has `label` `--name`, `detail` and `documentation` set to the value (hex for a colour), `kind` `Color` or `Variable`, and a `textEdit` writing `var(--name)`, or just `--name` after `(`. There is no `data` field. Resolve (234-236) returns the item unchanged. |
| `hoverProvider` | `true` | `{contents: value}`, a plain string, for the `--name` under the caret (281-304). |
| `definitionProvider` | `true` | The `Location` of the declaration, in whichever scanned file declared it (317-337). |
| `colorProvider` | `true` | `documentColor` matches `/var\((--[a-z-0-9]+)/g` (238-279). It returns a `ColorInformation` over `--name` for each variable whose resolved value culori parses as a colour. It never returns literal colours or `--x:` declarations, and misses names with capitals or `_`. `colorPresentation` (306-315) always answers `[]`. |
| `workspace.workspaceFolders.supported` | `true`, if the client supports folders | — |

It offers no references, rename, symbols, code actions, formatting, folding,
semantic tokens, inlay hints, links or diagnostics.

### 2.2 Languages and languageIds

The server filters nothing by languageId: any document it is sent is searched
for `var(--…)`. Only the files it *scans* are filtered, by `lookupFiles`, and the
parser is picked by extension: `.less` uses postcss-less, `.scss` uses
postcss-scss, anything else uses postcss (`CSSVariableManager.ts:53-65`). The VS
Code client's `documentSelector` is `cssVariables.languages`
(`vscode-css-variables/src/index.ts:58-61`). Its default
(`vscode-css-variables/package.json:66-87`) is `astro`, `svelte`, `vue`,
`vue-html`, `vue-postcss`, `scss`, `postcss`, `less`, `css`, `html`,
`javascript`, `javascriptreact`, `typescript`, `typescriptreact` and
`source.css.styled`.

Mapped to fastedit's language names, those that exist are `css`, `scss` (the
grammar pack), `less` (listed by vscode-css-language-server without a grammar),
`html`, `javascript` (sent as `javascriptreact` for `.jsx`), `typescript`,
`tsx` (sent as `typescriptreact`), `vue`, `svelte` and `astro` (the pack's
grammars). Those language ids already come out of `language_id`
(`crates/fe-lsp/src/document.rs:305`) unchanged. `postcss`, `vue-html`,
`vue-postcss` and `source.css.styled` have no fastedit language.

### 2.3 Settings

The server has exactly two settings, `CSSVariablesSettings { lookupFiles,
blacklistFolders }` (`CSSVariableManager.ts:27-51`):

- **`lookupFiles`** defaults to `**/*.less`, `**/*.scss`, `**/*.sass` and
  `**/*.css`.
- **`blacklistFolders`** defaults to `**/.cache`, `**/.DS_Store`, `**/.git`,
  `**/.hg`, `**/.next`, `**/.svn`, `**/bower_components`, `**/CVS`, `**/dist`,
  `**/node_modules`, `**/tests` and `**/tmp`.

`cssVariables.languages` and `cssVariables.trace.server` belong to the VS Code
client only.

The server reads its settings with `workspace/configuration`:
`connection.workspace.getConfiguration('cssVariables')` (line 143), with no
`scopeUri`, cached under one key for every document (line 137).

- **Configuration support.** It checks `capabilities.workspace.configuration`
  (lines 46-48). fe-lsp advertises it (`crates/fe-lsp/src/server.rs:2338`) and
  answers each item's `section` from the spec's `settings`
  (`crates/fe-lsp/src/router.rs:294`). So the spec's
  `settings = { cssVariables = { … } }` reaches it.
- **Configuration changes.** It registers for `didChangeConfiguration` (82-88).
  On a change it clears its caches, fetches its settings again and scans again
  (111-134). That matches fe-lsp, which sends `didChangeConfiguration` when only
  `settings` change (`registry.rs` `needs_restart`).
- **`initializationOptions`** are never read.

### 2.4 What it scans at startup, and after

- **When it scans.** In `onInitialized` (95-103), it takes `getWorkspaceFolders()`
  and runs `parseAndSyncVariables`, not awaited. It reads `workspaceFolders`
  only, never `rootUri`; with no folders it scans nothing. fe-lsp sends
  `workspaceFolders: [root]` (`server.rs:2294`), so the scan runs.
- **What it reads** (`CSSVariableManager.ts:162-187`). For each folder it runs
  `fastGlob(lookupFiles, { cwd, ignore: blacklistFolders, onlyFiles, absolute })`.
  Every match is read with `fs.readFileSync` and fully parsed with postcss, and
  `walkDecls` collects the `--*` declarations. Then nested `var()` references
  are resolved, at most 5 deep. `@import "http(s)://…"` in a scanned file is
  fetched with axios and parsed (84-117).
- **Cost.** The cost grows with the number of matching stylesheets under the
  root. There is no size limit, no `.gitignore`, and nothing excluded beyond
  `blacklistFolders`. That list leaves out `target`, `build`, `out`, `vendor`
  and fastedit's own `.worktrees`. Generated CSS in any of them is parsed, once
  per root.
- **After startup.** It registers no watcher. It re-reads a file only on
  `workspace/didChangeWatchedFiles` (155-177), which the VS Code client sends for
  `**/*.css`, `**/*.scss`, `**/*.sass` and `**/*.less`
  (`vscode-css-variables/src/index.ts:62-69`). It never parses open buffers'
  unsaved text. fe-lsp sends `didChangeWatchedFiles` only to a server whose
  registrations match (`crates/fe-lsp/src/watched_files.rs:1-3`), so as things
  stand **it would never hear of a change on disk**. §8 card A8 adds
  root-scoped static watches to a spec for this (below).
- **No refresh signal.** The server's index changes with no edit to the
  document being asked about: when the unawaited startup scan finishes, after a
  settings rescan, and after a watched file is re-read. It tells the client
  nothing, and LSP has no `documentColor` refresh request. So an early
  `documentColor` answers empty (or stale) and nothing prompts fastedit to ask
  again; §5 ("Asking again when a server's index changes") says how fastedit
  re-asks.
- **Its watched-file handler trusts the client.** `onDidChangeWatchedFiles`
  re-reads every path it is sent, with no check that the path is under its
  workspace folder or outside `blacklistFolders`. fe-lsp's `files_changed`
  queues each change for every session, and registrations match absolute
  paths. So a plain `**/*.css` registration would feed it stylesheets from
  another server root and from excluded build folders, which would pollute its
  one-name-one-value cache. The watch must therefore be scoped to the
  session's root and carry the same excludes as the scan.
- **Cache.** The cache is one `Map` keyed by variable name, so the last file
  scanned wins (`CacheManager.ts:30-44`). That is a quirk of the server, not
  something to fix here.

### 2.5 Root markers

The server looks for no marker; its root is whatever `workspaceFolders` says.
For a file under the window root, fastedit's root is the window root (§3.2 of
lsp-client.md), which is the folder the user means. For a stylesheet outside
it, `package.json` roots the file at its package, as vscode-css-language-server
does, so the scan covers that package rather than the file's lone folder.

### 2.6 Running beside another CSS server

Its completions are unfiltered `var(--name)` items, so beside
vscode-css-language-server (which completes the current file's custom
properties) the same name can come from both. Upstream has issue #93,
"Duplicate entries in suggestion list". §5 says when such duplicates are dropped and when both stay.

**Decision:** the built-in spec (card A9) is:

- launched as `css-variables-language-server --stdio`;
- an add-on for `css`, `scss`, `less`, `html`, `javascript`, `typescript`,
  `tsx`, `vue`, `svelte` and `astro`;
- given root markers `["package.json"]`;
- given `settings.cssVariables` with the server's own `lookupFiles` and its
  `blacklistFolders` plus `**/target`, `**/build`, `**/out`, `**/vendor` and
  `**/.worktrees`, written out in full so the server never sees a null section;
- given root-scoped static watches read from those same settings:
  `watch_files_from = "/cssVariables/lookupFiles"` and
  `watch_excludes_from = "/cssVariables/blacklistFolders"` (card A8). The
  server is told only of changes under its own root that its scan would read,
  and a user's override of either setting moves the watch with it;
- given `repoll_ms = 3000`, so shown buffers' colours catch up with its index
  (§5, "Asking again when a server's index changes").

**Open question Q1:** keep `javascript`, `typescript` and `tsx` in its
languages, as the VS Code extension's default does, or limit it to stylesheets,
HTML and the component languages (Vue, Svelte, Astro)? Its unfiltered items show
up in every completion list of a TS file, ranked after the primary's (§5), and
each TS root then runs a stylesheet scan. The default above keeps them, since
the 2026-11-08 approval names the TypeScript servers.

## 3. Spec model

- **The field.** `ServerSpec` gains `add_on: bool`, defaulting to `false`
  (primary). In settings it is `add_on = true` under `[lsp.servers.<name>]`.
  `LspServerSettings` gains `add_on: Option<bool>`, which `registry::apply`
  copies over the spec. An extension's `[contributes.lsp.<name>]` is read into
  `LspServerSettings` and through `extension_spec`, so extensions can declare
  add-ons with no extra work. The host does it by registering a spec with
  `add_on: true`. `add_on` is not `settings`, so `needs_restart` treats a change
  to it as a restart, which is what the server needs.
- **Trust boundary (lsp-client.md §3.3).**
  - `add_on` is accepted from the built-ins, the user layer, extensions and
    host specs.
  - In an untrusted root the project layer may still only narrow, with
    `enabled = false`. `add_on` there is ignored with the existing "LSP settings
    in project files apply only in trusted folders" diagnostic, like any other
    key. The filter in fe-config already drops every key except `enabled`;
    card A1 adds a test that it drops this one too. The same holds for card
    A8's `watch_files`, `watch_excludes`, `watch_files_from`,
    `watch_excludes_from` and `repoll_ms`, which A8 tests.
  - In a trusted root the project layer applies as written: a project can make
    a server an add-on, or stop it being one.
- **Primary and add-ons of a language.** With `specs = Registry::specs()` in the
  order they are chosen today (built-ins, settings-only, extensions, host), a
  document of language `L` gets:
  - **Primary:** the first spec listing `L` with `add_on == false`. This is
    `server_for` today, now skipping add-ons.
  - **Add-ons:** every spec listing `L` with `add_on == true`, in that order,
    with no limit on how many. Each is one process per root, which is the
    user's cost to choose. A spec named twice can't occur, because names are
    unique in `specs()`.
  - **Lead:** the primary *spec* if there is one, else the first add-on spec.
    The lead comes from the specs, never from which servers are running. A
    primary that is stopped, crashed out of its budget, missing, under an
    untrusted root or still starting is still the lead, and no add-on is
    promoted in its place: `lead_of` (§6.1) is then `None`, and the status item
    shows the primary's own state. "Primary-only" features (§5) go to the lead
    when it is placed and has the feature, otherwise to a placed add-on that
    has it (§5's *provider*). A language with only add-ons (`vue`, `svelte` and
    `astro` today, until they get main servers) is still served, with its
    first add-on as the lead.
- **Settings that choose servers.** `lsp.typescript_server` keeps choosing
  between the two TypeScript *primaries*; the unchosen one never runs. Add-ons
  are untouched by it. `enabled = false` on an add-on turns off just that
  add-on, and on a primary just that primary: the next primary listing the
  language takes over (as today), and the add-ons keep running. `lsp.enabled =
  false` stops everything. No `typescript_server`-style choice is added for
  add-ons, since each is on or off by itself.
- **API.** `registry::servers_for(specs, language) -> Servers<'_> { primary:
  Option<&ServerSpec>, add_ons: Vec<&ServerSpec> }`, with `Servers::lead()`.
  `server_for` becomes `servers_for(..).lead()`, so its callers keep their
  meaning.

**Decision:** an `add_on: bool` spec field and `[lsp.servers.<name>] add_on`
key, default `false`, under the same trust rules as `languages`. Any number of
add-ons per language, in registry order. The lead is the primary spec, else
the first add-on spec, whatever is running, and is never replaced by a running
add-on. `typescript_server` and `enabled` apply per server.

## 4. Document sync

Today a routed document is one `Placed { kind: Routed(Route) }`. It has one
`Route.root`, one entry in `Routes.places`, one opening in the lot while it
waits, and one `Subscribed` handed to its server's thread.

- **Route state.** `Route` keeps `window`, `path`, `uri` and `language`.
  `root: PathBuf` becomes `roots: Vec<(String, PathBuf)>`: one `server_root` per
  server of the document (lead first), each from that server's own markers.
  `reroot` recomputes all of them.
- **Places and the lot.** `Routes.places` becomes `HashMap<(BufferId, String),
  Place>` and `Routes.lot` becomes `HashMap<(BufferId, String), Opening>`, keyed
  by server name. So each server's placement, waiting, hand-back and forwarding
  is the existing per-document machinery, run per (document, server).
  `place_for`, `target`, `holds_uri`, `running_place`, `waiting_for`,
  `place_waiting` and `set_place` take the server name.
- **One holder per (URI, lead); each server placed on its own.** "A server has
  one document per URI" stays. Aliases (several buffers resolving to one file)
  must not split, with alias A on the primary and alias B on an add-on. So
  ownership is kept apart from live placement, under the **same key the
  workbench's `owners` map already uses: (lead name, path)**:
  - **The holder.** Each (URI, lead name) has one *holder*. A buffer's lead
    name is its language's configured lead (§3), placed or not. Every route
    with that lead shares it, whatever its add-ons. The holder is chosen from
    the open buffers, never from whether the lead happens to be placed:
    - the group's buffer that is placed on any server keeps it;
    - otherwise the earliest opened takes it;
    - when the holder closes, the next earliest becomes the holder on every
      server.
    Buffers in the group that aren't the holder are placed on no server and
    wait, as aliases wait today.
  - **Its servers.** The holder is placed on its *own* route: its lead and its
    own language's add-ons. Two aliases in CSS and SCSS share the lead
    vscode-css-language-server, so they are one group with one holder. If a
    CSS-only add-on existed, it would serve the holder only when the holder is
    the CSS buffer. The SCSS alias waits whatever the add-ons, so the workbench's
    one owner per (lead, path) is exactly the manager's one holder.
  - **Independent placement.** Each server of the holder's route places it on
    its own, under its own root's trust, its own crash budget and its own
    launch. A trusted add-on runs while the primary's root is untrusted, or the
    primary is missing or crashed out, and the other way round.
  - **Groups merging.** A settings change can give two groups the same lead,
    for example when a reload gives an SCSS alias's language the CSS alias's
    primary. Then the earlier-opened holder keeps the merged group. The other
    is closed on every server it was on (`didClose`) and waits, and the
    workbench's next `sync` takes owners from the manager as it does today. A
    change that splits a group gives each new group its holder by the same
    rule.
  - **Two leads meeting on one add-on.** Holders of one URI from groups with
    *different* leads are separate owners, in the workbench as in the manager.
    If both list the same add-on, that add-on keeps the one placed first, as
    `holds_uri` does today, and the other waits on that add-on only.
- **One subscription, fanned out.** The host still hands `open` one
  `fe_buffer::Subscription`. The manager can't subscribe for itself, because
  add-ons can join later on a settings reload with the host not involved.
  `Subscribed` becomes a hub with one queue (a *tap*) per server the document is
  on, plus the latest `(Text, Version)` drained:
  - Whichever thread drains (a server's actor, or `changed` for one waiting)
    drains the buffer's subscription and appends the events to every tap.
    `BufferEvent` is `Clone`, and its `Text` is a rope, so this is cheap.
  - A server joining later opens at the hub's latest text.
  - `Attached` holds a tap, and dropping it drops only that tap.
  - `detach` (on close) detaches the hub, so edits after `close` reach no
    server.
- **Versions and identities.** These are per server already. Each actor keeps
  its own `SentDocument` ring (`RING = 32`) and LSP version counter.
  `Identities` is keyed by (buffer, server): one open generation per `open`,
  with one active incarnation per server it reaches. Requests and diagnostics
  already check `active_identity(buffer, server)`.
- **`max_file_bytes`.** One limit per document, applied by each actor. A
  document over it is open on none of its servers, and every request answers
  "too large for the language server", as today.
- **Trust.** Trust is per root, so it is checked per (document, server) root.
  With a window root, every server shares it. Outside one, an add-on whose
  marker root isn't trusted waits on its own while the primary runs, and the
  other way round.
- **Restarts and crashes.** Crash budgets, logs and statuses are already per
  (name, root). A crashed or retired server hands back only its own
  (document, server) openings. They wait in the lot under that key, and the
  document's other servers keep syncing and answering. A
  `DiagnosticsCleared` is reported for that server's incarnation only.
  `restart(name, root)` restarts one server. A settings change that adds an
  add-on opens its documents at the hub's latest text, and one that removes an
  add-on sends `didClose` on that server only. The lead stays where it is, as
  today ("a document stays on a server that keeps running").
- **`open_on`.** A document the host places itself has exactly that one server,
  as today, and never gets add-ons.

**Decision:** route state per (document, server name), with one root per
server. One host subscription, fanned out to per-server taps by fe-lsp.
Per-server rings and identities, as now. One holder per (URI, lead name), the
workbench's owner key. It is chosen apart from placement, and placed on each
server of its own route independently.
Crashes, restarts, trust and settings act per server without disturbing the
document's other servers.

## 5. Feature routing

Terms used in the table:

- **Lead** is the configured primary, else the first add-on (§3), running or
  not.
- **Placed** means the document is open on that server now.
- **Provider** means the lead if it is placed and advertises the feature
  (statically or by registration), else the first placed add-on that does.
  Only that one server is asked. A primary-only feature with no provider
  answers `Ok(None)`, as a single server without the provider does today.
- **First-that-answers** asks every placed server advertising the feature, at
  once. It uses the first
  *non-empty* answer **in server order** (lead, then add-ons in order), waiting
  for earlier servers to answer, fail or time out before taking a later one's.
  That keeps it deterministic, not a race. The server it took becomes the
  owner of anything that follows.
- **Merged** asks every placed server advertising the feature, at once, each
  under its own `lsp.timeouts` entry. It shows when all have answered, failed
  or timed out. A server that fails contributes nothing, and the answer is an
  error only if every server failed. Results are concatenated in server order.
- **Dedupe** collapses two items only when accepting either would do exactly
  the same thing, which is checked on **both** items, never only on the one
  dropped. So the result doesn't depend on which server answered first. Where
  the row can't establish that, both stay. Where two items can't both be shown (one swatch or
  one link per range), the row gives a *precedence* instead: the earlier
  server's wins.
- **Owner** is the `ServerId` (and identity) of the server that produced an
  item. Every follow-up goes to it alone, and is sent only while that session
  still has the document open as it was (`request_if`), as today.

| Request / notification (fe-lsp today) | Route | Merge, dedupe, follow-ups |
|---|---|---|
| `didOpen`, `didChange`, `didSave`, `didClose` | every server of the document | Each server has its own tap, ring and version (§4). |
| `textDocument/completion` | merged, one version per list | One buffer version per merged list, every server re-asked on any `isIncomplete`, dedupe only between two final, command-free items, and per-server triggers: see **Completion merging** below the table. |
| `completionItem/resolve` | owner | `CompletionItem.server` and `from` already name it (`completion.rs:102`). Item commands (`workspace/executeCommand`) go to the owner. |
| `textDocument/hover` | merged | Contents in server order, separated by a rule, with exact duplicates dropped. The range is the first server's that gives one. |
| `textDocument/signatureHelp` | first-that-answers | Signatures and the active one aren't mergeable. Retrigger and trigger characters are each server's, and a trigger asks only the servers listing it. |
| `textDocument/definition`, `declaration`, `typeDefinition`, `implementation` | merged | Locations deduped by (URI, range), first server's order. One location still jumps; several open the picker, as now. A CSS `var(--x)` gets both the same-file declaration from vscode-css-language-server and the cross-file one from css-variables, deduped when they match. |
| `textDocument/references` | merged | Deduped by (URI, range). The pair sent with it (`document_request_pair`) is sent per server. |
| `textDocument/documentHighlight` | first-that-answers | Two servers' highlight sets would contradict each other. |
| `textDocument/documentSymbol` | provider | One outline. Merging two servers' trees would duplicate nodes. |
| `workspace/symbol` | merged across every ready session at the root | The enumeration is unchanged. `LspNavigation::workspace_symbols` (`navigation.rs:405`) already asks every server ready for the root (`servers_at`), each session once, whatever its languages, and add-ons are sessions at the root, so they are included with no change. The answers are then deduped by (name, kind, location), first server's copy kept, before the fuzzy ranking keeps the best `limit`. |
| `textDocument/prepareRename`, `textDocument/rename` | first-that-answers | `prepareRename` goes to every server advertising `renameProvider`. The first in server order to accept owns the rename, and `rename` goes to it alone. A server without `prepareProvider` counts as accepting. One edit set, never merged. |
| `textDocument/codeAction` | merged | Concatenated in server order, then ranked as now (quick fixes first, stable within rank). Dedupe only between two final, command-free actions: see **Code-action dedupe** below the table. `CodeAction.payload.server` is the owner. The lightbulb shows if any placed server has `codeActionProvider`. |
| `codeAction/resolve`, `workspace/executeCommand` (action) | owner | Already by `payload.server` (`code_action.rs:96`). |
| `textDocument/codeLens` | merged | Ordered by position, then server. Not deduped, since lenses rarely match. |
| `codeLens/resolve`, lens command | owner | `CodeLens.server` (`code_lens.rs:49`). |
| `textDocument/formatting`, `rangeFormatting` | provider | Two edit sets can't be applied together. |
| `textDocument/onTypeFormatting` | provider | Trigger characters are the provider's only (`on_type_formatting_triggers`). |
| `textDocument/foldingRange` | provider | Overlapping fold sets from two servers make nonsense folds. |
| `textDocument/selectionRange` | provider, once sent | fe-lsp doesn't send it today. When it does, one server answers. |
| `textDocument/semanticTokens/full`, `/full/delta` | provider | Legends differ, and a delta belongs to one server's last result. `workspace/semanticTokens/refresh` from a server re-asks only if it is the provider. |
| `textDocument/inlayHint` | merged | Ordered by position, then server. Not deduped: two hints at one position may carry different tooltips, edits or commands, so both show. |
| `inlayHint/resolve` | owner | `InlayHint.server` (`inlay_hint.rs:67`). A refresh re-asks only the server that sent it. |
| `textDocument/documentLink` | merged | Precedence, not dedupe: only one link can be clicked at a range, so where two servers' links have the same range the earlier server's wins. Links at different ranges are all kept. `DocumentLink` gains `server` and `from`. |
| `documentLink/resolve` | owner | Today it goes to `server_of(buffer)` (`document_link.rs:137`); it goes to the link's owner instead. |
| `textDocument/linkedEditingRange` | provider | Linked ranges must come from one server. |
| `textDocument/prepareCallHierarchy`, `prepareTypeHierarchy` | first-that-answers | `HierarchyItem` gains `server`. |
| `callHierarchy/incomingCalls`, `outgoingCalls`, `typeHierarchy/supertypes`, `subtypes` | owner | Today they take the buffer and go to its server (`hierarchy.rs:265`); they go to the item's owner instead. |
| `textDocument/documentColor` | merged | Precedence: one swatch per range, so at an exact same range the earlier server's colour wins. vscode-css-language-server colours literals and css-variables colours `var(--x)`, so in practice they don't overlap. `ColorInformation` gains `server` and `from`. |
| `textDocument/colorPresentation` | owner | Today it goes to `server_of(buffer)` (`document_color.rs:141`); it goes to the colour's owner instead. css-variables always answers `[]`, so a `var(--x)` swatch has no presentations, and the picker offers none. |
| `textDocument/publishDiagnostics` (pushed) | every server, merged by owner | Already per server: `LspEvent::Diagnostics { server, … }` and `DiagnosticOwner::Server(server)` in the editor. Nothing is deduped, as VS Code doesn't either. |
| `textDocument/diagnostic` (pulled), tsserver pulls | each server that pulls, merged by owner | Pulling is per session already (`pull.rs`). Each server is pulled separately and its list replaces only its own. |
| `workspace/didChangeConfiguration` | per server | Each server gets its own spec's `settings`, as now. |
| `workspace/didChangeWatchedFiles`, `didCreateFiles`, `didRenameFiles`, `didDeleteFiles` | per session | Unchanged: `notify_sessions` already reaches every server whose registrations or filters match. Root-scoped static watches (card A8) count as registrations, and match only paths under their own session's root, minus their excludes. Each send is followed by `LspEvent::Notified` (§5, "Asking again"). |
| `workspace/willRenameFiles` | merged | Unchanged: it already asks every matching server and concatenates (`file_operations.rs:176`). |
| `$/cancelRequest` | the server asked | Dropping a merged or first-that-answers future cancels every request still out. |

**Completion merging** (the `textDocument/completion` row):

- **One version.** Every server is asked at the same buffer version. A server
  that can't be admitted at it ("buffer changed") contributes nothing. So
  every item of the merged `Completions` is of one version and snapshot, as
  one server's items are today, and the workbench's `CompletionSession` keeps
  one snapshot per list.
- **Incomplete lists.** If any server said `isIncomplete`, typing on asks
  *every* server again at the new version and replaces the whole list. No
  server's items are kept across versions, so no main, additional or resolved
  edit ever needs rebasing. A newer request supersedes the older, as now:
  dropping its future cancels each server's request.
- **Dedupe compares two items, and both must qualify.** Two items from
  different servers collapse into one only when **each of the two** is
  *final* and command-free, and the two are *equivalent*:
  - **Final.** The item's server advertises no `resolveProvider`. With one,
    resolve may add edits whether or not the item has `data`.
  - **Command-free.** The item has no `command`. Commands are run by their
    owner, and the same payload can do different things on two servers.
  - **Equivalent.** The two are equal in label, kind, `filterText` and
    `insertTextFormat`, in the main edit (range and new text, or both insert
    and replace ranges), in `additionalTextEdits` and in commit characters.
  If either item fails any condition (a resolvable or command-bearing
  primary item beside a final add-on item, or the other way round), both
  stay, each resolving and running with its own owner.
- **Order of arrival doesn't matter.** The conditions are checked on both
  items, so they are symmetric, and merging runs once every server has
  answered, failed or timed out, over the lists in server order. Which server
  answered first never changes whether two items collapse. When they do, the
  copy kept is the earlier in server order (the primary's).
- **Labels.** Two entries left with the same label show their server's name in
  the detail. In practice css-variables advertises resolve, so its `--x`
  items never collapse, and they show beside vscode-css-language-server's.
- **Order and triggers.** The client's fuzzy filter and sort run on the
  merged list as now, ties broken by server order, so the primary's items
  come first. Trigger characters are the union, but a trigger character asks
  only the servers that list it; an invoked or typed-word completion asks
  all. Commit characters stay per item (its server's `allCommitCharacters`).

**Code-action dedupe** (the `textDocument/codeAction` row): actions are not
deduped across servers, with one exception, and **both** actions must qualify
for it.

- **When two collapse.** Each of the two has an `edit`, has no `command`, and
  comes from a server without `resolveProvider`. The two are equal in title,
  kind, `edit`, `disabled` and `isPreferred`.
- **Otherwise both show.** That covers a retained action with a command or one
  still to be resolved, whichever server it came from. Two entries left with
  the same title show their server's name after it.
- **Order of arrival doesn't matter**, as for completions. When two do
  collapse, the earlier in server order is kept.

**Merged colours and `editor.default_color_decorators`.** The built-in colour
card has landed (T337, commit 61d38ee). Today
`default_colors::source(on, mode, answered)` shows the server's swatches when
its latest answer had colours. Otherwise it shows the built-in ones in `auto`,
none in `never`, and always the built-in ones in `always`.

With add-ons, the decision is that **server colours win, range by range, and
only the lead can switch the built-in ones off in `auto`**:

- **`auto`.** If the lead answered colours, the editor shows the merged server
  colours, exactly as today for a single server. If not (the lead has no
  `colorProvider`, as for the TypeScript servers, or its answer was empty), it
  shows the built-in colours with the add-ons' colours laid over them: an
  add-on colour replaces any built-in colour whose range overlaps it. So a TSX
  file keeps its built-in `#rrggbb` swatches and gains css-variables'
  `var(--x)` ones. A CSS file shows vscode-css-language-server's literals plus
  css-variables' `var(--x)`, as now plus the variables.
- **`never`.** The merged server colours only.
- **`always`.** The built-in colours only, as today.
- **`editor.color_decorators` off.** Nothing, and nothing is asked.

`answered` becomes "the lead's part of the merged answer had colours". A slot
records which server answered which colours, so that colour presentations can
go to the colour's owner.

**Asking again when a server's index changes.** Colours, inlay hints, lenses
and links are cached in fe-workbench slots, per buffer version and per server
identity. Neither changes when css-variables' index does (§2.4): its startup
scan finishes, a settings rescan runs, or a watched stylesheet is re-read. An
early empty answer would then stay until the next edit, and so would a
`var(--x)` swatch after `--x` changed in another file.

There is no signal that the server's indexing has *finished*. `didChange…`
being sent isn't one: the upstream parser awaits file reads and remote
`@import`s after it. And no fixed timer bounds a scan. So fastedit polls, with
notifications as a shortcut:

- **Polling.** A spec may set `repoll_ms`, via `ServerSpec::repoll` and
  `[lsp.servers.<name>] repoll_ms`, off by default. While a buffer open on
  that server is shown in some editor, its cached slots (colours, inlay hints,
  lenses, links) are asked of that server again every `repoll_ms`, on the
  manager's `Clock`:
  - only when the slot has no request of its own in flight;
  - only for that server's part of the merged answer;
  - for as long as the buffer is shown, so an index that finishes at any
    time, however long after start, is seen within one period.
  An answer equal to the cached one changes nothing and repaints nothing.
  Hidden buffers aren't polled; showing one again asks it at once.
  css-variables' `documentColor` is one regex pass over the document, so the
  built-in spec sets `repoll_ms = 3000`.
- **Notifications as a shortcut.** `LspEvent::Notified { server, what }`
  follows each `didChangeWatchedFiles` or `didChangeConfiguration` fe-lsp
  hands a session. fe-workbench then re-asks that server's slots of the shown
  buffers after `REST`, without clearing what they show. That catches the
  common fast case early. A rescan that ends later is caught by the next poll.
- **Empty answers** are cached like any other, and polled like any other, so
  a request that beat the startup scan is answered again within one period.
- **Where it lands.** Card A8 adds `repoll`, `Notified` and the poll clock in
  fe-lsp (`LspManager::repoll_due`). W2 polls the colour slots, and W1 polls
  the hint, lens and link slots.

**Decision:** the table above. Document sync goes everywhere. Completion, hover,
the goto family, references, workspace symbols, code actions and lenses, inlay
hints, links, colours, diagnostics and willRename are merged. Signature help,
highlights, rename and hierarchy preparation are first-that-answers in server
order. Symbols, formatting of every kind, folding, selection range, semantic
tokens and linked editing go to one provider. Every follow-up goes to its
item's owner. In `auto`, colours follow the lead's answer, with add-on colours
laid over the built-in ones.

## 6. Workbench and UI

### 6.1 The fe-lsp API that replaces "the server of a buffer"

| New | Meaning |
|---|---|
| `servers_of(buffer) -> Vec<ServerId>` | The servers the document is placed on now, in server order (the lead first when it is placed). |
| `lead_of(buffer) -> Option<ServerId>` | The configured lead's server, if the document is placed on it; otherwise `None`, never an add-on in its place (§3). `server_of` stays as a deprecated alias of it until the last caller moves (card W2), then goes. |
| `provider_of(buffer, Feature) -> Option<ServerId>` | §5's provider. `Feature` is a small fe-lsp enum (`SemanticTokens`, `Folding`, `DocumentSymbols`, `LinkedEditing`, `Formatting`, `OnTypeFormatting`, `SelectionRange`), so fe-workbench needn't name `lsp_types` capabilities. |
| `is_served(buffer) -> bool` | `!servers_of(buffer).is_empty()`. |
| `served_identities(buffer) -> Vec<(ServerId, SessionId, IncarnationId)>` | The active identity on each server, so a slot can tell a restart or reopen of any one server and ask again. |
| `route_of(buffer)` | Unchanged in type: the **lead's** name. |
| `route_servers(buffer) -> Vec<RouteServer>` | The document's configured servers, **lead first whether placed or not**, then its add-ons in order. Each is `RouteServer { name, root, add_on, placed: Option<ServerId> }`. This is configured identity, kept apart from live ids, for status, restart and the log. |
| `status_at(name, root) -> Option<ServerStatus>` | The latest status of the server started at (name, root), from the existing `statuses` map, or `None` if none ever started there (a missing command reports `Stopped(Some(…))`; an untrusted root has none). |
| `route_for(language)` | Unchanged: the lead's name. |
| `language_servers(root, language) -> Vec<(String, AddOn, Option<ServerStatus>)>` | Lead first. `language_server` stays, as `language_servers(..).first()` without the flag (the lead's state, running or not). |

### 6.2 Each call site

| Call site | Today | Replacement |
|---|---|---|
| `crates/fe-workbench/src/lsp.rs:644` `server_name` | `route_for(language)` | Unchanged: owners are keyed by (lead name, path), and every server of a route syncs the same holder (§4). |
| `lsp.rs:697`, `:704`, `:706` (`owners_match_manager`, test) | `server_of(..).is_none()` / `.is_some()` | `is_served(..)`: the holder is placed on some server of its route, and an alias that isn't the holder is placed on none, so being served is what owning the path means. |
| `lsp.rs:700`, `:707` (`owners_match_manager`, test) | `route_of` | Unchanged (lead's name). |
| `lsp.rs:736` (`actual_owner_servers`, test) | `route_of` | Unchanged (lead's name). |
| `lsp.rs:1500` (sync) | `document.server_name = route_of(buffer)` | Unchanged for `server_name` (lead). `Document` gains `route: Vec<RouteServer>` from `route_servers(buffer)`: configured names and roots, lead first, each with an optional live id. |
| `lsp.rs:1505` (sync) | `server_of(buffer).is_some() == served` | `is_served(buffer)`, for the same reason. With the primary down and an add-on placed, the holder still owns its path. |
| `lsp.rs:1697` (sync) | `document.server = server_of(buffer)` | `Document.server: Option<ServerId>` becomes the `placed` ids inside `Document.route`, refreshed from `route_servers(buffer)`. As now, an id is kept after its server stops, so that statuses and diagnostics can be matched. A configured server that never had an id still has its entry, by name and root. |
| `lsp.rs:830` `Lsp::server` (status, `force_lsp_status`, `run_server_op`) | the document's one server | `Lsp::route(buffer) -> &[RouteServer]`, configured, lead first. `Lsp::lead(buffer)` is `route[0]` by configured identity, **never** the first placed server. Its status is from `Lsp.servers` by id when it has one, else `status_at(name, root)`. With no status at all, as under an untrusted root, it shows what a single server shows there today. |
| `lsp.rs:1980` `lsp_prepare_rename`, `:1994` `lsp_rename` | `server_of(buffer)?` as a gate | `is_served(buffer)` as the gate. `prepare_rename` is first-that-answers inside fe-lsp, and its `RenameTarget` gains `owner: (ServerId, SessionId, IncarnationId)`, the server whose `prepareRename` accepted and the opening it accepted on. `lsp_rename` takes that owner, and fe-lsp's `rename` sends to it alone, only while that session still has the document open as that incarnation (`request_if`). Otherwise it answers `Unavailable("the language server restarted")` and nothing is renamed. |
| `lsp_rename.rs:43` `Request`, `:72` prepare, `:318` confirm | `Request` keeps the prepared `range` and `placeholder` only; confirming calls `lsp_rename(buffer, version, caret, name)` | `Request` gains `owner`, copied from the prepared `RenameTarget`. Confirming passes it to `lsp_rename`, so the rename goes to the server that accepted preparation even when it is an add-on, never to the lead or another server. If that owner restarted or reopened the document between preparation and confirmation, the picker closes with a toast ("The language server restarted: nothing renamed", like the existing `STALE`) rather than asking another server. |
| `lsp.rs:2013` `lsp_code_actioner` | `server_of(buffer)?` | `is_served(buffer)`. `code_actions_for` merges inside fe-lsp. |
| `lsp_colors.rs:114` | `server_of(buffer)`, one `Slot.server` / `identity` / `no_provider` | `served_identities(buffer)`. `Slot` keeps `asked_of: Vec<(ServerId, Identity)>` and asks again when it changes. `no_provider` becomes per server. The answer is merged in fe-lsp, with owners, and `Slot.answered` keeps `(Version, Vec<ColorSwatch>, lead_answered: bool)` for `default_colors::source`. |
| `lsp_inlay_hints.rs:79` | `server_of(buffer)`, `Slot.server` | `served_identities(buffer)`. Hints are merged in fe-lsp and `resolve_inlay_hint` goes by `hint.server`. A refresh from any one server re-asks. |
| `lsp_semantic_tokens.rs:129` | `server_of(buffer)` | `provider_of(buffer, Feature::SemanticTokens)`. A change of provider retires the old tokens, as a change of server does now. |
| `lsp_links.rs:182` | `server_of(buffer)` | `served_identities(buffer)`. Links are merged, and resolve goes by owner. |
| `lsp_lightbulb.rs:95` | `server_of` + `capabilities(server).code_action_provider` | `servers_of(buffer)` filtered by a code-action provider. `Wanted.server` becomes `Wanted.servers`. |
| `lsp_lenses.rs:153` | `server_of(buffer)` | `served_identities(buffer)`. Lenses are merged, and resolve and commands go by `lens.server`. |
| `lsp_folds.rs:77` | `server_of(buffer)` | `provider_of(buffer, Feature::Folding)`. |
| `lsp_symbols.rs:107` `session_of` | `server_of` + `active_identity` | `provider_of(buffer, Feature::DocumentSymbols)` + `active_identity`. |
| `lsp_linked_editing.rs:52` | `server_of(..)?` as a gate | `provider_of(buffer, Feature::LinkedEditing)?`. |
| `lsp_linked_editing.rs:93` | `server_of(..).is_some()` | `provider_of(buffer, Feature::LinkedEditing).is_some()`. |
| `hierarchy_panel.rs:865` | `server_of(buffer).is_some()` as a gate | `is_served(buffer)`. Preparation is first-that-answers, and each `HierarchyItem` carries its owner for expansion. |
| `workbench_commands.rs:1463` `restart_language_server`, `:1470` `show_language_server_log` → `lsp.rs:2085` `run_server_op` | `lsp.server(buffer)`, one `restart(name, root)` / `log(name, root)` | `lsp.route(buffer)`: every configured server of the active document, by (name, root), placed or not. Restart calls `restart(name, root)` for each, primary and add-ons, and records each returned id in `Document.route`. Log opens the log directly when the route has one server. Otherwise it first opens a picker of the route's servers ("css-variables-language-server — running", "vscode-css-language-server — stopped: not found"), each read with `log(name, root)`, which needs no id. |
| `crates/fe-lsp/src/navigation.rs:263` (`LspNavigation`, fe-nav) | `language_server(root, language)` | `language_servers(root, language)`. A language's readiness becomes the best of its servers' (Ready if any is). Navigation asks every placed server of the document that is ready, lead or add-on, and merges (§5). It waits for one still starting only as long as it waits for the lead today, then goes on with those that are ready. So a healthy add-on answers when the primary is missing, untrusted or down, and search is the fallback only when no server answers. |

### 6.3 Status, restart and a missing add-on

- **Status bar.** The item shows the **lead**'s status, as now, even while the
  lead isn't running and an add-on is: a missing or stopped primary still
  reads "stopped: …", whatever its add-ons do. An add-on's
  status appears only in the item's hover list ("css-variables-language-server
  — indexing…"), never in place of the lead's. The one exception is an add-on
  that started and then stopped with an error (crash budget spent): then
  "· css-variables-language-server stopped" follows the lead's text, in the
  error tone. A `window/showMessage` from an add-on shows as one from the
  primary does today, since the server chose to tell the user.
- **Restart Language Server** restarts all the document's servers (above). One
  that stays stopped for lack of a command says so in its log only.
- **A missing add-on.** If css-variables-language-server isn't installed, its
  launch fails ("not found") and the user sees one **one-time info toast**
  naming the npm package that provides it (the spec's `package`), and no status
  text or consent prompt beyond the one the root already has. The failure is
  also in its log, and the lead's features work as they did. This differs from a
  missing primary, whose "stopped: … not found" shows today; for a built-in
  add-on, one toast and no lasting status is the point. Like any stopped server it isn't relaunched
  until a restart, a settings change or a new spec.

**Decision:** the fe-lsp API in 6.1 and the replacements in 6.2. The status item
follows the lead, with add-ons in its hover list and shown only when one fails
after it started. Restart restarts every server of the document. A missing
add-on shows a one-time info toast naming the npm package.

## 7. Testing

- **Two fake servers for one document.**
  - **Launcher.** `crates/fe-lsp/src/tests/kill.rs:540` already has `Named`, a
    launcher sending each spec name to its own launcher. Card A2 moves it into
    `fake.rs` as `FakeLaunchers::new([("css", primary.launcher()), ("vars",
    add_on.launcher())])`. Each `FakeServer` keeps its own script, records,
    replicas and launches, so a test checks each server's `didOpen`/`didChange`
    replica and the requests it got separately.
  - **Specs.** Tests register two specs for one language, `ServerSpec { add_on:
    true, .. }` for the second, through `register_spec`.
- **Gate cases** (all fake, Linux-OK, no real server):
  - **Sync**
    - Both servers get `didOpen` at the same text, and each edit as its own
      `didChange` with its own version. Their replicas match the buffer after
      a burst of edits.
    - An add-on that joins on a settings reload opens at the current text.
    - Removing an add-on sends `didClose` to it only.
  - **Isolation**
    - A crash of the add-on leaves the primary's requests and diagnostics
      alone, and the reverse. Each restarts within its own budget.
    - `max_file_bytes` closes the document on both.
    - An untrusted add-on root, with the primary's root trusted, starts only
      the primary.
    - Alias buffers of one path are placed on both servers by the same holder,
      never split, including when the primary can't start and only the add-on
      is placed. Closing the holder moves both servers to the next alias.
    - Aliases with the same primary but different add-on sets (a CSS buffer
      and an SCSS buffer, with a CSS-only add-on) have one holder. The other
      waits on every server. fe-workbench's `owners_match_manager` holds.
    - A settings reload that gives two aliases' languages the same primary
      merges their groups. The earlier holder stays, the other gets `didClose`
      on each server it was on, and `owners_match_manager` holds after the
      reload.
  - **Lead unavailable**
    - With the primary's root untrusted, or its command missing, or its crash
      budget spent, a trusted add-on is still placed and synced. `lead_of` is
      `None`, `servers_of` lists the add-on, and `route_servers` still lists
      the primary first, with no id.
    - fe-workbench, primary never placed (untrusted root): the status item
      shows the primary's state, not the add-on's. Restart Language Server
      restarts both by (name, root). The log picker lists the primary's log as
      well as the add-on's.
    - In that state, a provider feature goes to the add-on when it advertises
      the feature, definitions come from the add-on through `LspNavigation`
      without waiting past today's ready wait, and search is used only when
      no server answers.
  - **Routing**
    - A primary-only feature with the primary lacking the provider goes to the
      add-on that has it (provider), and to nobody when neither does.
    - First-that-answers takes the primary's non-empty answer even when the
      add-on answers first, and the add-on's when the primary's is empty.
    - Rename goes to the server whose `prepareRename` accepted, by the owner
      carried in `RenameTarget`. When that owner restarted before `rename`,
      nothing is sent and the answer is `Unavailable`.
    - fe-workbench: the rename picker carries the owner through confirmation.
      With only the add-on accepting preparation, the add-on renames and the
      primary gets no `rename`. With the owner restarted before confirming,
      nothing is renamed and no other server is asked.
  - **Merging**
    - Completion lists: two lists merge, and a trigger character only one
      server lists asks only it.
    - Completion versions: with one server incomplete and the other complete,
      typing on re-asks both at the new version. Accepting the complete
      server's item after typing applies its edit at the new version's range,
      leaving no stray text. A request superseded by a newer one is cancelled
      on both servers, and its answer never reaches the newer menu.
    - Completion dedupe collisions. Each case runs twice, once with the
      primary answering first and once with the add-on first, and must give
      the same list:
      - Two equal final items (servers without resolve, no command) show once,
        the primary's copy.
      - A resolvable primary item beside an equal final add-on item: both
        show. The mirror (final primary, resolvable add-on) also shows both.
      - A primary item with a `command` beside an equal final, command-free
        add-on item: both show. The mirror also shows both.
      - Same label and edits, both without `data`, where the add-on's server
        resolves and its resolve adds a different import: both show, and
        accepting the add-on's applies its import.
      - Identical `command` payloads from two owners: both show, and each runs
        on its own server.
      - Same label and edits with different `filterText`: both kept. Typing
        that matches only the second still finds it.
    - Code-action collisions. Each case runs in both orders of arrival:
      - Two equal final, command-free actions with edits show once, the
        primary's copy.
      - A primary action with a `command`, or one awaiting resolve, beside an
        equal final add-on action: both show. The mirror also shows both.
      - Same title and kind with different edits or `disabled`: both show,
        with server names.
      - Identical commands from two owners: both show, and each runs on its
        own server.
      - Actions awaiting resolve: both show.
    - Hover contents join in order. Definitions and references are deduped by
      location. Two links or two colours at one range keep the earlier
      server's. Two hints at one position both show.
    - Workspace symbols, mixed languages: a TS primary, a CSS add-on and
      rust-analyzer at one root are each asked once. A symbol two servers
      report once is listed once, and every other symbol is kept.
    - One server erroring or timing out still gives the other's results.
  - **Owners.** Completion resolve, code action resolve, lens resolve, inlay
    hint resolve, link resolve, `colorPresentation` and hierarchy expansion
    each reach only the server that produced the item, and none reaches a
    restarted session.
  - **Colours** (fe-workbench)
    - `auto` with a lead that has colours shows only the merged server colours.
    - `auto` with a lead that has none shows the built-in colours with an
      add-on's overlapping colour replacing the built-in one.
    - `never` and `always` behave as in §5.
    - Index changes without edits, on a manual clock:
      - A fake add-on whose "scan" ends long after start (past any fixed
        timer) answers empty until then. Its colours show at the first poll
        after, with no edit.
      - A watched-file change whose processing ends after `REST` (the fake
        still answers the old colour at the `Notified` re-ask) shows the new
        colour at the next poll, with the consumer unedited.
      - A hidden buffer isn't polled.
      - An equal answer doesn't repaint.
  - **Watches** (fe-lsp)
    - Two roots, each with its own add-on session, define `--x` differently.
      A change under root B reaches only B's session.
    - A change under a `blacklistFolders` folder (`node_modules/`, `dist/`)
      reaches neither.
    - Changing `settings.cssVariables.lookupFiles` changes what is sent from
      then on.
  - **UI** (fe-workbench)
    - The status item shows the lead's status.
    - A missing add-on shows one info toast naming its npm package, once.
    - Restart restarts both servers.
    - The log picker lists both.
- **Real server.** One `#[ignore]`d test in the slow group (lsp-client.md
  §6.3, `crates/fe-lsp/tests/real_server.rs`, whose servers
  `scripts/lsp-real.sh` installs into `tools/lsp`) runs
  css-variables-language-server on a temporary root with two stylesheets. It
  checks a `var(--x)` colour and a completion from the other file, and skips
  when the command isn't installed.

**Decision:** two `FakeServer`s behind a name-keyed `FakeLaunchers`, with the
cases above in the gate and one real-server smoke test in the slow group.

## 8. Implementation plan

All cards are size s or m, and all are Linux-OK: fe-lsp and fe-workbench's LSP
code have no platform code, and the fe-workbench tests run headless on the GPUI
test platform with no screenshots. Test files are new unless noted, and each is
declared from its directory's `mod.rs` or `tests/mod.rs`. Cards that touch
`crates/fe-workbench/src/lsp_colors.rs` are marked **[lsp_colors.rs]**.

| Card | Size | What | Scope | Depends on | Tests added |
|---|---|---|---|---|---|
| A1 | s | `ServerSpec::add_on` and `LspServerSettings::add_on` (schema, template, `apply`), plus `registry::servers_for` / `Servers::lead`. `server_for` skips add-ons. No routing change. | `crates/fe-lsp/src/launcher.rs`, `crates/fe-lsp/src/registry.rs`, `crates/fe-lsp/src/tests/registry/**`, `crates/fe-lsp/src/tests/registry.rs`, `crates/fe-config/src/settings.rs`, `crates/fe-config/settings.schema.json`, `crates/fe-config/settings-template.toml`, `crates/fe-config/tests/lsp_add_on_setting.rs` | — | `crates/fe-lsp/src/tests/registry/add_on.rs` (primary/add-on/lead choice, `enabled`, `typescript_server`, extension and host add-ons, `needs_restart`), `crates/fe-config/tests/lsp_add_on_setting.rs` (parse, schema, project-layer filter) |
| A2 | s | `FakeLaunchers` (name-keyed launcher) moved from `kill.rs` into the fake. | `crates/fe-lsp/src/tests/fake.rs`, `crates/fe-lsp/src/tests/kill.rs`, `crates/fe-lsp/src/tests/fake_launchers.rs`, `crates/fe-lsp/src/tests/mod.rs` | — | `crates/fe-lsp/src/tests/fake_launchers.rs` |
| A3 | m | The subscription hub: `Subscribed` with per-server taps and latest text. Still one server per document, so behaviour is unchanged. | `crates/fe-lsp/src/document.rs`, `crates/fe-lsp/src/server.rs`, `crates/fe-lsp/src/manager.rs`, `crates/fe-lsp/src/tests/sync/**`, `crates/fe-lsp/src/tests/sync.rs` | — | `crates/fe-lsp/src/tests/sync/fan_out.rs` (two taps drain the same events in order; a late tap starts at the latest text; detach stops both) |
| A4 | m | Placement per (document, server) at open, close, `changed` and crash: `Route.roots`, keyed places and lot, one holder per (URI, lead name) chosen apart from placement, each server placed independently (trust, crashes), and a configured lead that is never replaced, plus `servers_of`, `lead_of`, `is_served`, `served_identities`, `routes_of`, `language_servers`. Requests still go to the lead. | `crates/fe-lsp/src/manager.rs`, `crates/fe-lsp/src/server.rs`, `crates/fe-lsp/src/lib.rs`, `crates/fe-lsp/src/tests/add_ons/**`, `crates/fe-lsp/src/tests/add_ons.rs`, `crates/fe-lsp/src/tests/mod.rs` | A1, A2, A3 | `crates/fe-lsp/src/tests/add_ons.rs`, `crates/fe-lsp/src/tests/add_ons/sync.rs` (both replicas, versions, `max_file_bytes`, aliases), `crates/fe-lsp/src/tests/add_ons/crash.rs` (one crashes, the other unaffected), `crates/fe-lsp/src/tests/add_ons/lead_unavailable.rs` (untrusted, missing or crashed-out primary with a healthy add-on; holder kept; `route_servers` lead first with no id), `crates/fe-lsp/src/tests/add_ons/holders.rs` (same lead with different add-on sets, groups merging on a reload, two leads meeting on one add-on) |
| A5 | m | Reconcile per server: settings, trust, extension and host specs adding or removing add-ons, reroot per server, and `restart(name, root)` of one. | `crates/fe-lsp/src/manager.rs`, `crates/fe-lsp/src/restart.rs`, `crates/fe-lsp/src/tests/add_ons/**` | A4 | `crates/fe-lsp/src/tests/add_ons/reconcile.rs` (join at latest text, removal `didClose`, untrusted add-on root, `enabled = false` on each) |
| A6 | m | Routing core: `provider_of` and `Feature`, plus `ask_all` / `first_that_answers` helpers over `document_request` aimed at one server. Owner fields on `ColorInformation`, `DocumentLink` and `HierarchyItem`, so colour presentations, link resolve and hierarchy follow-ups go to the owner. The provider rows of §5 (symbols, formatting, on-type, folding, semantic tokens, linked editing), plus signature help, highlights, rename and hierarchy preparation as first-that-answers. `RenameTarget.owner`, and `rename` taking it and sending only to that session. | `crates/fe-lsp/src/manager.rs`, `crates/fe-lsp/src/requests.rs`, `crates/fe-lsp/src/{document_color,document_link,hierarchy,signature_help,document_highlight,rename,formatting,folding_range,semantic_tokens,linked_editing,document_symbol}.rs`, `crates/fe-lsp/src/tests/add_ons/**` | A4 | `crates/fe-lsp/src/tests/add_ons/provider.rs`, `crates/fe-lsp/src/tests/add_ons/first_answer.rs`, `crates/fe-lsp/src/tests/add_ons/owners.rs` |
| A7 | m | Merged features in fe-lsp: completion (one version per list, re-ask every server on any `isIncomplete`, equivalence-only dedupe, trigger union), hover, the goto family and references with `LspNavigation` readiness over `language_servers`, workspace symbols (dedupe over the existing all-sessions enumeration), code actions (equivalence-only dedupe), lenses, inlay hints, links and colours (precedence by range), with one server's failure tolerated. | `crates/fe-lsp/src/{completion,hover,navigation,code_action,code_lens,inlay_hint,document_link,document_color}.rs`, `crates/fe-lsp/src/requests.rs`, `crates/fe-lsp/src/tests/add_ons/**` | A6 | `crates/fe-lsp/src/tests/add_ons/merge_completion.rs`, `crates/fe-lsp/src/tests/add_ons/merge_lists.rs` (hover, locations, actions, lenses, hints, links, colours; one failing), `crates/fe-lsp/src/tests/add_ons/merge_collisions.rs` (completion and code-action collisions, each in both orders of arrival: both final, one side resolvable or command-bearing and the mirror, resolve adding imports without `data`, identical commands from two owners, differing `filterText`), `crates/fe-lsp/src/tests/add_ons/workspace_symbols.rs` (mixed languages) |
| A8 | m | Root-scoped static watches and index-change events. `ServerSpec::watch_files` / `watch_excludes` and `[lsp.servers.<name>]` keys of the same names hold globs relative to the session's root; a path outside the root never matches. Alternatively, `watch_files_from` / `watch_excludes_from` are JSON pointers into the spec's `settings`, read again at each configure, so the watch follows the effective `lookupFiles` and `blacklistFolders`. Either way the result is held as a permanent `didChangeWatchedFiles` registration of the session. Adds `LspEvent::Notified { server, what }` after each `didChangeWatchedFiles` and `didChangeConfiguration` sent, and `ServerSpec::repoll` / `[lsp.servers.<name>] repoll_ms` with `LspManager::repoll_due(server, now)` on the manager's `Clock` (§5, polling). | `crates/fe-lsp/src/launcher.rs`, `crates/fe-lsp/src/registry.rs`, `crates/fe-lsp/src/registration.rs`, `crates/fe-lsp/src/watched_files.rs`, `crates/fe-lsp/src/event.rs`, `crates/fe-lsp/src/server.rs`, `crates/fe-lsp/src/tests/registration/**`, `crates/fe-lsp/src/tests/registration.rs`, `crates/fe-config/src/settings.rs`, `crates/fe-config/settings.schema.json`, `crates/fe-config/settings-template.toml` | A1 | `crates/fe-lsp/src/tests/registration/static_watch.rs` (two roots with the same variable names, excluded folders, a `lookupFiles` change), `crates/fe-lsp/src/tests/registration/index_events.rs` (`Notified` after each notification; `repoll_due` on a manual clock; `repoll_ms` parsed, defaulting off) |
| W1 | m | fe-workbench call sites except colours: `lsp.rs` (`Document.route`, `Lsp::route`/`lead`, gates), `lsp_inlay_hints.rs`, `lsp_semantic_tokens.rs`, `lsp_links.rs`, `lsp_lightbulb.rs`, `lsp_lenses.rs`, `lsp_folds.rs`, `lsp_symbols.rs`, `lsp_linked_editing.rs`, `hierarchy_panel.rs`, and `lsp_rename.rs`, whose `Request` carries the prepared rename's `owner` through confirmation. `Document.route` holds configured `RouteServer`s; `Lsp::route` and `Lsp::lead` select by configured identity. Hint, lens and link slots of shown buffers are re-asked on `Notified` and polled every `repoll_ms` (§5). | `crates/fe-workbench/src/lsp.rs`, `crates/fe-workbench/src/lsp_{inlay_hints,semantic_tokens,links,lightbulb,lenses,folds,symbols,linked_editing}.rs`, `crates/fe-workbench/src/hierarchy_panel.rs`, `crates/fe-workbench/src/lsp_rename.rs`, `crates/fe-workbench/src/tests/lsp_rename_add_ons.rs`, `crates/fe-workbench/src/tests/lsp_add_ons.rs`, `crates/fe-workbench/src/tests/mod.rs` | A7, A8 | `crates/fe-workbench/src/tests/lsp_add_ons.rs` (two fakes; hints, lenses and actions from both; folds and tokens from the provider; owners kept; a primary that can't start with a served add-on keeps the path's owner; CSS and SCSS aliases with different add-on sets keep `owners_match_manager`; a hint changed by the add-on's index shows at the next poll), `crates/fe-workbench/src/tests/lsp_rename_add_ons.rs` (only the add-on accepts `prepareRename`: confirming renames through the add-on alone and the primary gets no `rename`; the owner restarts between preparation and confirmation: nothing is renamed, no other server is asked, and the toast shows) |
| W2 | s | **[lsp_colors.rs]** Colours: per-server identities in `Slot`, the merged answer with owners, presentations to the owner, and `default_colors::source` taking "the lead answered" with the add-on overlay in `auto`. Colour slots of shown buffers are re-asked on `Notified` and polled every `repoll_ms`. Removes the `server_of` alias. | `crates/fe-workbench/src/lsp_colors.rs`, `crates/fe-workbench/src/default_colors.rs`, `crates/fe-lsp/src/manager.rs`, `crates/fe-workbench/src/tests/lsp_colors_add_ons.rs`, `crates/fe-workbench/src/tests/mod.rs` | A7, A8, W1 | `crates/fe-workbench/src/tests/lsp_colors_add_ons.rs` (`auto` with and without lead colours, `never`, `always`, presentations to the owner, a scan ending long after start shows at the next poll, a watched-file change processed after `REST` shows at the next poll with the consumer unedited, hidden buffers not polled) |
| W3 | s | Status item (the configured lead by `Lsp::lead`, the hover list, a failed add-on), Restart Language Server for every configured server by (name, root), the Language Server Log picker over `Lsp::route`, and a one-time info toast naming the npm package for a missing add-on. | `crates/fe-workbench/src/lsp.rs`, `crates/fe-workbench/src/workbench_commands.rs`, `crates/fe-workbench/src/tests/lsp_status_add_ons.rs`, `crates/fe-workbench/src/tests/mod.rs` | W1 | `crates/fe-workbench/src/tests/lsp_status_add_ons.rs` (primary never placed under an untrusted root: the item shows the primary's state; restart and the log reach both servers by name and root; a missing add-on shows one info toast naming its package, once) |
| A9 | s | **Last.** css-variables-language-server in `builtin_specs` as an add-on: command and `--stdio`, §2's languages (as Q1 settles), `package.json` marker, `settings.cssVariables`, `watch_files_from` / `watch_excludes_from` pointing into it, and `repoll_ms = 3000`. Settings template and docs list it, and the slow-group real-server test. | `crates/fe-lsp/src/registry.rs`, `crates/fe-lsp/src/tests/registry/**`, `crates/fe-lsp/tests/real_server.rs`, `crates/fe-lsp/tests/fixtures/real/**`, `scripts/lsp-real.sh`, `tools/lsp/package.json`, `tools/lsp/package-lock.json`, `crates/fe-config/settings-template.toml`, `docs/**` | A5, A7, A8, W2, W3 | `crates/fe-lsp/src/tests/registry/css_variables.rs` (it is an add-on beside vscode-css-language-server for CSS and beside the TS server for `tsx`, its settings reach `workspace/configuration`, and its root-scoped watch follows those settings), plus `real_server_css_variables` in `crates/fe-lsp/tests/real_server.rs` (§7) |

Only **W2** touches `crates/fe-workbench/src/lsp_colors.rs`. W1 leaves it
alone, keeping `server_of` (the deprecated alias of `lead_of`) until W2 moves
it, so W1 and W2 don't collide. W2 then removes the alias. A2 and A3 can run
alongside A1, and A8 needs only A1.

**Decision:** the twelve cards above (A1–A8, W1–W3, then A9, the built-in spec, last).
Each is s or m, and all are Linux-OK. Only W2 touches `lsp_colors.rs`.

## 9. Out of scope and open questions

**Out of scope:**

- Add-ons for documents placed with `open_on`.
- Merging semantic tokens or folding from several servers.
- Per-language ordering of add-ons other than registry order.
- A setting that makes an add-on's missing command visible.
- Feeding unsaved buffer text to css-variables, whose design reads only disk.
- Fixing the server's last-definition-wins cache.
- `didChangeWorkspaceFolders` for multi-root.
- `textDocument/selectionRange` itself, which fe-lsp doesn't send yet.

**Answers (approved by the user 2026-11-12):**

- **Q1** (§2): keep. `javascript`, `typescript` and `tsx` stay in
  css-variables-language-server's languages.
- **Q2** (§6.3): changed. A missing built-in add-on is not silent: it shows a
  one-time info toast naming the npm package.
- **Q3** (§2.4): fixed list. `target`, `build`, `out`, `vendor` and
  `.worktrees` go in `blacklistFolders`.
- **Q4** (§2.4): covered by trust. Servers start only in trusted folders.

**Decision:** the scope above, with Q1 to Q4 answered as listed.
