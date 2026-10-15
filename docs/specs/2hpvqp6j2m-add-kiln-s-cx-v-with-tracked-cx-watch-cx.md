---
task_uid: "01m50ewgt8z1d3m82hpvqp6j2m"
task: "T88"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/cx.rs: a view reading a model with cx.watch redraws after cx.update_model changes it; watching it on every one of 10 renders registers one observer; once the view is released the observer is gone"
    required: true
    manual: false
  - id: "C2"
    text: "cx.rs: cx.model reads without observing: the view does not redraw when only that model changes"
    required: true
    manual: false
  - id: "C3"
    text: "cx.rs: cx.setting::\u003cS\u003e() gives the loaded settings and the view redraws after the settings file reloads"
    required: true
    manual: false
  - id: "C4"
    text: "cx.rs: cx.workspace() inside a kiln::app window gives its workspace; cx.service::\u003cT\u003e() gives what kiln::provide stored and panics naming T if nothing was"
    required: true
    manual: false
  - id: "C5"
    text: "Cx derefs to Context\u003cV\u003e and exposes window and cx (a doc test calls a Context method and a Window method through it)"
    required: true
    manual: false
---

# Add kiln's Cx<V> with tracked cx.watch, cx.model, cx.update_model, cx.setting, cx.handler, cx.workspace and cx.service, so views store no Subscriptions (Mac T678)

Filed on argon from Mac T678 (card ~/coordination/argon-cards/T678.json rev 1; note zh). Mac task uid 01m4vbj80edyreajqvsbsrzadh. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Mac deps already on the trunk: Mac T674. Argon deps: T86 (Mac T677). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T678)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). The approval includes two small pieces of a reactive layer, and no more:
`Cx<V>` and tracked `cx.watch::<Model>()`. Today every reaction to another entity is a `Subscription` the author
stores in a correctly sized array (`examples/host/panel.rs:24` `[Subscription; 2]`, `conversation.rs:47`;
fastedit's `Workbench` has 12 `Subscription` fields), and state that other views show is relayed by hand
(`examples/host/app.rs:399, 412`: `self.status_bar.update(cx, |_, cx| cx.notify())`). GPUI already drops a
detached observer once the observer entity is released (`invoke_observer` returns false,
`gpui-pre-0.3.7/src/app/context.rs:881-893`), so a tracked watch needs no storage beyond de-duplication.

## Change

New `kiln/crates/kiln/src/cx.rs` and `kiln/crates/kiln/src/model.rs`, re-exported from `lib.rs`:

- `Cx<'a, 'b, V> { pub window: &'a mut Window, pub cx: &'a mut Context<'b, V> }`, `Deref`/`DerefMut` to
  `Context<V>`, so all of GPUI stays one field away. `kiln::with_cx(window, cx, |cx| ..)` builds one inside a
  handwritten `Render::render`.
- **Models**: app-wide state keyed by type, each an `Entity<M>` kiln holds. `cx.model::<M>() -> &M` reads
  without observing; `cx.update_model(|m: &mut M, cx| ..)` mutates and notifies; `cx.watch::<M>() -> &M` reads
  and makes the current view redraw when `M` changes, through a detached observer de-duplicated per (observer,
  model) so a view watching every render registers once. `kiln::model::register::<M: Default>(cx)` creates one
  (and `Contributions::add_model` can call it); reading an unregistered model creates it from `Default`.
- `cx.setting::<S>() -> &S`: the settings, watched the same way through `GlobalSettings<S>`, so the view redraws
  after a reload.
- `cx.handler(|view, cx: &mut Cx<V>| ..)`: a listener for `on_click` and the like, with a `Cx`.
- `cx.workspace() -> Entity<Workspace<ItemPane>>` for a view in a `kiln::app` workspace window (`kiln::app` records
  each window's workspace); `cx.service::<T>() -> &T` for an app-wide value provided with `kiln::provide(cx,
  value)`, panicking with the type's name if none was. Update `app_builder.rs` to record the workspace.

New tests go in `kiln/crates/kiln/tests/cx.rs`.

## Implementation notes

- gpui-pre 0.3.7 records every entity a window's draw reads and redraws that window when one of them notifies
  (`App::notify`, `window_invalidators_by_entity`). So a model read by an uncached view still redraws its
  window; what `cx.watch` adds is notifying the view itself, which a cached view (`AnyView::cached`) needs to
  render again. `tests/cx.rs` draws its views cached for that reason, and counts both renders and the view's
  notifications.
- Kiln holds each watch's `Subscription` (one per view and watched type, in the `Watches` global) rather than
  detaching it, and drops them when the view is released (`Context::on_release`), so "the observer is gone" is
  immediate rather than at the model's next notify. `kiln::model::watchers::<T>(cx)` counts the views watching
  `T` (a model, or `GlobalSettings<S>`).
- `Cx::handler` is generic over a named `F` with `+ use<V, E, F>`: in edition 2024 a returned `impl Fn` would
  otherwise capture `Cx`'s lifetimes and could not be `'static`.
- `Cx::workspace` falls back to the window's root view when `kiln::app` didn't record the window (a workspace
  opened by hand).
- Carried T86 F3: `AppBuilder::start` now snapshots contributed panels and statuses (and checks it has a window)
  after `init` and the item kinds, and `register_early` registers panel toggles after draining command
  registrations, so a panel contributed while registering a command has its toggle. Regression test in
  `tests/modules.rs` (`what_inits_callbacks_contribute_reaches_each_window`), outside the card's scope list.
