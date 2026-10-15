---
task_uid: "01m50ewgxqxdpa9ej7b766ddpv"
task: "T90"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/macro_command.rs: a doc-titled #[kiln::command(key = secondary-n, menu = File)] fn new_note in a kiln::testing app with no TOML makes NewNote with ID CRATE.new_note and title New Note, run from its key, the palette and File \u003e New Note"
    required: true
    manual: false
  - id: "C2"
    text: "macro_command.rs: id = workbench.save_all and context = .. override the ID and context, and contributed menu items sort by file and line"
    required: true
    manual: false
  - id: "C3"
    text: "Records from a library target apply only through its kiln::module! Module passed to .module(..); in debug builds unclaimed linked library records log a warning naming the crate (a unit test over the collection with synthetic records)"
    required: true
    manual: false
  - id: "C4"
    text: "compile_fail doctests in kiln/src/lib.rs: a command fn without a doc line, an unknown attribute argument and a wrong handler signature fail to compile"
    required: true
    manual: false
  - id: "C5"
    text: "kiln-macros adds no crate to Cargo.lock beyond itself (syn, quote, proc-macro2, heck and inventory are already there), and the_macro_makes_gpui_actions still passes unedited"
    required: true
    manual: false
---

# Add the kiln-macros crate and #[kiln::command] on fns: one declaration gives a command its type, ID, title, default key, menu item and handler (Mac T680)

