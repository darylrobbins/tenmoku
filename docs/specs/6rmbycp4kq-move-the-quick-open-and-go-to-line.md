---
task_uid: "01m53q0tst7w30ym6rmbycp4kq"
task: "T116"
rev: 4
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "picker::toggled and Toggle, toast::dismisses_all, config_problems::shows and pane_toolbar::toggles with their registries are gone, each of their if's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists quick_open.toggle, go_to_line.toggle, toast.dismiss_all, workbench.show_config_problems and view.toggle_breadcrumbs with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited except the one lookup change in config_problems_tests.rs the_command_is_in_the_catalog_and_the_palette that the lead's note 2 allows (title from the typed command instead of command_list(), both assertions kept), and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the quick open and go to line toggles, toast dismissal, the config problems picker and the breadcrumbs toggle to #[kiln::command] methods (Mac T701)

Filed on argon from Mac T701 (card ~/coordination/argon-cards/zo/T701.json rev 2; note zo, the kiln phase-3 chain, card 10 of 17, serial). Mac task uid 01m4vbjv04gxjpt9wzsrdmcg6h. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T115 (Mac T699), the card before. Title the host commit "... (Mac T701)".

## Problem

Four single-command families still dispatch through `fe_command`, each with its own registry and an `if` in `Workbench::on_run_command`: the picker toggles (`crates/fe-workbench/src/picker.rs` ~57: `quick_open.toggle`, `go_to_line.toggle`, matched by `picker::toggled`), toast dismissal (`crates/fe-workbench/src/toast.rs` ~46, ~327: `toast.dismiss_all`, `toast::dismisses_all`), the config problems picker (`crates/fe-workbench/src/config_problems.rs` ~43-80: `workbench.show_config_problems`, `config_problems::shows`) and the pane toolbar toggle (`crates/fe-workbench/src/pane_toolbar.rs` ~159-190: `view.toggle_breadcrumbs`, `pane_toolbar::toggles`, moved here from the former T702).

## Change

Move the 5 commands to methods in `workbench_commands.rs` with their old IDs and titles, calling `toggle_quick_open`, `toggle_go_to_line`, the toast layer's `dismiss_all`, `show_config_problems` and `toggle_breadcrumbs`. Delete `picker::toggled`/`Toggle`, `toast::dismisses_all`, `config_problems::shows` and `pane_toolbar::toggles` with their registries. `config.problems` (the status item's ID, not a command) stays.

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
## Lead's note (argon, 2026-10-17)

T134 (before this card) adds `palette = false` to kiln's `#[command]`, because the old registries kept some commands out
of the palette and typed commands are all listed. If a command this card moves was kept out of the palette before (the
old code's comment names `toast.dismiss_all`), mark it `palette = false`, so the palette lists the same commands as at
the base.

## Lead's note 2 (argon, 2026-10-17, after the worker's block; the same narrow exception as T112's)

C1 and C3 conflicted on config_problems_tests.rs `the_command_is_in_the_catalog_and_the_palette` (~line 338), which
finds workbench.show_config_problems in `crate::command_list()` to assert its title. C3 is narrowed to allow exactly the
change that test needs: keep its catalog check as it is, and take the command's title from the typed command (for
example `kiln::platform::command_actions::typed_command`, as typed_command_ids_tests.rs does) instead of
command_list(), still asserting "Show Config Problems". Don't drop either assertion. config_problems_tests.rs is added
to the scope.
