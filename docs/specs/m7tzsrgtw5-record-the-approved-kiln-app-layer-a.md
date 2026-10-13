---
task_uid: "01m4vbhy4y90rtfcm7tzsrgtw5"
task: "T665"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/kiln-app-layer.md exists, status build, approved by the owner 2026-10-13, with the decision, the rule, the diagnosis, the three options compared with their table, the chosen shape, the three phases, what not to do, risks with mitigations and the prototype evidence"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-app-layer.md states the registration rule: inventory auto-registration only within the app crate; library crates register through an explicit kiln::Module passed to .module(..), because the prototype found unreferenced library registrations silently missing"
    required: true
    manual: false
  - id: "C3"
    text: "app-framework.md section 3 no longer says there is no kiln App type; it quotes the rule (default the composition; wrap nothing on the paint path; every convention has a one-line override and every sugar a GPUI-shaped escape hatch), gives the 2026-10-13 approval, keeps kiln::init and Config as the manual path and links kiln-app-layer.md"
    required: true
    manual: false
  - id: "C4"
    text: "kiln/docs/framework.md section 3 and kiln/README.md say the same without naming fastedit"
    required: true
    manual: false
---

# Record the approved Kiln app layer: a design record in docs/decisions and the amended no-App-type rule

## Problem

The owner approved a new direction for Kiln's developer experience on 2026-10-13: "I approve the new kiln
approach. Let's draft cards for the work and start working on it." It reverses one line of
`docs/decisions/app-framework.md` §3, "There is no kiln `App` type", which kiln's own copies repeat
(`kiln/docs/framework.md` §3 line 57, `kiln/README.md` line 53). The proposal it approves was written outside
the repository (a design note of 2026-10-13 against main @ d5ed50eb), so nothing in the repo records the
decision, its reasons or its limits, and the Kiln cards that follow need a record to cite.

## Change

Write the record, amend §3, and say the same in kiln's standalone docs. Docs only; no code.

### 1. New `docs/decisions/kiln-app-layer.md`

Status **build**, approved by the owner 2026-10-13. Copy in the essentials below (rephrase freely; keep the
facts, file:line references and numbers):

**Decision.** Re-envision Kiln in the *composition layer*, not the rendering model. Option A ("batteries plus
attribute macros over today's Kiln") plus two small pieces of a reactive layer: `Cx<V>` (a window-plus-context
bundle that derefs to `Context<V>`, with `pub window` and `pub cx`) and tracked `cx.watch::<Model>()`, which
replaces stored `Subscription`s. No `rsx!`/`view!` element macro, no Copy-signal arena, no `.kiln` DSL.

**Rule** (replaces "extend GPUI, never wrap it"): *Default the composition; wrap nothing on the paint path.
Every convention has a one-line override, and every sugar has a GPUI-shaped escape hatch.*

**Diagnosis** (what an app author must handle by hand today):

- the composition tax: `kiln/crates/kiln/examples/host/app.rs` is 566 lines, about 400 of them generic
  workspace glue (`watch`, `sync_peers`, `pane_changed`, `tab_dropped`, `close_pane`, `follow_active`, the
  `Panes` centre view, the status bar view), and fastedit has a near-verbatim copy (`PaneArea`,
  `crates/fe-workbench/src/workbench.rs` ~2134);
- the Shell vs Root cliff: the first dock panel forces a rewrite from `Root` onto `Shell`, `DockState`,
  `Dock`, `Capabilities`, `shortcut_contexts`; `Shell::new`'s `UiTheme` argument is ignored (`shell.rs:83`);
- subscription lifetimes and `cx.notify()` relays: `[Subscription; N]` arrays in every view; fastedit's
  `Workbench` has 54 fields, 12 of them `Subscription`s;
- one command in four places linked by strings (the `commands!` list, `keymap.toml`, `menus.toml`, the
  handler plus `.on_action(cx.listener(..))`); fastedit's command families each add an `Op` enum, a
  `CommandTarget`, a `Global` registry, `init`/`registry()`/`op()`, and an arm in `on_run_command`'s 15-arm
  chain (`workbench.rs` ~1979-2059), listed again in `command_catalog()` and `command_list()`
  (`crates/fe-workbench/src/lib.rs` ~153-200); every app copies 30 lines of picker keys;
