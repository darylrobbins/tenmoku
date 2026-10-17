---
task_uid: "01m4p2fxyn4k5a2kksatjvfqzp"
task: "T401"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Cargo.toml pins gpui-pre, gpui-pre-platform and gpui-pre-sum-tree to the same exact version, the newest on crates.io, its comment naming that snapshot's Zed revision, and Cargo.lock has no gpui-pre-* crate at another version"
    required: true
    manual: false
  - id: "C2"
    text: "rust-toolchain.toml names the toolchain that Zed revision's rust-toolchain.toml names"
    required: true
    manual: false
  - id: "C3"
    text: "The gate passes (fmt, clippy, nextest, the no-gpui check) and cargo deny check passes"
    required: true
    manual: false
  - id: "C4"
    text: "The finish notes say whether the new snapshot makes a per-focus-handle context stack public (for T198) and list each GPUI API whose meaning changed, with file:line"
    required: true
    manual: false
---

# Bump gpui-pre to its newest snapshot with the matching Rust toolchain (first monthly bump, due 2026-11-02)

## Problem

The plan pins GPUI exactly and says "Bump it monthly" (plan, Research findings: GPUI; risk 1).
`Cargo.toml:42-50` has pinned `gpui-pre`, `gpui-pre-platform` and `gpui-pre-sum-tree` at `=0.3.7`
(zed@1a28cff) since 2026-10-02, and `rust-toolchain.toml:1-2` matches that Zed revision's toolchain
("Bump together with gpui-pre"). No bump has been done. T198 waits on a gpui-pre that exposes
per-focus-handle context stacks. The first monthly bump is due about 2026-11-02.

## Change

Move to the newest `gpui-pre` snapshot on crates.io, every `gpui-pre-*` crate together:

- `Cargo.toml`: the three pins to the new exact version; the comment above names its Zed revision.
- `rust-toolchain.toml`: the toolchain that Zed revision's `rust-toolchain.toml` names, with the
  comment updated.
- Fix what the new snapshot breaks, in fastedit's and kiln's crates, with the smallest change that
  keeps behaviour. Where a GPUI API changed meaning (not only its name or signature), say so in the
  finish notes with the file and line.
- Update the version where it is written down as current: `kiln/README.md:195,226` (the toolchain
  and the pin), `docs/decisions/app-framework.md:13,95`, and `kiln/NOTICE:9` after checking whether
  the new snapshot ships a NOTICE file (if it does, carry what Apache-2.0 §4(d) asks into `NOTICE`
  and `kiln/NOTICE`). Leave attributions of code adapted from 0.3.7 (THIRD_PARTY.md,
  kiln/THIRD_PARTY.md, kiln/CLEAN_ROOM.md) and dated findings in decision files as they are.
- `deny.toml`: only if `cargo deny check` needs a change for the new crates' licences or sources.

In the finish notes, also say whether the new snapshot makes `Window::context_stack_for_focus_handle`
(or an equivalent per-focus-handle context stack) public, for T198.

If crates.io has no snapshot newer than 0.3.7, change nothing and say so in the finish notes.

## Implementation notes

- Newest crates.io snapshot: gpui-pre, gpui-pre-platform, gpui-pre-sum-tree 0.3.9 (zed@93d7456, published
  2026-10-12). Nothing in fastedit or kiln needed a source change; the workspace compiled as is.
- Toolchain: zed@93d7456's rust-toolchain.toml names 1.98.1, the same as before, so only the comment changed.
- NOTICE: gpui-pre 0.3.9 ships no NOTICE file (only LICENSE-APACHE), so `NOTICE` needs nothing and
  `kiln/NOTICE` just names the new version.
- T198: `Window::context_stack_for_focus_handle` is still private (gpui-pre-0.3.9 src/window.rs:6830); it is
  only reachable through `bindings_for_action_in` and `highest_precedence_binding_for_action_in`. No public
  per-focus-handle context stack. The public `Window::context_stack` (window.rs:6730) is for the focused handle only.
- GPUI APIs whose meaning changed (paths inside gpui-pre-0.3.9/src):
  - `Keymap::possible_next_bindings_for_input` (keymap.rs:285), so `Window::possible_bindings_for_input`: leaves out
    bindings that a later `null`/`Unbind` binding disables, and resolves candidates against the typed input. fastedit does not call it.
  - Key dispatch (`Window::input_preference`, window.rs:5990): the choice between character input and key bindings is made
    before keystroke interceptors run, and is reported as `KeystrokeEvent::input_preference` to interceptors and observers.
    It still hinges on `InputHandler::accepts_text_input`, so fe-terminal-view's vi-mode override (input.rs:318) behaves as before.
  - `InteractiveElement::debug_selector` (elements/div.rs:885,898) and so `VisualTestContext::debug_bounds`
    (app/test_context.rs:1020): selectors are recorded only under `test` or `debug_assertions` + `test-support`, no longer
    with `test-support` alone. Release builds with `test-support` return `None`; kiln-shell's window_frame tests run in debug.
  - `Window::bounds_changed` (window.rs:2791) no longer refreshes the window's display id; a separate display-change path does.
- Leak checks: in 0.3.9 `test-support` no longer implies `leak-detection` (gpui-pre build.rs:6-16, Cargo.toml features), so
  every crate whose dev-dependency on gpui asks for `test-support` now also asks for `leak-detection`, keeping the
  entity-handle leak checks the tests had under 0.3.7.
