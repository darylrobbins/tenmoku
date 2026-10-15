---
task_uid: "01m50ewgvzzz62dh48vmc61z41"
task: "T89"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln tests/model_persist.rs: changes to a persisted model reach STATE/FILE.json after the debounce, a burst of updates coalescing into one write made off the main thread"
    required: true
    manual: false
  - id: "C2"
    text: "model_persist.rs: at quit the latest value is written before the app exits"
    required: true
    manual: false
  - id: "C3"
    text: "model_persist.rs: a later start loads the value; a missing file starts from Default; an unparsable file is renamed to FILE.json.bad with a warning and the model starts from Default"
    required: true
    manual: false
---

# Persist a kiln model to the state directory: loaded at start, written debounced off the main thread and at quit (Mac T679)

Filed on argon from Mac T679 (card ~/coordination/argon-cards/T679.json rev 1; note zh). Mac task uid 01m4vbj8spnjcjy61vaa3qbha7. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T88 (Mac T678). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T679)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2; design record
`docs/decisions/kiln-app-layer.md`). `#[kiln::model(persist)]` is to give a model durable storage by
convention, as egui's defaulted `save` with auto-save and SwiftUI's `@AppStorage` do. Today the host example
hand-encodes its threads into the layout entry as `Value` (`examples/host/threads.rs:91-97`) and saves them
through the window's layout hooks.

## Change

In a new `kiln/crates/kiln/src/model/persist.rs`, declared from `model.rs`:

- `kiln::model::persist::<M: Serialize + DeserializeOwned + Default>(cx, file)` registers `M` as a persisted
  model stored at `<state dir>/<file>.json` (the `kiln::app` state dir; `file` defaults, in the macro, to the
  type's snake-case name).
- Loading: at registration, synchronously (it's small and needed for the first frame). A missing file starts
  from `Default`; a file that doesn't parse is renamed to `<file>.json.bad`, logged with `log::warn!`, and `M`
  starts from `Default`.
- Saving: after each `update_model` of `M`, debounced (500 ms) and written off the main thread through
  kiln-shell's `persist::Writer` (atomic, as layouts are), a burst of updates coalescing into one write; and at
  quit through `on_app_quit`, which waits for the write.

New tests go in `kiln/crates/kiln/tests/model_persist.rs`.