- persistence by hand and untyped (`LayoutStore`, `WindowLayout`, quit and close hooks, `Value` encoding);
- per-app paths, user files, settings plumbing and stringly theme tables;
- closed extension points (fastedit's `enum DockPanel` with five parallel `match` tables) and test-only
  accessors (`#[allow(dead_code, reason = "for tests/host.rs")]`).

**Options compared** (the same notes app: sidebar, a command with key and menu, a status item, one setting,
persistence):

| | Today | A: batteries + macros | B: reactive layer | C: full DSL |
|---|---|---|---|---|
| Code | ~800 Rust + ~65 TOML, 10 files | ~103 Rust, 0 TOML, 4 files | ~72 Rust | ~26 DSL + 13 Rust |
| Escape to GPUI | n/a | everywhere | partial, two models | at the boundary only |
| Effort | n/a | 6–8 weeks kiln + 3–5 fastedit | A + 5–8 weeks | 4–6+ months + tooling |
| Risk to fast paths | n/a | none (same GPUI calls) | low if confined | none, but splits the codebase |

B buys nothing on GPUI's per-view rendering (fine-grained signals need a retained tree; GPUI has none) and adds
runtime borrow/disposal panics and a second programming model. C is a language to build and own forever and
can't express editor-grade UI; its real use is a later third-party extension manifest. Precedents consulted:
Dioxus, Leptos, Iced, Slint, Xilem, egui, Tauri, gpui-kit, Floem, VS Code/Theia contribution points, Rails,
SwiftUI, Jetpack Compose.

**The shape.**

- `kiln::app(id, name)` returns an `AppBuilder` whose `run()` applies conventions: paths from the id, the user's
  `settings.toml`/`keymap.toml`/`theme.toml` watched, a `Workspace` window, layout saved and restored, kiln's
  default keys and menus, quit on last window off macOS, debug builds failing fast on the app's own config
  errors. `kiln::init` and `Config` stay underneath as the documented manual path.
- `kiln_shell::Workspace<P: PaneView = ItemPane>`: panes, docks, status bar, palette, saved layout. Generic over
  the pane, so fastedit's `Pane` (`impl PaneView`, `fe-workbench/src/pane.rs` ~826) fits.
- `kiln-macros`: `#[kiln::command]`, `#[kiln::commands]` (on an `impl`, view-scoped), `#[kiln::panel]`,
  `#[kiln::item]`, `#[kiln::status]`, `#[kiln::settings]`, `#[kiln::model(persist)]`, typed `keymap!`/`menus!`.
  Every macro emits plain data plus fn pointers into one `Contributions` registry; each has a documented
  handwritten equivalent. Attribute macros only, so rustfmt and rust-analyzer keep working in the bodies.
- **Registration rule:** `inventory` auto-registration only for the app (binary) crate's own items. Library
  crates expose a `kiln::Module` the app passes to `.module(..)`. The prototype showed a library crate's
  `#[command]` silently missing when nothing referenced the crate; debug builds warn on a `Module` that
  contributes nothing.
- Overrides: `id = ".."`, `context = ".."`, `kind = ".."` keep existing IDs; `AppBuilder` methods override each
  convention; `Workspace::shell()/group()/dock(pos)` expose the parts; `window_root(..)` replaces the workspace.
- Nothing on the paint path is wrapped: `EditorElement`, `TerminalElement` and `FileViewerElement` stay raw
  `impl Element`.

**Plan.** Phase 1, workspace and defaults (no macros; the host example falls under ~400 Rust lines from 929;
fastedit untouched). Phase 2, the `Contributions` registry (registration allowed after init) and `kiln-macros`,
with an `examples/notes` of about 100 lines. Phase 3, fastedit's `Workbench` embeds `Workspace<Pane>` and its
command families move to `#[kiln::commands]` one card each, keeping every command ID and key context
(app-framework.md §4) and every existing test unedited. fastedit's 223-binding keymap stays a TOML table.

**What not to do.** No element macro; no signal arena or fine-grained reactivity; no DSL for app authoring; no
convenience layer on the paint path; macros never mandatory; no renamed command IDs or contexts; no forcing a
large app's curated keymap into attributes; no silent cross-crate `inventory`.

**Risks and mitigations.** Linker magic (explicit `Module`s); macro opacity (attribute macros only,
`quote_spanned!` errors at user code, documented expansions); hiding GPUI from an editor (`Cx` exposes
`window` and `cx`, any `Render + Focusable` works everywhere); GPUI churn (expansions name kiln items, except
the Action impl `commands!` already writes); `inventory` order (sort by file and line, `order = n`, typed
`menus!`); two ways to do everything during migration (the manual API is the documented underneath).

**Prototype evidence.** A GPUI-free prototype (`#[command]` macro, inventory registry, typed `menus!`): a typo
in `menus!` gives `E0425 … similarly named struct NewNote`; a wrong handler signature gives `E0061` at the
user's fn; an unknown attribute key is spanned on the key; a duplicate default key is reported at startup
(`secondary-n is bound to both app.new_note and workbench.save_all`); an unreferenced library crate's command
is silently absent; build cost cold 4.3 s, incremental ~0.2 s. GPUI facts checked in gpui-pre 0.3.7: detached
`Context::observe` callbacks unregister once the observer is released (`app/context.rs:881-893`), so
`cx.watch` needs no `Subscription` storage.

### 2. `docs/decisions/app-framework.md` §3

Replace "There is no kiln `App` type; `&mut App` and `Window` are reachable everywhere." with: the manual path
(`kiln::application()`, `kiln::init`, `Config`, `Root`/`Shell`) stays as documented, and on top of it kiln now
offers `kiln::app(id, name)` and the attribute macros, under the rule quoted above, decided by the owner
2026-10-13; see `kiln-app-layer.md`. Retitle §3 "API: default the composition, wrap nothing on the paint path"
and keep the rest of §3 as is. Add a line to the header noting the amendment and its date.

### 3. kiln's own docs

`kiln/docs/framework.md` §3 and `kiln/README.md` ("Using it") say the same, without naming fastedit
(kiln's docs are written to leave the repository). The README keeps its current example as "the manual path".
