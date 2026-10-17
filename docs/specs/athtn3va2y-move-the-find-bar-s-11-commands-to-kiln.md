---
task_uid: "01m53q0tywyk4fgvathtn3va2y"
task: "T117"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "find_bar.rs no longer has FindCommands, its Commands global or registry(); the 11 find.* commands are methods in find_commands.rs's #[kiln::command(context = \"pane\")] impl Pane, attached on the pane's root element, with the editor-context ones keeping their old predicate (narrowed by the lead: each method keeps its old context: editor, editor || find_bar, or find_bar; the bar element still sets find_bar)"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists the 11 find.* commands (with their editor, find bar or either contexts) with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the find bar's 11 commands to #[kiln::command] methods on the pane, under the find_bar and editor contexts (Mac T702)

Filed on argon from Mac T702 (card ~/coordination/argon-cards/zo/T702.json rev 2; note zo, the kiln phase-3 chain, card 11 of 17, serial). Mac task uid 01m4vbjvste6mqgzchj9prw1zw. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T116 (Mac T701), the card before. Title the host commit "... (Mac T702)".

## Problem

The find bar's 11 commands (`crates/fe-workbench/src/find_bar.rs` ~847-915: `FindCommands`, its `Commands` global, `registry()`, `on_input_command` ~827) dispatch through `fe_command` and run on the pane (`Pane::run_find`, `pane.rs` ~1326): `find.show` (`editor` context), `find.show_replace`, `find.next`, `find.previous` (editor or find bar), and `find.close`, `find.toggle_case`, `find.toggle_whole_word`, `find.toggle_regex`, `find.toggle_replace`, `find.replace_next`, `find.replace_all` (`find_bar`). They aren't the Workbench's, so they don't go in `workbench_commands.rs`.

## Change

New `crates/fe-workbench/src/find_commands.rs` holds `#[kiln::command(context = "find_bar")] impl Pane`, the pane's one `ViewCommands` block, with the 11 commands as methods (old IDs and titles; `context = ".."` on those whose old context was the editor's or either, spelled as the old `ContextPredicate`), each calling `run_find` with its op. `Pane`'s root element attaches them (`Self::attach`), and the find bar's `on_input_command` calls `kiln::commands::dispatch_typed` first as `Workbench::on_run_command` does (T693). Delete `FindCommands`, the `Commands` global and `registry()`, and `find_bar::registry()`'s lines in `lib.rs`. `FindOp` stays (`run_find` takes it). The pane-toolbar toggle that this card's placeholder also named moved to T701.

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
## Lead's note (argon, 2026-10-17, after the worker's block)

C1's literal `#[kiln::command(context = "find_bar")] impl Pane` can't hold without a key-binding regression:
ViewCommands::attach makes the impl's context the key context of the element it's attached to (the pane's root), so
find_bar would cover the whole pane, editor included. Escape in a non-vim editor would then run find.close instead of
editor.single_selection, and alt-c/w/r would toggle find options. C1 is narrowed: the impl's context is `pane` (a frame
nothing else names), and each method keeps its old context (editor, editor || find_bar, or find_bar), while the bar's own
element still sets find_bar. The test the_find_bar_keys_stay_in_the_find_bar shows the difference. terminal_find.rs is
added to the scope: it used find_bar::op, and its row now handles the 11 typed actions through find_commands::handle_in.
typed_command_ids_tests.rs compares contexts with ContextPredicate::parse, so `editor || find_bar` can be listed.

## Implementation notes

- **The impl's context is `pane`, not `find_bar` (C1 deviates from its text).** `ViewCommands::attach` sets the
  impl's context as the key context of the element it's given. On the pane's root, `find_bar` would put the
  whole pane, editor included, in the find bar's keys. Context predicates match any frame in the stack, and a
  later binding wins, so `escape` in a non-vim editor would run `find.close` (keymap line 954) instead of
  `editor.single_selection` (line 197). The same goes for `alt-c`/`alt-w`/`alt-r` and the toggles. The root
  now carries a new `pane` frame instead, which no binding, menu or predicate names. The bar's element still
  sets `find_bar`. Every method gives its own old context: `editor`, `editor || find_bar` or `find_bar`.
  `the_find_bar_keys_stay_in_the_find_bar` fails with `context = "find_bar"` on the impl and passes with
  `pane`.
- **Context guard.** kiln runs a typed handler wherever it's dispatched, but the old registry checked the
  predicate. Each method, and the terminal row's handlers, run only when `kiln::current_invocation`'s stack
  satisfies the command's registered context. An invocation-less raw action runs, as `Workbench::in_editor`
  does. `find_commands_run_only_where_their_context_holds` fails without the guard.
- **`terminal_find.rs` is outside the card's scope but had to change.** The terminal panel's find row ran the
  same `find.*` commands through `find_bar::op`. Keys and menus now dispatch typed actions, and the terminal
  panel isn't inside a pane. So the row attaches handlers for the 11 typed actions
  (`find_commands::handle_in`, with the same guard) mapped to `TerminalPanel::run_find`. Its `RunCommand`
  handler calls `dispatch_typed` first, as the find bar's does.
- `Pane::on_run_command` is gone: a `RunCommand` from the editor reaches `Workbench::on_run_command`'s
  `dispatch_typed`, and one from the find bar's inputs reaches `on_input_command`'s.
- `typed_command_ids_tests.rs`: the 11 rows are appended. The title/context assertion now compares against
  `ContextPredicate::parse(context)` rather than `identifier(context)`, so `editor || find_bar` can be
  listed. For the existing single-identifier rows the two are equal. Two regression tests are added. No other
  test file is touched.
