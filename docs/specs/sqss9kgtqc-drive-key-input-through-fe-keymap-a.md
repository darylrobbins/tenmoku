---
task_uid: "01m46tytf8a1axevsqss9kgtqc"
task: "T73"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "In crates/fe-gpui/src/tests.rs only bindings_are_all_tested changes (it now enumerates fe_keymap::DEFAULT_MACOS bindings in context editor); CASES, DEDICATED and every other test in the file are unchanged and pass, including the cmd-k cmd-0 and cmd-k cmd-j chords; the other fe-gpui tests, the screenshots example tests and the crates/fastedit/src/perf.rs tests pass"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test keymap_tests::default_keymap_matches_the_old_table: DEFAULT_MACOS loads with no diagnostics and has the same 47 bindings (keys, command, context, in order) as card 1b’s table, inlined in the test as the expected list; every command validates against the editor or app registry; keymap_report has no Shadowed issue"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test keymap_tests::user_layer_rebinds_and_unbinds: with a user layer binding ctrl-a to editor.move_line_start and unbinding alt-z in context editor, ctrl-a moves the caret to the line start and alt-z no longer toggles soft wrap; set_user_keymap with a new layer takes effect on the next keystroke"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test keymap_tests::chord_timeout_uses_the_simulated_clock: with cmd-k bound alone and as the prefix of cmd-k cmd-0, cmd-k then advance_clock(1 s) dispatches the cmd-k binding; cmd-k then x inserts x and dispatches nothing else; cmd-k cmd-0 before the timeout folds all; no real time is waited"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test keymap_tests::window_context_reaches_the_matcher: invocations dispatched from an editor carry the context [editor] (seen through observe_invocations), and a binding with context workbench does not fire in an editor-only window"
    required: true
    manual: false
  - id: "C6"
    text: "Gate check: rg 'KeyBinding::new|bind_keys' crates matches only test files; crates/fe-gpui/src/default_bindings.rs is deleted; crates/fastedit/src/main.rs creates one ConfigWatcher covering settings.toml and keymap.toml; scripts/check-no-gpui.sh passes; Cargo.lock gains no [[package]] entry"
    required: true
    manual: false
---

# Drive key input through fe-keymap: a default keymap file, a GPUI keystroke adapter and hot reload

# Drive key input through fe-keymap: a default keymap file, a GPUI keystroke adapter and hot reload

Foundation card 4b. Depends on card 4a (`fe-keymap`) and card 1b (`RunCommand`, the
`default_bindings.rs` table and `observe_invocations`). It also depends on card 3 through 4a,
because its `ConfigWatcher` and `main.rs` wiring are extended here.

## Problem

After card 1b, GPUI's own keymap resolves keystrokes from a hard-coded table. Users can't rebind
anything. `fe-keymap`'s matcher, its context precedence and its user layer aren't used by the app.

## Change

### Default keymap data

- New `crates/fe-keymap/keymaps/default-macos.toml` holds every row of
  `crates/fe-gpui/src/default_bindings.rs` as `[[bindings]]` entries: 46 editor bindings in
  context `editor`, and `cmd-q` to `app.quit` with no context. Keep the same keys and order.
- Export it from `crates/fe-keymap/src/lib.rs` as `pub const DEFAULT_MACOS: &str = include_str!(…)`.
- Delete `crates/fe-gpui/src/default_bindings.rs`.

### GPUI adapter: `crates/fe-gpui/src/keymap.rs` (new)

**Installation.** `fe_gpui::init(cx)` stops creating GPUI `KeyBinding`s:
`rg 'KeyBinding::new|bind_keys' crates/` finds nothing outside tests. Instead it installs:

- a GPUI global holding a `fe_keymap::Matcher`, built from `DEFAULT_MACOS` plus an optional user
  layer passed to `init_with_keymap`
- a `cx.intercept_keystrokes` interceptor, or an equivalent capture-phase listener that the
  implementer prefers

**Per keystroke:**

- **Context.** The adapter turns `window.context_stack()` (GPUI `KeyContext`s) into a
  `fe_command::ContextStack`. Each context identifier becomes a frame name or flag, and each
  `key=value` pair carries over.
- **`Dispatch(inv)`.** Dispatch `RunCommand(inv)` to the focused element with
  `window.dispatch_action` and stop propagation. The editor and app handlers from card 1b run
  unchanged, and so does `observe_invocations`.
- **`Pending`.** Stop propagation and start a **1 s** timer through the GPUI executor. When it
  fires, call `flush()` and handle the result as below. A new keystroke cancels the timer.
- **`Unmatched(keys)`.** Let the current keystroke propagate so text input reaches the view, as
  today. Swallowed earlier keystrokes are re-delivered as input with a re-entrancy guard, so the
  matcher doesn't see them twice. A keystroke without a `key_char`, such as `cmd-k`, delivers
  nothing.

`fe_gpui::KEY_CONTEXT` stays `"editor"`, and `EditorView::render` keeps `.key_context(KEY_CONTEXT)`.

