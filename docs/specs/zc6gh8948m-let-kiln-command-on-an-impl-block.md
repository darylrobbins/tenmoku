---
task_uid: "01m50ewh1gzc18tyzc6gh8948m"
task: "T92"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/macro_view_commands.rs: a method command runs from its key and menu only while focus is inside the view, and the palette lists it only there"
    required: true
    manual: false
  - id: "C2"
    text: "macro_view_commands.rs: attach sets the key context around the view; a handwritten Render calling Self::attach behaves the same; a view without ViewCommands is unaffected by kiln::__private::attach"
    required: true
    manual: false
  - id: "C3"
    text: "macro_view_commands.rs: IDs default to CRATE.METHOD, id and context override them, and unmarked methods in the impl are left alone"
    required: true
    manual: false
  - id: "C4"
    text: "compile_fail doctests: a wrong method signature and a #[command] method outside a #[kiln::command] impl fail to compile"
    required: true
    manual: false
---

# Let #[kiln::command] on an impl block declare a view's own commands under its key context, attached wherever kiln hosts the view (Mac T682)

Filed on argon from Mac T682 (card ~/coordination/argon-cards/T682.json rev 1; note zh). Mac task uid 01m4vbjbyn3q0d0f5yyhxjjtzg. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T91 (Mac T681). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T682)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). Commands that act on a view (rename the focused note, send this
conversation's message) need a key context set around the view and an `.on_action(cx.listener(..))` per command
in its `render`, as `examples/host/conversation.rs` does. The approved design calls this `#[kiln::commands]` on
an `impl` block, SwiftUI-`@FocusedObject`-like: the command runs where focus is.

`kiln::commands!` (the manual, function-like form, used across kiln's tests and examples) already owns the name
`commands` in kiln's macro namespace, and an attribute can't share it. So the impl-block form is the same
attribute as the fn form, `#[kiln::command]`, applied to an `impl`.

## Change

In `kiln/crates/kiln-macros` (a new `src/view_commands.rs`) and a new `kiln/crates/kiln/src/view.rs`:

```rust
#[kiln::command(context = "note")]       // sets .key_context("note") around the view
impl NoteView {
    /// Rename Note
    #[command(key = "f2", menu = "Edit")]
    fn rename(&mut self, cx: &mut Cx<Self>) { /* … */ }
}
```

- On an `impl` block, `#[kiln::command]` takes `context` (required) and finds methods marked `#[command(key, menu,
  id)]`; each gets a command type (PascalCase of the method), ID `<crate>.<method>` unless `id`, its title from
  the doc line, and a contribution with `context` set. Unmarked methods are left alone.
- It implements `kiln::ViewCommands for NoteView` (in `view.rs`): `CONTEXT` and `attach(element, cx) -> element`,
  which adds `.key_context(CONTEXT)` and one `.on_action(..)` per command calling the method with a `Cx<Self>`. A
  handwritten `Render` calls `Self::attach(div(), cx)` (documented); views kiln hosts through the phase-2 view
  macros get it automatically, through `kiln::__private::attach::<V>(..)`, which is the identity for a view with no
  `ViewCommands` (autoref dispatch, so no impl is required).
- A wrong method signature reports at the user's method; a `#[command]` method outside such an `impl` is an error
  naming the outer attribute.

New tests go in `kiln/crates/kiln/tests/macro_view_commands.rs`.

## Implementation notes

- `kiln::__private::attach` is a macro, `kiln::__private::attach!(V, element, cx)`, not a fn
  `attach::<V>(..)`: autoref dispatch picks `ViewCommands::attach` only where `V` is a concrete type at
  the call site, and inside a generic fn it never is. The view macros expand at concrete sites, so they
  call the macro. (A `TypeId`-keyed `inventory` registry would have kept the fn form but drops a library
  crate's views silently, against the design's "no silent cross-crate inventory".)
- `ViewCommands::attach` takes `cx: &Context<Self>`, so a `Render` passes its `Context` or a `&mut Cx<Self>`
  (by deref).
- A method's `context = ".."` replaces the impl's for that command's contribution (its keys, menu item and
  palette entry); its handler is still attached only on the view, under the impl's key context.
- A method marked `#[kiln::command]`, or `#[command]` with `kiln::command` imported, outside such an impl
  is the error naming the impl attribute. A bare `#[command]` with nothing imported is rustc's "cannot find
  attribute", which a macro can't reword.
- A view command's `CommandRecord.run` is `kiln::__private::run_in_view`, a no-op: the view's handler runs
  it, and `contributions.rs` (outside this card's scope) keeps `run` required.
