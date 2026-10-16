---
task_uid: "01m50ewh924mk6wvw39yb6wpdv"
task: "T96"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln tests/macro_model.rs: a #[kiln::model] type is available through cx.model from start with no register call"
    required: true
    manual: false
  - id: "C2"
    text: "macro_model.rs: a #[kiln::model(persist)] type is stored at STATE/SNAKE_NAME.json, file = .. overriding the name, and restored on a second start"
    required: true
    manual: false
  - id: "C3"
    text: "compile_fail doctests: an unknown argument, and persist on a type that is not Serialize, fail to compile at the user's tokens"
    required: true
    manual: false
---

# Add #[kiln::model] and #[kiln::model(persist)], registering an app model, persisted to the state directory when asked (Mac T686)

Filed on argon from Mac T686 (card ~/coordination/argon-cards/T686.json rev 1; note zh). Mac task uid 01m4vbjez254bb0c0enax2c2jy. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T89 (Mac T679), T95 (Mac T685). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T686)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). Models (`cx.model`, `cx.update_model`, `cx.watch`) and their persistence
(`kiln::model::persist`) exist from earlier cards, but an app still registers each by hand; the approved design
declares them with `#[kiln::model]`, and `#[kiln::model(persist)]` for one stored at `<state>/<name>.json`.

## Change

In `kiln/crates/kiln-macros` (new `src/model.rs`):

- `#[kiln::model]` on a struct or enum: contributes `kiln::model::register::<M>` (created from `Default` at start).
- `#[kiln::model(persist)]`, with `file = ".."` to override the snake-case type name: contributes
  `kiln::model::persist::<M>(cx, file)`; the type must be `Serialize + DeserializeOwned + Default`, and a missing
  bound reports at the user's type.
- Unknown arguments are errors spanned on the key. Rustdoc shows the expansion and the handwritten form.

New tests go in `kiln/crates/kiln/tests/macro_model.rs`.