**Public API.** `fe_gpui::set_user_keymap(cx, layer)` swaps the keymap on hot reload, with last good
per layer from `KeymapStore`. `fe_gpui::keymap_report(cx)` returns the issue list, with
`report_contexts` set to `[[editor]]`.

### fastedit `main.rs`

- Load the user keymap from `fe_config::paths::config_dir()/keymap.toml` with `KeymapStore`, and
  pass it to `fe_gpui::init_with_keymap`.
- Add `keymap.toml` to the `ConfigWatcher` file list from card 3. One watcher covers both files.
  The foreground task sends each changed path to the right store and applies the result.
- Log keymap diagnostics and report issues, as card 3 does for settings.

### Existing tests

- `crates/fe-gpui/src/tests.rs`: only `bindings_are_all_tested` changes. It enumerates the
  default keymap's bindings in context `editor` from `fe_keymap` instead of
  `cx.key_bindings()`, and keeps its `>= 40` floor and its comparison with `CASES` and
  `DEDICATED`.
- `CASES`, `DEDICATED` and every other test in that file are unchanged and pass, now resolved by
  our matcher. That includes the `cmd-k cmd-0` and `cmd-k cmd-j` chords.
- `crates/fastedit/src/perf.rs` tests and the `screenshots` example pass unchanged. Its
  `dispatch_keystroke(alt-z)` goes through the interceptor.

### Tests (new file)

`crates/fe-gpui/src/keymap_tests.rs`, declared `#[cfg(test)] mod keymap_tests;` from `lib.rs`.
Timer tests use the GPUI test executor's simulated clock (`advance_clock`), never real time.

## Out of scope

- Workbench bindings. T63 adds `workbench.*` rows to `default-macos.toml` when it lands.
- Vim routing.
- A keymap UI and on-screen diagnostics.
- Non-macOS keymaps.

## Decided (lead defaults; the user may revisit)

- GPUI's keymap is no longer used at all. Every key goes through `fe-keymap`, so there is one
  precedence rule and one source of truth.
- The chord timeout is 1 s and isn't configurable yet.
- The default keymap file lives in `fe-keymap` (`keymaps/default-macos.toml`), so later crates
  such as `fe-workbench` add rows to one file rather than calling bind functions.
- Unmatched swallowed chord prefixes are re-delivered as text input. A prefix without a character
  delivers nothing.

## Implementation notes

- **What `init_with_keymap` and `set_user_keymap` take.** Both take the effective keymap
  (`impl Into<Arc<fe_keymap::Keymap>>`): `DEFAULT_MACOS` with the user's layer over it, as
  `KeymapStore` builds it, rather than a bare `KeymapLayer`. `KeymapStore` keeps the last good
  version of each layer, but it exposes only the combined keymap (`keymap()` and
  `KeymapEvent::keymap`), and `fe-keymap/src/load.rs` is outside this card's scope. So the
  store's output is what main.rs hands over: on startup, then on every reload.
  `fe_gpui::init(cx)` uses `fe_gpui::default_keymap()`, the defaults alone. The tests build a
  user layer with `Keymap::from_layers([default, user])`.
- **Command catalog.** `fe_gpui::commands::command_catalog()` checks bindings against the
  editor registry, then against the app registry. main.rs passes it to `KeymapStore::load`, so
  an unknown command is dropped with a warning.
- **`command_tests.rs`.** It's outside the scope list, but its `table_is_the_only_binding_source`
  read the deleted `DEFAULT_BINDINGS` and GPUI's keymap. It is ported, keeping its name. It now
  checks that the default keymap has 47 rows, that `cmd-q` → `app.quit` is the only binding
  outside `editor`, and that GPUI's keymap is empty after `init`.
- **Dispatch is synchronous.** It goes through the focused element's
  `FocusHandle::dispatch_action`, which, like GPUI's keymap did, runs the action inside the key
  event. `Window::dispatch_action` defers to the end of the effect cycle, which broke
  `each_frame_wraps_at_most_one_slice`. That test measures the `alt-z` work inside the same
  update. With no focused element, the adapter falls back to `Window::dispatch_action`.
- **A failed chord.** The swallowed keystrokes are re-delivered as input, through
  `Window::dispatch_keystroke` under the re-entrancy guard. Those without a `key_char` deliver
  nothing. Then the keystroke that ended the chord is matched again, alone, as a fresh one:
  `cmd-k left` moves left, as GPUI's replay did. If nothing matches it either, it propagates to
  the view. `cmd-k x` still types `x` and dispatches nothing else.
- **Keys the adapter ignores.** Standalone modifier keystrokes, and keys `fe-keymap` can't name
  (such as `f21`), skip the matcher and propagate as before. They don't cancel the chord timer.
- **Hot reload.** `set_user_keymap` drops a chord in progress (`Matcher::set_keymap`) without
  re-delivering it. `keymap_report`'s issues are logged at info level at startup and after each
  reload. A user override of a default binding is reported as `Overridden`, which is expected
  and not a warning.
