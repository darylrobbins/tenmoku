---
task_uid: "01m53q0r3khsk23bepqgfcpsk3"
task: "T107"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "workspace_activate_move.rs: Workspace::activate makes a pane in the group active and focused and emits ActiveChanged (nothing for a pane outside it), and Workspace::move_tab to an edge splits off a pane that later drops and closes are followed for (it is watched), emits PaneAdded, and closes the emptied source"
    required: true
    manual: false
  - id: "C2"
    text: "module_before_init.rs: a library crate's kiln::module! fn called in an app where kiln::init never ran makes typed_command find its #[kiln::command] methods and dispatch_invocation run one, and when init runs afterwards each is registered once"
    required: true
    manual: false
  - id: "C3"
    text: "run_command_typed.rs: kiln::commands::dispatch_typed runs a view method for a RunCommand naming a typed command with that view focused, keeping the invocation's context, and returns false and runs nothing for an ID that isn't typed; recorded_catalog.rs: kiln::recorded_catalog(krate) knows that crate's #[kiln::command] IDs with no app, rejects args for them and reports others unknown"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p kiln -p kiln-shell -p kiln-platform passes with no existing kiln test edited, and the Kiln Chat host example builds unchanged"
    required: true
    manual: false
---

# Let kiln's Workspace<P> activate a pane and move a tab by key, and let a library's typed commands register, run from a RunCommand and be checked without kiln::init (Mac T700)

Filed on argon from Mac T700 (card ~/coordination/argon-cards/zo/T700.json rev 4; note zo, the kiln phase-3 chain, card 1 of 17, serial). Mac task uid 01m4vbjtac0xkjg150h3xx6v2g. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Title the host commit "... (Mac T700)".

## Problem

Phase 3 moves fastedit's `Workbench` onto `kiln_shell::Workspace<Pane>` and its commands onto `#[kiln::command]`.
Reading kiln on main (e776ee0e, the kiln app layer T669-T689) against fastedit found four gaps that block it,
all in kiln:

1. `Workspace<P>` has no public way to make a pane active (`activate` is fastedit's, `workbench.rs` ~1458, called
   from nav, problem landing and the panels) or to move a tab by key: its `move_tab` path is private to
   `tab_dropped` (`kiln/crates/kiln-shell/src/workspace.rs` ~478), and a host using `group_mut().move_tab` skips
   `watch` for a split-off pane. fastedit's `pane.move_tab_*` keys need both.
2. A library crate's `#[kiln::command]`s register only once `kiln::init` has run: `Contributions::add_commands`
   queues them while `resolver` is unset (`kiln/crates/kiln/src/contributions.rs` ~890). fastedit's test apps call
   `fe_gpui::init` and `fe_workbench::init` without `kiln::init` (`crates/fe-workbench/src/tests/mod.rs` ~71), so
   `typed_command` would never find fe-workbench's commands there.
3. fastedit's tests dispatch commands as a raw `RunCommand(Invocation)` with `window.dispatch_action` (139 sites,
   e.g. `crates/fe-workbench/src/dock_position_tests.rs` ~59). A raw `RunCommand` naming a typed command never
   reaches the typed action: only `dispatch_invocation` builds it (`kiln-platform/src/command_actions.rs` ~186).
4. `fe_workbench::command_catalog()` takes no `cx`: about 20 callers check keymaps and menus against it without
   an app (`crates/fastedit/tests/menus.rs`, `crates/fe-workbench/src/tests/default_keymaps.rs` ~49, ~129, and
   others). `kiln::catalog(cx)` (`kiln/crates/kiln/src/setup.rs` ~632) needs one, so a typed command's ID would
   read as unknown there.

## Change

- `Workspace<P>::activate(&mut self, pane: Entity<P>, window, cx)`: makes a pane already in the group the active
  one, focuses its active item (or the pane while empty), follows the titles and emits `ActiveChanged`; nothing
  for a pane not in the group.
