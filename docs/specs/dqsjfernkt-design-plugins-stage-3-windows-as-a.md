---
task_uid: "01m70b3amtz0yw5tdqsjfernkt"
task: "T1652"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003clane\u003e-plugins-stage-3.md exists with sections Status, Windows as a stage-2 target, Two budget tiers, API gaps, Core vs plugin, Implementation plan, Out of scope and open questions; each ends in a decision or a named open question"
    required: true
    manual: false
  - id: "C2"
    text: "Windows section lists every Unix-only item in kiln-plugin and fe-plugin found by reading the code (spawn and resolution, process-group kill, preopens, path and drive-letter handling, symlinked dev links, tree watcher, cfg(unix) tests such as PermissionsExt), the change for each, and which tests run in the ci.yml windows job"
    required: true
    manual: false
  - id: "C3"
    text: "Budget tiers gives concrete numbers with reasons: interactive deadline and memory vs background wall-time, fuel, manifest-declared memory cap with a host ceiling shown in the install prompt, the background-job API (start, progress, cancel, result; Runs model of T1570 fit or non-fit), streamed program output, and one chosen language-server path (native fe-lsp-managed vs WASI long-lived store) with the other costed"
    required: true
    manual: false
  - id: "C4"
    text: "API gaps has a WIT sketch per gap (language features, panels, tasks and runs, settings contributions), each naming its capability, budget tier and limits; language features cite the routing table rows of the T1645 decision doc (docs/decisions/q5j03ax2af-lsp-add-on-servers.md) rather than restating them; it covers compat.rs adapters with the exact conditions for marking the worlds stable, and signing and registry links to T1481-T1486 and T1482"
    required: true
    manual: false
  - id: "C5"
    text: "Core vs plugin section has a cost model measured on this repo (per-instance memory, cold and cached startup, per-call boundary cost for small and large payloads, with commands and numbers), states which built-ins stay core (editing, vim, rendering, syntax, LSP client, terminal, search, each with a reason) and which may become first-party plugins, and recommends 2 or 3 dogfood plugins with the API gaps each exercises"
    required: true
    manual: false
  - id: "C6"
    text: "Implementation plan lists cards of size s or m only, each with scope globs, dependencies, Linux-OK or Mac-only, Windows-relevant, and new test files, Windows cards first"
    required: true
    manual: false
  - id: "C7"
    text: "The doc's status line says proposed and names the user as the approver; no implementation card is filed by this card, and git diff --stat against the base lists only the new doc; the finish notes ask the USER to sign off before any stage-3 card is drafted"
    required: true
    manual: false
---

# Design plugins stage 3: Windows as a target, background-job and larger-memory budget tier, language-server plugins, API gaps (language features, panels, tasks, settings), and what moves from core to first-party plugins, in a decision record the user signs off

The user (2026-11-09): "We're going to want the plugins to be cross-platform, so Windows does need to be a target. Are the limits on a plugin enough? That's great for a tree sitter, but let's say it's something more advanced, like compiling the project, or doing a more advanced lookup. And 64 meg probably couldn't run a language server in the sandbox. Anything on the interactive loop, yes, we want these fast timelines, but if it's an asynchronous process, maybe we can give it more latitude. And let's shore up the API gaps as well. Is there anything in the app that we should be moving to a plugin instead of keeping core? Or is that going to slow things down and take up more memory?"

Stage 1 and 2 are accepted (docs/decisions/vmtnmkay62-extensions-and-plugins.md, T839) and built (T1045 onward: kiln-plugin, fe-plugin, wit worlds kiln:extension and fastedit:editor, both still preview). Today every plugin call runs under CALL_DEADLINE 100 ms, MEMORY_BYTES 64 MiB, PROGRAM_DEADLINE 10 s total per call, EVENT_QUEUE 256, MAX_READ_BYTES 16 MiB (kiln/crates/kiln-plugin/src/limits.rs); the world has log, commands, events, fs, process, net, secrets and fastedit's buffer, decorations, diagnostics, formatting, status. That suits a formatter or a decorator, not a compile, a deep lookup or a language server. This is a design card: one doc, no code.

