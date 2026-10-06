---
task_uid: "01m48sjfcj3aazrvrekvqpfjz4"
task: "T98"
rev: 4
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-gpui/src/keymap.rs documents, at the focus-out subscription, that a focus move away and back with no draw in between is not seen by a pending chord, why (gpui-pre's focus_generation is pub(crate); focus callbacks fire at draw time), and that keystrokes can't move focus while a chord is pending"
    required: true
    manual: false
  - id: "C2"
    text: "No behaviour change: no code outside comments changes, and existing fe-gpui tests pass"
    required: true
    manual: false
---

# Follow-ups from review of T95

Follow-up from the review of T95, F1 (medium): crates/fe-gpui/src/keymap.rs's focus-out subscription has no focus generation, so a focus move A->B->A with no draw in between is not seen by a pending chord.

Lead decision (respec rev 4): resolve F1 as a documented limit, not a code change. gpui-pre 0.3.7 keeps Window.focus_generation pub(crate), and on_focus_out/on_focus/on_focus_lost fire at draw time by diffing focus paths. While a chord is pending, keystrokes can't move focus (each one ends or continues the chord before it propagates), so A->B->A within one frame needs a mouse, timer or menu focus move and back before the next draw. A fastedit-owned focus wrapper (worker's option 1) would touch pane.rs/main.rs/view.rs, which the M3 chain holds, for a near-unreachable race; revisit if gpui exposes a synchronous focus signal.

Do: add a doc comment on the chord/focus-out handling in keymap.rs stating this limit, why it exists, and the condition under which it could be lifted. No behaviour change.
