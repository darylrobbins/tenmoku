# Kiln app layer: default the composition, wrap nothing on the paint path

Status: **build**. Approved by the owner 2026-10-13: "I approve the new kiln
approach. Let's draft cards for the work and start working on it." Amends
`app-framework.md` §3, which said "There is no kiln `App` type". The proposal
was a design note of 2026-10-13 against main @ d5ed50eb; this record is its
home in the repository.

## Decision

Re-envision Kiln in the *composition layer*, not the rendering model. Option A
("batteries plus attribute macros over today's Kiln") plus two small pieces of
a reactive layer:

- `Cx<V>`, a window-plus-context bundle that derefs to `Context<V>`, with
  `pub window` and `pub cx`;
- tracked `cx.watch::<Model>()`, which replaces stored `Subscription`s.

No `rsx!`/`view!` element macro, no Copy-signal arena, no `.kiln` DSL.

## The rule

Replaces "extend GPUI, never wrap it":

> Default the composition; wrap nothing on the paint path. Every convention
> has a one-line override, and every sugar has a GPUI-shaped escape hatch.

## Registration rule

`inventory` auto-registration works only for the app (binary) crate's own
items. Library crates register through an explicit `kiln::Module` that the app
passes to `.module(..)`. The prototype found that a library crate's
`#[command]` is silently missing when nothing references the crate, so no
cross-crate `inventory`. Debug builds warn on a `Module` that contributes
nothing.

## Diagnosis

What an app author must handle by hand today:

- **The composition tax.** `kiln/crates/kiln/examples/host/app.rs` is 566
  lines, about 400 of them generic workspace glue (`watch`, `sync_peers`,
  `pane_changed`, `tab_dropped`, `close_pane`, `follow_active`, the `Panes`
  centre view, the status bar view). fastedit has a near-verbatim copy
  (`PaneArea`, `crates/fe-workbench/src/workbench.rs` ~2134).
- **The Shell vs Root cliff.** The first dock panel forces a rewrite from
  `Root` onto `Shell`, `DockState`, `Dock`, `Capabilities` and
  `shortcut_contexts`. `Shell::new`'s `UiTheme` argument is ignored
  (`shell.rs:83`).
- **Subscription lifetimes and `cx.notify()` relays.** `[Subscription; N]`
  arrays in every view; fastedit's `Workbench` has 54 fields, 12 of them
  `Subscription`s.
- **One command in four places, linked by strings:** the `commands!` list,
  `keymap.toml`, `menus.toml`, and the handler plus
  `.on_action(cx.listener(..))`. fastedit's command families each add an `Op`
  enum, a `CommandTarget`, a `Global` registry, `init`/`registry()`/`op()`,
  and an arm in `on_run_command`'s 15-arm chain (`workbench.rs` ~1979-2059),
  listed again in `command_catalog()` and `command_list()`
  (`crates/fe-workbench/src/lib.rs` ~153-200). Every app copies 30 lines of
  picker keys.
- **Persistence by hand and untyped:** `LayoutStore`, `WindowLayout`, quit and
  close hooks, `Value` encoding.
- **Per-app paths, user files, settings plumbing and stringly theme tables.**
- **Closed extension points** (fastedit's `enum DockPanel` with five parallel
  `match` tables) and test-only accessors
  (`#[allow(dead_code, reason = "for tests/host.rs")]`).

## Options compared

The same notes app: a sidebar, a command with key and menu, a status item, one
setting, persistence.

| | Today | A: batteries + macros | B: reactive layer | C: full DSL |
|---|---|---|---|---|
| Code | ~800 Rust + ~65 TOML, 10 files | ~103 Rust, 0 TOML, 4 files | ~72 Rust | ~26 DSL + 13 Rust |
| Escape to GPUI | n/a | everywhere | partial, two models | at the boundary only |
| Effort | n/a | 6–8 weeks kiln + 3–5 fastedit | A + 5–8 weeks | 4–6+ months + tooling |
| Risk to fast paths | n/a | none (same GPUI calls) | low if confined | none, but splits the codebase |

