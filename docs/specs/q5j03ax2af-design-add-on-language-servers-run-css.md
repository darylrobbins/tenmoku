---
task_uid: "01m6z4nm24mkexffq5j03ax2af"
task: "T1645"
rev: 3
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003clane\u003e-lsp-add-on-servers.md exists with sections Status, What css-variables-language-server needs, Spec model, Document sync, Feature routing, Workbench and UI, Testing, Implementation plan, Out of scope and open questions; each ends in a decision or a named open question"
    required: true
    manual: false
  - id: "C2"
    text: "What css-variables-language-server needs cites the server source at a named commit or release and lists its advertised capabilities, languages and languageIds, how it reads its cssVariables settings, and what it scans at startup"
    required: true
    manual: false
  - id: "C3"
    text: "Feature routing has one row for every request and notification fe-lsp sends today (completion and every resolve, hover, signature help, the goto family, references, highlights, symbols, rename, code actions and lenses, formatting, folding, selection range, semantic tokens, inlay hints, links, linked editing, hierarchies, documentColor and colorPresentation, pushed and pulled diagnostics), each primary-only, first-that-answers or merged, with merge, dedupe and resolve-back-to-owner rules for merged ones, and states how merged documentColor interacts with editor.default_color_decorators; dedupe applies only when both compared items meet the conditions (final, command-free), shown for both orders of arrival, and the routing table renders as one Markdown table with single-line rows (longer explanations sit below it, referenced from the row)"
    required: true
    manual: false
  - id: "C4"
    text: "Workbench and UI names the replacement for each fe-workbench call site of server_of, route_of and language_server (crates/fe-workbench/src/lsp.rs and the lsp_*.rs, hierarchy_panel.rs and workbench_commands.rs files)"
    required: true
    manual: false
  - id: "C5"
    text: "Implementation plan lists cards of size s or m only, each with scope globs, dependencies, Linux-OK or Mac-only and its new test files, ending with the card that adds css-variables-language-server to builtin_specs as an add-on, and marks which cards touch crates/fe-workbench/src/lsp_colors.rs"
    required: true
    manual: false
  - id: "C6"
    text: "The doc's status line says proposed and names the user as the approver; no implementation card is filed by this card, and git diff --stat against the base lists only the new doc"
    required: true
    manual: false
---

# Design add-on language servers: run css-variables-language-server beside a language's primary server (vscode-css-language-server), with per-feature routing and merging

The user (2026-11-08) approved making css-variables-language-server (npm `css-variables-language-server`, vunguyentuan/vscode-css-variables, MIT; `css-variables-language-server --stdio`) a built-in language server. It resolves `var(--x)` to its colour and completes custom properties defined in other files of the project. It is an add-on: it runs beside a language's main server (vscode-css-language-server for CSS/SCSS/Less, the card this depends on; also beside the TypeScript, Vue, Svelte and Astro servers for the languages it lists), not instead of it.

fastedit can't do that today. One server is chosen per language: `server_for` (crates/fe-lsp/src/registry.rs) takes the first spec listing the document's language, `LspManager::open` (crates/fe-lsp/src/manager.rs) syncs a buffer to that one server, and `server_of`, `route_of` and `language_server` return a single server. fe-workbench asks for "the" server of a buffer in crates/fe-workbench/src/lsp.rs (12 call sites) and one each in lsp_colors.rs, lsp_inlay_hints.rs, lsp_semantic_tokens.rs, lsp_links.rs, lsp_lightbulb.rs, lsp_lenses.rs, lsp_folds.rs, lsp_symbols.rs, lsp_linked_editing.rs (2), hierarchy_panel.rs and workbench_commands.rs. docs/decisions/lsp-client.md §3.2 says "M4 has one server per language". Adding the spec alone would leave it never chosen for CSS. So this is a design card first: one doc, no code.

## Deliverable

docs/decisions/<lane>-lsp-add-on-servers.md (the `<lane>-` prefix convention of docs/decisions/), with these sections, each ending in a decision or a named open question for the user:

1. **Status**: 'proposed', names the user as approver, says no code changed.
2. **What css-variables-language-server needs**: from its source (packages/css-variables-language-server in vunguyentuan/vscode-css-variables, via Context7 or the web, at a named commit or release): the capabilities it advertises (completion, documentColor, hover, definition, others), the languages and languageIds it handles, how it reads settings (`cssVariables.lookupFiles`, `cssVariables.blacklistFolders`, ...; `workspace/configuration` or `initializationOptions`), whether and how it scans the workspace folder at startup (cost on a large repository, honouring node_modules/.git exclusions), and which root markers suit it.
3. **Spec model**: how a spec says it is an add-on rather than the primary (a `ServerSpec` field and an `[lsp.servers.<name>]` key, with its name, default and trust-boundary treatment per lsp-client.md §3.3); how many add-ons a language may have; whether settings, extensions and the host can declare add-ons; how `lsp.typescript_server`-style choice and `enabled = false` apply.
4. **Document sync**: a document synced to a primary and N add-ons at once: the manager's document/route state, one buffer subscription fanned out or one per server, per-server version rings and identities, `max_file_bytes`, trust, restarts and crashes of one server without disturbing the others, root per server (each has its own markers).
5. **Feature routing**: a table of every feature fe-lsp sends today (completion and resolve, hover, signature help, definition/declaration/type definition/implementation, references, document highlight, document symbols, workspace symbols, rename, code actions and lenses, formatting (whole, range, on type), folding, selection range, semantic tokens, inlay hints, document links, linked editing, call/type hierarchy, documentColor and colorPresentation, diagnostics pushed and pulled) saying primary-only, first-that-answers, or merged across servers; for merged ones how results combine and dedupe, and how a follow-up request (completion resolve, code action resolve, colorPresentation, lens resolve) goes back to the server that produced the item. Say how documentColor merging interacts with the built-in colour detection card's `editor.default_color_decorators` (server colours win in `auto`), and build on it if it has landed.
6. **Workbench and UI**: the API that replaces "the server of a buffer" for each call site listed above, the status bar and server-status UI with several servers per document, Restart Language Server with several, and what a user sees when the add-on isn't installed (nothing, as for any missing built-in).
7. **Testing**: how the fake server (crates/fe-lsp/src/tests/fake.rs) runs two servers for one document, and which merge and routing cases the gate tests.
8. **Implementation plan**: cards of size s or m (never l), each with scope globs, dependencies, Linux-OK or Mac-only and the test files it adds; the last one adds css-variables-language-server to `builtin_specs` as an add-on for its languages. Note which cards touch crates/fe-workbench/src/lsp_colors.rs.
9. **Out of scope and open questions**.

## Notes

No implementation card is filed by this card; when it lands the lead brings the doc to the user. Touch no file but the new doc. Linux-OK (no platform code involved).
