---
task_uid: "01m49e5q91e04kdz7saqmc36z8"
task: "T135"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T117

Findings of the review of T117 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/menu.rs:74: Successful GPUI parsing does not guarantee a faithful native key equivalent. gpui-pre 0.3.7's macOS menu builder omits the function modifier, so rebinding Save to cmd-fn-s and unbinding cmd-s installs a native cmd-s equivalent anyway. AppKit can then dispatch Save when fe-keymap rejects cmd-s, undoing the user's unbinding. Its native key conversion also lacks mappings for enter and tab, although both parse successfully. The headless binding tests cannot detect these native conversions.
  fix: Check native representability before installing display bindings. Omit and debug-log shortcuts whose modifiers or keys cannot be represented faithfully, including fn, enter and tab unless their native conversion is corrected. Add regression coverage for these cases.
  instances: crates/fe-gpui/src/menu.rs:74

- F2 medium crates/fe-gpui/src/menu.rs:152: When the captured window closes before deferred dispatch, the fallback reruns the invocation through the app registry. Window commands then resolve cx.active_window() again, potentially affecting another window: a pending Close Window for A can close B after A disappears, and Minimize, Zoom or Full Screen can similarly target B. This converts cancellation of a stale target into an operation on a different target.
  fix: Do not replay stale window-targeted invocations through dispatch_app when that can select another window. Drop those invocations or preserve their original window target, while retaining windowless dispatch for app-wide commands. Add a regression where A closes and B becomes active before the deferred callback.
  instances: crates/fe-gpui/src/menu.rs:152


Scope notes (triage, main 1e99299): both still true. menu.rs:74 installs any `gpui::Keystroke::parse`-able shortcut, and gpui-pre-macos 0.3.7's menu builder (platform.rs) builds the modifier mask from cmd/ctrl/alt/shift only and `key_to_native` has no enter or tab mapping. menu.rs:152 still replays through `commands::dispatch_app` when `handle.update` fails. Fix both in `crates/fe-gpui/src/menu.rs`. Put the regressions in a new `crates/fe-gpui/src/menu_native_tests.rs`, registered from menu.rs with `#[cfg(test)] #[path = "menu_native_tests.rs"] mod native_tests;` so lib.rs isn't touched. Leave the keymap, menu table and existing menu_tests.rs alone. This card adds no default key bindings.

## Implementation notes

- F1: `menu::native` mirrors gpui-pre-macos 0.3.7's menu builder: a shortcut gets a display binding only without `fn` and with a single-character key or one of the names its `key_to_native` converts (`NATIVE_KEYS`). Others, such as `cmd-fn-s`, `cmd-enter` and `ctrl-tab`, are debug-logged and the item shows no shortcut. Recheck `NATIVE_KEYS` when gpui-pre is bumped.
- F2: when the captured window closed before the deferred dispatch, only commands in the `app` namespace replay through `dispatch_app`; anything else (the `window.*` commands, and editor/workbench ones the app registry wouldn't know) is dropped with a debug log.
- Not fixed, outside this card: while the captured window is still open, `window.*` commands reach `commands::on_active_window`, which resolves `cx.active_window()` again, so a window activated between choosing and the deferred run would still be the target. Fixing it means giving app commands the window they ran in (commands.rs).