Filed on argon from Mac T680 (card ~/coordination/argon-cards/T680.json rev 1; note zh). Mac task uid 01m4vbj9pq1jx34ds194ggbzy5. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Mac deps already on the trunk: Mac T668. Argon deps: T88 (Mac T678). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T680)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). Adding one command today touches four places linked by strings: the
`kiln::commands!` list (`examples/host/commands.rs:12-13`), `keymap.toml` (`keys`, `command = "chat.…"`,
`context`), `menus.toml` (the doc line's title restated, the ID again), and a handler plus
`.on_action(cx.listener(..))` in `render` (`examples/host/app.rs:213-222, 515`). A typo in either TOML file is
only a run-time warning.

A GPUI-free prototype of this macro (syn, quote, inventory) was built during the design: `new_note` →
`NewNote`, ID `<crate>.new_note` from `CARGO_CRATE_NAME`, title from the doc line, `id = ".."` override, the
handler call `quote_spanned!` on the signature so a wrong signature reports `E0061` at the user's fn, an unknown
argument spanned on its key, cold build 4.3 s and incremental ~0.2 s. It also showed a library crate's command
**silently absent** when nothing referenced that crate.

## Change

- **New crate `kiln/crates/kiln-macros`** (proc-macro; `syn` 2, `quote`, `proc-macro2`, `heck`, all already in
  `Cargo.lock`), its licence set in its own `Cargo.toml` as the other kiln crates do, added to the root
  `[workspace.dependencies]` beside the other kiln crates; `kiln` depends on it and on `inventory` (already in the
  lock through gpui) and re-exports the attribute as `kiln::command`.
- **`#[kiln::command(key = "..", menu = "..", id = "..", context = "..")]` on a fn** `fn new_note(ws: &mut
  Workspace, cx: &mut Cx<Workspace>)`: keeps the fn; adds a unit command type named in PascalCase, a real GPUI
  action with kiln metadata exactly as `kiln::commands!` writes it (so `the_macro_makes_gpui_actions` still
  describes it); ID `<crate>.<fn>` unless `id`; title from the first doc line (a compile error without one); and an
  `inventory` record (ID, title, context, keys, menu, file, line, register, run) that `kiln::app` turns into a
  `Contributions::add_command`, the handler running on the window's workspace. Errors are spanned on the user's
  tokens.
- **Collection rule.** Records are collected automatically only when they were compiled into a binary, example
  or integration-test target (the macro records `option_env!("CARGO_BIN_NAME")` and
  `option_env!("CARGO_TARGET_TMPDIR")`, and `CARGO_CRATE_NAME`). A library crate exposes
  `kiln::module!(pub fn module);`, which expands to a `Module` adding that crate's records; the app passes it to
  `.module(..)`. In debug builds, linked library records no module claimed log a warning naming the crate.
  Menus and rails sort by (file, line) within an `order`.
- Rustdoc on the attribute shows its expansion and the handwritten equivalent (`kiln::commands!` plus
  `Contributions::add_command`).

`kiln::commands!` stays as the manual form. New tests go in `kiln/crates/kiln/tests/macro_command.rs`; the
compile-error checks are `compile_fail` doctests on the re-export in `kiln/src/lib.rs` (no trybuild).

## Implementation notes

- `kiln-macros` (`src/lib.rs`, `src/command.rs`) parses the fn and its arguments (`key`, `menu`, `order`,
  `id`, `context`) and emits `::kiln::__command!` (a hidden `macro_rules!` in `kiln/src/lib.rs` that writes
  the unit struct, its `gpui::Action` impl, `impl Command` and GPUI's `MacroActionBuilder` submission exactly
  as `kiln::commands!` does) plus a `const _` block with the register fn, the handler shim and the
  `inventory::submit!` of a `kiln::__private::CommandRecord`. The default action name is
  `concat!(env!("CARGO_CRATE_NAME"), "::", "NewNote")`, so the ID comes from the compiling crate; an `id`
  override's last segment must survive PascalCase → kiln's snake case, checked at compile time and spanned on
  the literal. The handler call is `quote_spanned!` on the fn's name, so a wrong signature is `E0061`/`E0308`
  there; the missing doc line and unknown arguments are spanned on the fn name and the argument's key.
- `contributions.rs`: `CommandRecord` (`inventory::collect!`), `register_recorded` (registers the typed
  command and, once per type, an app-wide `cx.on_action` that runs the fn on the dispatch window's
  workspace after the dispatch, via a new `on_workspace` helper also used by the panel toggle),
  `Contributions::add_recorded` (records whose `CARGO_BIN_NAME` or `CARGO_TARGET_TMPDIR` was set, sorted by
  file then line), `add_crate_commands` (what `kiln::module!` expands to: that crate's library records, the
  crate marked claimed) and `Contributions::warn_unclaimed`. `AppBuilder::start` calls `add_recorded` before
  the modules and `warn_unclaimed` after them in debug builds. The collection rules are pure functions over
  record slices, unit-tested with synthetic records (C3).
- `kiln::command` the attribute and `kiln::command` the module (`kiln_command`) coexist: different namespaces.
- Carried T86 F1/F2 (`toggle_panel`): the toggle hides the dock with `toggle_dock` only when the dock is
  visible on that panel; otherwise `Dock::show` (focus_on_show, rail join), then focus moves to the active item
  if a hidden panel still holds it. Regression tests in `tests/modules.rs` (outside the listed scope, beside
  the module panel tests they reuse): `a_toggle_shows_its_hidden_panel_as_a_rail_click_does` and
  `focus_leaves_the_panel_a_toggle_replaces`, both failing on the old code.
- C4's `compile_fail` doctests run under `cargo test -p kiln --doc`, not the gate's nextest stage.
- `kiln/README.md` and `kiln/docs/framework.md` crate tables don't list `kiln-macros` yet (out of scope).

### Review round 1 (codex eetnrx)

- F1: the deferred handler now runs under the invocation and window captured in the listener, through
  a new `kiln_platform::command_actions::set_dispatch` (scope expansion: a 10-line setter beside the
  private `set_current`/`set_window`), restoring the previous state after; nested dispatch from a handler
  is covered (`a_handler_sees_its_invocation_and_window`).
- F2: the generated handler is the public `kiln::workspace_handler::<A>(fn)` (also the handwritten
  equivalent's), which notifies the observers after the fn unless `dispatch_invocation` already did (a view
  on the focus path handling `A`, the same `is_action_available` check it makes); covered for global-only,
  view-propagates and view-overrides (`observers_hear_of_each_run_once`).
- F3: `MenuSpec` gains `declared: Option<(&'static str, u32)>`, set from the record; menu items sort by
  (menu, order, declared), handwritten (`None`) first within an order, then in the order added. Covered
  across batches (`menu_items_go_by_where_declared_whatever_added_them`).