- `Workspace<P>::move_tab(&mut self, source: &Entity<P>, item: ItemId, destination: Destination<P>, window, cx)
  -> Option<Entity<P>>`: what `tab_dropped` does now (moves it, watches a split-off pane and emits `PaneAdded`,
  makes the target active), plus closing `source` if that emptied it; `tab_dropped` calls it.
- A library module's commands register at once when `kiln::init` hasn't run: `add_commands` registers each
  spec's action and typed record immediately (what `spec.register` does) whenever kiln-platform's command global
  exists, and `init` doesn't register them a second time; the keymap and menu checks still happen at `init`.
- `kiln_platform::commands::dispatch_typed(run: &RunCommand, window: &mut Window, cx: &mut App) -> bool`: if
  `run.0` names a registered typed command, dispatches it through `dispatch_invocation` (keeping its context and
  source) and returns true; false otherwise, for the caller to go on. Re-exported as `kiln::commands::dispatch_typed`.
- `kiln::recorded_catalog(krate: &'static str) -> Box<dyn CommandCatalog>`: a catalog, needing no app, of the
  `#[kiln::command]`s `inventory` recorded for that crate: it knows their IDs and accepts them with no args
  (`Value::Null`), as their unit actions build.

New tests, one file each: `kiln/crates/kiln-shell/tests/workspace_activate_move.rs`,
`kiln/crates/kiln/tests/module_before_init.rs`, `kiln/crates/kiln-platform/tests/run_command_typed.rs`, `kiln/crates/kiln/tests/recorded_catalog.rs`.

Land after argon's batch 17 (Mac T688/T689, 7c7a242) is on main: it doesn't touch these files, but the kiln
README and `framework.md` it adds should mention `activate`, `move_tab` and `dispatch_typed` only if this card
changes them: it doesn't need to.

## Rules for every card of this chain

Phase 3 of the Kiln app layer (`docs/decisions/kiln-app-layer.md`, approved 2026-10-13). The chain runs serially,
in this order: T700 (kiln), T690, T691, T693, T694, T695, T697, T698, T699, T701, T702, T703, T692, T696, T704,
T705, T706.

- No command ID or key context is renamed (`app-framework.md` §4): every moved command keeps its ID with
  `id = ".."` and its context with `context = ".."`.
- No existing fastedit test is edited; behaviour is proven by the existing suites plus the new test file each
  card names.
- Nothing on the paint path changes: `EditorElement`, `TerminalElement` and `FileViewerElement` stay raw
  `impl Element`.
- The keymap and menus stay TOML tables (`crates/fe-keymap/keymaps/*.toml`, `menus/*.toml`), untouched.
- `crates/fastedit/src/main.rs` is not touched: `fe_workbench::init` registers what the chain adds (neon's T707
  owns `main.rs` and the keymap files).

## Implementation notes

- `dispatch_typed` dispatches the typed action once the current dispatch is over (`cx.defer`, then
  `dispatch_invocation` in the same window): called from a view's `RunCommand` listener, that view is leased
  for the dispatch, so its own handler of the typed action would panic ("already being updated") if run
  nested. It still returns true or false at once; a typed command whose args don't build logs and returns true.
- "kiln-platform's command global exists" is `kiln_platform::commands::initialized(cx)` (new, public).
  `add_commands` registers when that or `init` has run; a spec registered early is dropped from the list
  `register_early` drains, so `init` registers it once.
- `recorded_catalog(krate)` matches records by `krate` alone, the app's own targets' too, so an integration
  test can check its own `#[kiln::command]`s; it rejects any args other than `Null`.
- `module_before_init.rs` can't link a real library crate's records (an integration test's are the app's),
  so it submits `CommandRecord`s for its `#[kiln::command]` methods as the macro writes them in a library
  crate named `notes_lib`, and calls `add_crate_commands(cx, "notes_lib")`, what `kiln::module!` expands to.
