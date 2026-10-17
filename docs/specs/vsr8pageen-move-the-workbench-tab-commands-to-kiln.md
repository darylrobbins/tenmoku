---
task_uid: "01m53q0rttvg5czvvsr8pageen"
task: "T110"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "new_registry, the Commands global, WorkbenchCommands, PaneTarget and PaneCommand in lib.rs are gone, the registry dispatch's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "workbench_commands_tests.rs: workbench.close_tab, workbench.next_tab and workbench.previous_tab each run from their default keys, from a RunCommand dispatched as the existing tests do, and from the command palette, and command_catalog() accepts all three with no app"
    required: true
    manual: false
  - id: "C3"
    text: "typed_command_ids_tests.rs lists the three tab commands with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the workbench tab commands to #[kiln::command] methods, setting up fe-workbench's kiln module, its one Workbench command block and the RunCommand bridge (Mac T693)

Filed on argon from Mac T693 (card ~/coordination/argon-cards/zo/T693.json rev 3; note zo, the kiln phase-3 chain, card 4 of 17, serial). Mac task uid 01m4vbjmff34knnf2t2ce81gda. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T109 (Mac T691), the card before. Held in the inbox: waits on Mac T707 (neon's zv, Cargo.lock) reaching argon through a later Mac stack and sync; the Mac lead clears it. Title the host commit "... (Mac T693)".

## Problem

fastedit's commands live in about 17 `fe_command` registries, each with an `Op` enum, a `CommandTarget`, a
`Global`, `init`/`registry()`/`op()`, an arm in `Workbench::on_run_command`'s chain
(`crates/fe-workbench/src/workbench.rs` ~2512-2594), and lines in `command_catalog()`/`command_list()`
(`crates/fe-workbench/src/lib.rs` ~162-219). The first and smallest is the tab commands' `new_registry`
(`lib.rs` ~128-160): `workbench.close_tab`, `workbench.next_tab`, `workbench.previous_tab`, run on the active pane.
This card moves them to `#[kiln::command]` and sets up what every later family card reuses.

## Change

- `crates/fe-workbench/Cargo.toml` depends on `kiln` (the macros expand to `::kiln::..` paths).
- `lib.rs` declares `kiln::module!(pub(crate) fn module);` and `init` calls `module(cx)` first, so the crate's
  typed commands register in the app and in the test apps that never run `kiln::init` (T700).
- New `crates/fe-workbench/src/workbench_commands.rs` holds the one `#[kiln::command(context = "workbench")]
  impl Workbench` block, with `close_tab`, `next_tab` and `previous_tab` (`#[command(id = "workbench.close_tab")]`
  and so on; doc lines `Close Tab`, `Next Tab`, `Previous Tab`), each calling the active pane as `new_registry`'s
  closures do.
- `Workbench`'s root element in `render` is `Self::attach(div(), cx)`, which adds the commands' handlers (and the
  `workbench` key context, as the shell's element already has).
- `on_run_command` first calls `kiln::commands::dispatch_typed(run, window, cx)` (T700) and returns if it took
  the command, so a `RunCommand` naming a typed command (the existing tests' dispatch) still runs it.
- Delete `new_registry`, the `Commands` global, `WorkbenchCommands`, `PaneTarget`, `PaneCommand`, the registry
  dispatch at the end of `on_run_command`, and the `new_registry()` lines in `command_catalog()` and
  `command_list()`: the palette lists typed commands itself, and `command_catalog()` gains
  `kiln::recorded_catalog("fe_workbench")` (T700) so its app-less callers (`crates/fastedit/tests/menus.rs`,
  `tests/default_keymaps.rs`) still know the moved IDs.
- New `crates/fe-workbench/src/typed_command_ids_tests.rs`: a table of `(id, title, context)` that later cards
  extend, checking each is a registered typed command with that title and context, and no `fe_command` registry
  in `command_catalog()` still claims it. New `crates/fe-workbench/src/workbench_commands_tests.rs` for the
  dispatch paths.

## How a command family moves (T693 sets this up; every later family card repeats it)

- Each command becomes a method marked `#[command(id = "<old id>")]` in the one
  `#[kiln::command(context = "workbench")] impl Workbench` block in `crates/fe-workbench/src/workbench_commands.rs`.
  A view gets one `ViewCommands` impl, so every family's Workbench methods go in that block; a method's body calls
  the family's existing function (`self.run_pane_op(..)`'s body, moved or called). Its doc line is the old title;
  a command whose old context wasn't `workbench` gives `context = ".."` on the method.
- Delete the family's `CommandTarget`, `Commands` global, `init`/`registry()`/`op()`, its arm in
  `Workbench::on_run_command` (`workbench.rs`), and its lines in `command_catalog()` and `command_list()`
  (`lib.rs`). Its `Op` enum goes too unless something other than command dispatch uses it (a rail click, a
  toolbar button): then it stays, only the registry goes.
- Append the family's IDs, titles and contexts to the table in `crates/fe-workbench/src/typed_command_ids_tests.rs`.

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

- `tests/lsp.rs` and `tests/lsp_quit.rs` (not editable here) ask `cx.has_global::<crate::Commands>()` to tell
  whether `init` has run. With the registry's `Commands` global gone, `lib.rs` keeps that name as a
  `#[cfg(test)]` alias of `kiln::Contributions`, the global `module(cx)`, `init`'s first call, sets. A later
  card can drop it once those tests ask for something else.
- `command_catalog()` is `kiln::recorded_catalog("fe_workbench")` followed by a new `registry_catalogs()`, the
  registries' catalogs it listed before. `typed_command_ids_tests.rs` checks no catalog in `registry_catalogs()`
  claims a moved ID; later family cards delete their line there.
- The tab commands' handler, `Workbench::on_active_pane` (`workbench_commands.rs`), reaches the active pane
  through the public `Workbench::pane()`, so the module sits beside `workbench` in `lib.rs`, not inside it.
