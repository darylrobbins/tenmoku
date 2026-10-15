---
task_uid: "01m50ewgnvydcn4hcywxrggsmq"
task: "T86"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/modules.rs: a module adding a panel, an item kind, a status source and a command, passed to .module(..), gives each workspace window that panel in its dock with a Toggle command on its key, in the palette and the View menu, restores that item kind and draws the status item"
    required: true
    manual: false
  - id: "C2"
    text: "modules.rs: panels sort by order, then registration order, in the dock and its rail"
    required: true
    manual: false
  - id: "C3"
    text: "modules.rs: .contributions(|c| ..) does the same inline, and a module that contributes nothing logs a warning naming it in debug builds"
    required: true
    manual: false
  - id: "C4"
    text: "Each Contributions add_ method's rustdoc example compiles (doc tests pass)"
    required: true
    manual: false
---

# Add panels, item kinds, status items, settings and models to kiln's Contributions, and kiln::Module for library crates (Mac T677)

Filed on argon from Mac T677 (card ~/coordination/argon-cards/T677.json rev 1; note zh). Mac task uid 01m4vbj7f90mqd69qanfb8b2xn. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Mac deps already on the trunk: Mac T673, Mac T672. Mac lead's note: today's Contributions (Mac T673) is an App global whose adders are associated fns taking cx (Contributions::add_command(cx, spec)); fit the spec's Module and add_* to that without changing add_command's signature (tests/contributions.rs is outside the scope). The criteria are what count. Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T677)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). The previous card added `kiln::Contributions` with commands. The macros
also contribute panels, item kinds, status items, settings sections and models, and library crates must
contribute explicitly: the approved rule is *`inventory` auto-registration only within the app crate; library
crates register via an explicit `.module(..)`*, because the macro prototype found a library crate's
registrations silently missing when nothing referenced the crate.

## Change

In `kiln/crates/kiln/src/contributions.rs` and `kiln/crates/kiln/src/app_builder.rs`:

- `Contributions::add_panel(PanelSpec { id, title, icon, position, order, toggle_key, make })`, where `make`
  builds the panel (`fn(&mut Window, &mut App) -> AnyDockPanel`, a boxed `Entity<impl DockItem>`). Each panel also
  contributes a toggle command `<id>.toggle` titled `Toggle <title>`, bound to `toggle_key` if given, in the View
  menu. `kiln::app` adds every contributed panel to each workspace window, sorted by `order`, then registration
  order; toggling goes through `Workspace::toggle_dock`, so focus leaves a hidden panel.
- `add_item_kind(fn(&mut App))` (the `layout::register_item::<T>` call), `add_status(StatusSpec { side, order,
  item: fn(&App) -> Option<StatusItem> })` (a workspace status source), `add_settings(fn(Config) -> Config)`
  (adds a settings store before init) and `add_model(fn(&mut App))`.
- `kiln::Module` is `fn(&mut Contributions)`. `AppBuilder::module(m)` applies it in `start` before init;
  `AppBuilder::contributions(|c| ..)` is the inline form. In debug builds a module that contributes nothing logs a
  warning naming it (`std::any::type_name_of_val`).
- Rustdoc on each `add_*` shows the handwritten form, as the macros' "underneath".

New tests go in `kiln/crates/kiln/tests/modules.rs`.

## Implementation notes

- `kiln::Module` is `fn(&mut App)`, not `fn(&mut Contributions)`, following the lead's note: every
  `Contributions::add_*` is an associated fn taking `cx`, as `add_command` already was, so a module
  calls `Contributions::add_panel(cx, PanelSpec { .. })` and the inline form is
  `.contributions(|cx| ..)`. `AppBuilder::module` takes any `FnOnce(&mut App)` so that
  `type_name_of_val` names the fn item (`notes::module`) in the debug warning; only `.module(..)`
  warns, not the inline form.
- A panel's toggle command `<id>.toggle` is registered in the app's command registry (so the
  palette lists it and the keymap and menus check against it) by `init`, before the shipped catalog
  shares the registry. So `add_panel` and `add_settings` work before `init` only; after it they log a
  warning and add nothing. Item kinds and models added before `init` run at its end (from
  `contributions::start`), and at once after it.
- The toggle shows the dock with the panel, or calls `Workspace::toggle_dock` when the panel is the
  one the dock shows.
- `StatusSpec`'s `side` and `order` sort the sources as they're added; the item's own side and
  priority still place it on the bar (`Workspace::add_status_source` takes a plain `fn`).
- `lib.rs` gains one `pub use` line for the new types (outside the listed scope, unavoidable).
- An app calling `kiln::init` by hand applies contributed settings with
  `Contributions::configure(cx, config)`.
- C4: `cargo test --doc -p kiln` runs (nextest doesn't run doc tests): the examples on `Module` and
  every `add_*` pass.