## Deliverable

docs/decisions/<lane>-plugins-stage-3.md (the `<lane>-` prefix convention of docs/decisions/), each section ending in a decision or a named open question for the user:

1. **Status**: 'proposed', the user as approver, no code changed, how it amends the accepted record (which of its §7, §8, §12 lines it supersedes).
2. **Windows as a stage-2 target**: everything Unix-only today in kiln-plugin and fe-plugin (read it, do not guess: program spawn and resolution, process-group kill and signals, fs preopens and path handling, `\` and drive letters in grants and manifests, symlinked dev links, the tree watcher, file modes such as PermissionsExt in the tests, the compile cache dir), what changes for each, and how the existing `windows` job in .github/workflows/ci.yml proves it (which kiln-plugin and fe-plugin tests run there, which are cfg(unix) today and get a Windows twin).
3. **Two budget tiers**: interactive calls (event and command handlers, decorations, formatting on the edit path) keep the tight deadlines; background work gets latitude. Decide with concrete numbers and reasons: the background-job API (start, progress, cancel, result; progress into the Runs model of T1570 if it fits, say if it does not), its own wall-time and fuel policy, a larger memory cap declared in the manifest, shown in the install prompt and with a host ceiling (and how it interacts with the user's per-plugin override settings), long-running spawned programs with streamed stdout and stderr, backpressure and cancellation (compiling a project; PROGRAM_DEADLINE's 10 s total becomes what for a background job), and how a plugin provides a language server: (a) a native server process that the host's fe-lsp manages (trust, grants, restart, status UI) versus (b) a WASI server in its own long-lived store with a bigger memory cap; pick one as the supported path, say what the other costs.
4. **API gaps, with WIT sketches**: (a) language features (completion, hover, signature help, code actions, goto family, references, symbols, inlay hints, formatting, diagnostics) routed through the same multi-provider routing, merge, dedupe and resolve-back-to-owner rules as the add-on servers of T1645 (read docs/decisions/q5j03ax2af-lsp-add-on-servers.md on that lane; cite its table rows, do not re-invent them; say what changes if that doc changes); (b) panels as a declarative view tree with no webviews (node kinds, events back, theming, accessibility, size limits); (c) tasks and runs; (d) settings contributions (schema, validation, per-project vs user, secrets); (e) the version adapters in kiln-plugin compat.rs: what exists, what a 1.1 minor needs, and the exact conditions under which the preview worlds are marked stable; (f) signing and registry, linking to T1481 to T1486 and T1482 and the deferred stage-2 signing question. Each API names its capability, its tier from section 3 and its host-side limits.
5. **Core vs plugin, measured**: a cost model of a wasm plugin from measurements run on this repo's kiln-plugin (per-instance memory, startup cold and with the compile cache, per-call boundary cost for a small and a large payload, thread cost), with the commands and numbers in the doc. Then which built-ins, if any, should become first-party plugins and which must stay core (at least: editing, vim, rendering, syntax, LSP client, terminal, search, each with a one-line reason). Recommend 2 or 3 first-party plugins to dogfood the API, each with the API gaps it exercises. Answers the user's memory and speed question directly.
6. **Implementation plan**: cards of size s or m only (never l), each with scope globs, dependencies, Linux-OK or Mac-only, Windows-relevant yes or no, and its new test files; the Windows cards first.
7. **Out of scope and open questions**.

## Notes

No implementation card is filed by this card; when it lands the lead brings the doc to the user. Touch no file but the new doc. Measurements may be run in a scratch checkout or with cargo's target dir outside the repo; none of that is committed. Linux-OK. Do not read Zed, VS Code or other editors' plugin code beyond their public docs; say in the finish notes which public docs, if any, were read.
