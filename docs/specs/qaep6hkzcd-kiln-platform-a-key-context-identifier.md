---
task_uid: "01m5q1jaepe3rdbdqaep6hkzcd"
task: "T315"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-platform exports a key context identifier (HOLDS_ALL_KEYS, \"holds_all_keys\") documented beside HOLDS_KEYS; while the focused element's own context carries it, on_keystroke consults no binding for any keystroke, including ones with the platform modifier, abandons a pending chord, and lets the keystroke propagate to the element's key listener"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-platform tests/keys.rs: cmd-w bound to a command is delivered to a holds_all_keys element's key listener and the command doesn't run; the same keystroke runs the command without the identifier; HOLDS_KEYS behaviour is unchanged (cmd keys still matched)"
    required: true
    manual: false
---

# kiln-platform: a key context identifier that holds every keystroke, platform modifier included, for the Keyboard Shortcuts recorder (T300)

## Problem
T300 (Keyboard Shortcuts recorder, Mac T1101) needs a focused view to receive every keystroke raw, including ones with the platform modifier such as cmd-w and cmd-q on macOS, without the keymap running them. kiln-platform has no seam for that outside an editor:

- `HOLDS_KEYS` (`kiln/crates/kiln-platform/src/keys.rs`, `holds_keys`) skips the matcher only for keystrokes *without* the platform modifier: `!event.keystroke.modifiers.platform && …`. The terminal's vi mode relies on that so cmd bindings stay the app's. On macOS cmd-w still closes the tab while a recorder holds keys.
- `KeyOwner::live_stack` can hold every keystroke, but there is one owner, fe-gpui's `VimKeys`, and it holds only for a focused editor with a vim command pending.
- GPUI runs kiln's interceptor (registered first) before any other interceptor or key listener, so a view can't get ahead of it.

## Change
Add a second key-context identifier beside `HOLDS_KEYS`, e.g. `pub const HOLDS_ALL_KEYS: &str = "holds_all_keys";`: while the focused element's own (innermost) context carries it, `on_keystroke` abandons any pending chord and returns without consulting the matcher for every keystroke, platform modifier included, so the keystroke propagates to the element's key listener. Document it in the module doc next to `HOLDS_KEYS`. Standalone modifier presses behave as with `HOLDS_KEYS`.

No app code changes; T300 then sets the identifier on its recorder's context.

## Tests
kiln-platform `tests/keys.rs`: with a binding for `cmd-w` (and a `cmd-k cmd-s` chord), a focused element whose context carries `holds_all_keys` gets both keystrokes in its key listener and nothing is dispatched; without it the binding runs; a chord pending when focus moves into such an element is abandoned. Linux-OK (the test platform takes `cmd-` keystrokes on any host).