B buys nothing on GPUI's per-view rendering (fine-grained signals need a
retained tree; GPUI has none) and adds runtime borrow/disposal panics and a
second programming model. C is a language to build and own forever and can't
express editor-grade UI; its real use is a later third-party extension
manifest. Precedents consulted: Dioxus, Leptos, Iced, Slint, Xilem, egui,
Tauri, gpui-kit, Floem, VS Code/Theia contribution points, Rails, SwiftUI,
Jetpack Compose.

## The chosen shape

- `kiln::app(id, name)` returns an `AppBuilder` whose `run()` applies
  conventions: paths from the id, the user's `settings.toml`, `keymap.toml`
  and `theme.toml` watched, a `Workspace` window, layout saved and restored,
  kiln's default keys and menus, quit on last window off macOS, debug builds
  failing fast on the app's own config errors. `kiln::init` and `Config` stay
  underneath as the documented manual path.
- `kiln_shell::Workspace<P: PaneView = ItemPane>`: panes, docks, status bar,
  palette, saved layout. Generic over the pane, so fastedit's `Pane`
  (`impl PaneView`, `fe-workbench/src/pane.rs` ~826) fits.
- `kiln-macros`: `#[kiln::command]`, `#[kiln::commands]` (on an `impl`,
  view-scoped), `#[kiln::panel]`, `#[kiln::item]`, `#[kiln::status]`,
  `#[kiln::settings]`, `#[kiln::model(persist)]`, typed `keymap!`/`menus!`.
  Every macro emits plain data plus fn pointers into one `Contributions`
  registry; each has a documented handwritten equivalent. Attribute macros
  only, so rustfmt and rust-analyzer keep working in the bodies.
- Registration follows the rule above: `inventory` for the app crate's own
  items, explicit `kiln::Module`s for library crates.
- Overrides: `id = ".."`, `context = ".."`, `kind = ".."` keep existing IDs;
  `AppBuilder` methods override each convention;
  `Workspace::shell()/group()/dock(pos)` expose the parts;
  `window_root(..)` replaces the workspace.
- Nothing on the paint path is wrapped: `EditorElement`, `TerminalElement` and
  `FileViewerElement` stay raw `impl Element`.

## Plan

1. **Workspace and defaults** (no macros). The host example falls under ~400
   Rust lines from 929; fastedit is untouched.
2. **The `Contributions` registry** (registration allowed after init) and
   `kiln-macros`, with an `examples/notes` of about 100 lines.
3. **fastedit adopts it.** `Workbench` embeds `Workspace<Pane>` and its
   command families move to `#[kiln::commands]`, one card each, keeping every
   command ID and key context (`app-framework.md` §4) and every existing test
   unedited. fastedit's 223-binding keymap stays a TOML table.

## What not to do

- No element macro.
- No signal arena or fine-grained reactivity.
- No DSL for app authoring.
- No convenience layer on the paint path.
- Macros never mandatory.
- No renamed command IDs or contexts.
- No forcing a large app's curated keymap into attributes.
- No silent cross-crate `inventory`.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Linker magic | explicit `Module`s for library crates |
| Macro opacity | attribute macros only, `quote_spanned!` errors at user code, documented expansions |
| Hiding GPUI from an editor | `Cx` exposes `window` and `cx`; any `Render + Focusable` works everywhere |
| GPUI churn | expansions name kiln items, except the `Action` impl `commands!` already writes |
| `inventory` order | sort by file and line, `order = n`, typed `menus!` |
| Two ways to do everything during migration | the manual API is the documented underneath |

## Prototype evidence

A GPUI-free prototype (`#[command]` macro, inventory registry, typed
`menus!`):

- a typo in `menus!` gives `E0425 … similarly named struct NewNote`;
- a wrong handler signature gives `E0061` at the user's fn;
- an unknown attribute key is spanned on the key;
- a duplicate default key is reported at startup (`secondary-n is bound to
  both app.new_note and workbench.save_all`);
- an unreferenced library crate's command is silently absent (hence the
  registration rule);
- build cost: cold 4.3 s, incremental ~0.2 s.

GPUI facts checked in gpui-pre 0.3.7: detached `Context::observe` callbacks
unregister once the observer is released (`app/context.rs:881-893`), so
`cx.watch` needs no `Subscription` storage.
