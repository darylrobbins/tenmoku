---
task_uid: "01m5stckrbxmkr2fhadhdnpdtq"
task: "T342"
rev: 5
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-core test chord_prefix_dispatch::prefix_with_own_binding_runs_now: with 'escape' bound in a deeper context and 'escape escape' bound in an outer one, one Escape dispatches the deeper binding at once (no Pending wait); a second Escape within the timeout dispatches the chord; a different second key is handled as a fresh keystroke; a timeout dispatches nothing more"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-core test chord_prefix_dispatch::pure_prefix_unchanged: a prefix with no binding of its own (secondary-k) still returns Pending and resolves or times out as before; existing matcher tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-platform tests keys.rs: an already-dispatched prefix is never replayed or flushed again on a miss or timeout"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench zen_mode_escape_tests::esc_esc_leaves: in zen two Escapes leave zen; a single Escape closes an open find bar at once and does not leave; with no find bar and no selection a single Escape does nothing visible; fe-keymap zen_mode_keys: escape escape resolves in the zen_mode context"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check_kiln_boundary.py, nextest and clippy pass in the root workspace and in kiln/"
    required: true
    manual: false
---

# Leave Zen Mode with Escape Escape without delaying a lone Escape: a chord prefix with its own binding runs at once and keeps listening (Mac T1173)

## Problem
VS Code leaves Zen Mode on `escape escape` (workbench.action.exitZenMode, when inZenMode). T1147 shipped Zen Mode without it (Known limit): kiln's matcher (kiln/crates/kiln-core/src/matcher.rs) turns any key that starts a longer binding into `KeyOutcome::Pending`, so with `escape escape` bound in a zen context a lone Escape in a pane waits CHORD_TIMEOUT (1 s), and if another key comes first the Escape is dropped (Find stays open, a selection is not cancelled). Found in T1147 review t6kq38 F1 (high).

## Change
A chord prefix whose own single-key binding resolves in a deeper context runs that binding at once and keeps listening for the rest of the chord:
- Escape closes Find / cancels a selection immediately; a second Escape within the timeout runs the chord (`workbench.toggle_zen_mode` to leave zen).
- On a miss or a timeout nothing already run is replayed or run again (kiln/crates/kiln-platform/src/keys.rs must not replay or flush an already-dispatched prefix).
- Add the new outcome as an explicit `KeyOutcome` variant, documented in matcher.rs (a reviewer rejected unlisted variants before, T72, so it is listed here on purpose).
- Then bind `escape escape` in the zen_mode context in crates/fe-keymap/keymaps/fastedit.toml.
Bindings that are only a chord prefix (no single-key binding of their own, e.g. secondary-k) behave as today.

## Tests
New kiln/crates/kiln-core/tests/chord_prefix_dispatch.rs; extend kiln/crates/kiln-platform/tests/keys.rs; new crates/fe-workbench/src/zen_mode_escape_tests.rs (add `mod` in lib.rs).

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y60).
