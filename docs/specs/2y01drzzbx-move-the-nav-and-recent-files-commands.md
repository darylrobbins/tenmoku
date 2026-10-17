---
task_uid: "01m53q0tbjpjspsb2y01drzzbx"
task: "T114"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "nav.rs's and recent_picker.rs's registries and op() functions are gone, run_nav_op and run_recent_op's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists the 4 nav.* commands, workbench.open_recent, workbench.clear_recent and recent.open_in_new_window with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the nav and recent-files commands (back, forward, go to definition, workspace symbols, open and clear recent) to #[kiln::command] methods (Mac T698)

Filed on argon from Mac T698 (card ~/coordination/argon-cards/zo/T698.json rev 2; note zo, the kiln phase-3 chain, card 8 of 17, serial). Mac task uid 01m4vbjrkq2ds0k2jewnam17fd. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T113 (Mac T697), the card before. Title the host commit "... (Mac T698)".

## Problem

The `nav` family (`crates/fe-workbench/src/nav.rs` ~787-845: `NavOp`, its registry, `op()`) with its `run_nav_op` arm, and the recent-files family (`crates/fe-workbench/src/recent_picker.rs` ~53-140: `RecentOp`, `OPEN_RECENT`, `CLEAR_RECENT`, `OPEN_IN_NEW_WINDOW`, its registry, `op()`) with its `run_recent_op` arm (`workbench.rs` ~2101) still dispatch through `fe_command`: `nav.back`, `nav.forward`, `nav.goto_definition`, `nav.workspace_symbols`, `workbench.open_recent`, `workbench.clear_recent` and `recent.open_in_new_window` (the last in the recent picker's context).

## Change

Move the 7 commands to methods in `workbench_commands.rs` with their old IDs, titles and contexts, calling what `run_nav_op` and `run_recent_op` call. The nav state (`Nav`), its buttons and `NavOp` where the toolbar's back/forward buttons use it stay.

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

- `NavOp` and `Workbench::run_nav_op` stay: the existing `tests/nav_pending.rs` calls them. The four `nav.*`
  methods run them. `RecentOp` and `Workbench::run_recent_op` stay too: vim's `:oldfiles` runs
  `RecentOp::Open`. `recent_picker` became `pub(crate)` in `workbench.rs` so `workbench_commands.rs` can name
  `RecentOp`.
- `OPEN_IN_NEW_WINDOW` stays, `#[cfg(test)]`, for `recent_picker_tests.rs`. `OPEN_RECENT` and `CLEAR_RECENT`
  are gone, along with `recent_picker::init`. That init put the two workbench commands in the palette, and the
  palette now lists typed commands on its own.
- Change in behaviour: the palette lists a typed command wherever its context holds, so it lists "Open Recent
  in New Window" when it opens over a picker. The registry used to keep that command out. Choosing it does
  nothing unless the recent picker shows (the method propagates, as before). kiln's `#[command]` has no
  option to keep a command out of the palette.
