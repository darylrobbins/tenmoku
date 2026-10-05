---
task_uid: "01m46tyt0am5qjb3bhdh20bnr4"
task: "T69"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-gpui/src/tests.rs is unchanged and all its tests pass (every_binding_does_what_it_should, bindings_are_all_tested, fold, soft wrap, clipboard, undo, save and IME tests), as do the other fe-gpui tests, the screenshots example tests and the crates/fastedit/src/perf.rs tests"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test command_tests::table_is_the_only_binding_source: DEFAULT_BINDINGS has 47 rows (46 editor rows in context editor, cmd-q to app.quit with none); every row names a command registered in the editor or app registry; after init, cx.key_bindings() holds exactly one RunCommand binding per row with the same keystrokes, context and command ID"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test command_tests::keystrokes_dispatch_through_the_registry: with observe_invocations attached, typing x and y then left then cmd-z records editor.insert {text: x}, editor.insert {text: y}, editor.move_left and editor.undo, each with source keyboard and context [editor], and the view shows the same text and caret as before this card"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test command_tests::recorded_keystrokes_replay_headless: invocations recorded from a keystroke session (typing, alt-right, shift-left, cmd-x, cmd-v, enter, cmd-z) serialized to JSON and replayed through fe_editor::commands::registry() on a headless EditorModel with the same starting text give the same text and selections as the view"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test command_tests::app_commands_propagate_from_the_editor: with the editor focused, cmd-q reaches the app registry as app.quit (seen by observe_invocations) after the editor handler propagates it; a RunCommand with an ID neither registry knows is ignored without a panic"
    required: true
    manual: false
  - id: "C6"
    text: "Gate check: rg 'KeyBinding::new' crates matches only crates/fe-gpui/src/commands.rs; rg 'actions!' crates/fe-gpui/src crates/fastedit/src matches nothing; main.rs defines no Quit action; scripts/check-no-gpui.sh passes"
    required: true
    manual: false
---

# Dispatch every editor key binding and cmd-q through the command registry

# Dispatch every editor key binding and cmd-q through the command registry

Foundation card 1b. Depends on card 1a (`fe-command`, and `fe_editor::commands` with its 41
registered editor commands). It moves the GPUI front end onto the registry and changes no
behaviour. Cards 4a and 4b later replace the binding table below with keymap files.

## Problem

`crates/fe-gpui/src/lib.rs` declares 41 GPUI unit actions and binds them with 46 `KeyBinding::new`
calls in `init`. `crates/fe-gpui/src/view.rs` handles each one in its own `on_action` closure.
`crates/fastedit/src/main.rs` declares a separate `Quit` action and binds `cmd-q` itself. Bindings
live in GPUI calls, not data. Nothing passes through the registry, so key presses can't be
recorded or replayed.

## Change

### The GPUI adapter: `crates/fe-gpui/src/commands.rs` (new)

- **One GPUI action** replaces the 41 unit structs:
  `#[derive(Clone, PartialEq, Action)] #[action(namespace = fastedit, no_json)] pub struct RunCommand(pub fe_command::Invocation)`.
  Derive or implement `PartialEq` on `Invocation` in `fe-command` if card 1a didn't, a one-line
  change. Remove the `actions!` block from `lib.rs`.
- **The app registry.** `app.quit` is registered in a `Registry<App>` that `fe_gpui::init` owns,
  with the handler `cx.quit()` and no context. `init` installs a global
  `cx.on_action(|run: &RunCommand, cx| …)` that dispatches app commands. `main.rs` drops its
  `actions!(fastedit, [Quit])`, its `KeyBinding` and its `on_action`. It keeps calling
  `fe_gpui::init(cx)`.
- **The view handler.** `EditorView` replaces its `on_action` closures, and the `movement_actions!`
  macro, with one `on_action(RunCommand)` listener:
  - The listener builds a host for the view and dispatches through `fe_editor::commands::registry()`.
    The registry is built once, in a `static`/`OnceLock` or a GPUI global.
  - The host provides the clipboard through `cx`, `page_rows()`, and the syntax fold candidates
    that `fold` and `fold_all` use today (`synced_syntax`).
  - The listener maps `Effect` onto today's calls: `Changed` calls `self.changed(cx)`, `Notify`
    calls `cx.notify()`, and `Nothing` does neither.
  - `CommandError`s are logged as now ("undo failed: …", "save failed: …").
  - An ID the editor registry doesn't know, such as `app.quit`, is left to propagate
    (`cx.propagate()`), so the app-level handler gets it.
- **Typed text.** Committed text input (`replace_text_in_range` with no range and no marked text)
  is dispatched as `editor.insert { text }` with `source: Keyboard`, so typing is recorded too.
  IME composition (marked text) and explicit-range replacements stay on their current direct path.
- **The invocation context.** Invocations dispatched from the view carry the view's context stack:
  one `editor` frame.
- **Recording seam.** `fe_gpui::commands::observe_invocations(cx, f)` registers a callback that
  receives every invocation the editor and app registries dispatched. This is the hook a macro
  recorder, logger or agent bridge attaches to. No recording UI.

### One binding table: `crates/fe-gpui/src/default_bindings.rs` (new)

- **The table** holds plain data with no GPUI types:
  `pub const DEFAULT_BINDINGS: &[DefaultBinding]`, where
  `DefaultBinding { command: &str, keys: &str, context: Option<&str> }`.
  - It has one row per current binding: all 46 editor rows with context `editor`, plus `cmd-q` for
    `app.quit` with no context.
  - Keystroke strings are exactly today's (`"cmd-k cmd-0"` and so on).
- **`fe_gpui::init`** builds the GPUI `KeyBinding`s from the table in one loop. Each binding's
  action is `RunCommand` with that ID and `source: Keyboard`, and the context string goes to GPUI
  as its predicate.
- **The key context.** `KEY_CONTEXT` becomes `"editor"`. GPUI key context names now equal
  `fe-command` context names. The existing `bindings_are_all_tested` test reads the constant, so it
  needs no change.

### Behaviour stays the same

- `crates/fe-gpui/src/tests.rs` is **not edited** and passes. That covers its key-case table,
  `bindings_are_all_tested`, the fold, soft-wrap, clipboard, undo and save tests, and IME input.
- The other fe-gpui tests, the `screenshots` example's pixel tests and `crates/fastedit/src/perf.rs`'s
  tests pass unchanged too.

### Tests (new file)

`crates/fe-gpui/src/command_tests.rs`, declared `#[cfg(test)] mod command_tests;` from `lib.rs`.

## Out of scope

- Keymap files, user keymaps and chords handled by our own engine (cards 4a and 4b).
- Settings (card 3).
- Mouse input as commands.
- A command palette.
- Menus.

## Decided (lead defaults; the user may revisit)

- One data-carrying GPUI action (`RunCommand`) instead of a generated unit struct per command.
- GPUI key context names equal `fe-command` context names (`editor`). A later workbench uses
  `workbench`.
- `app.quit` lives in an app registry owned by `fe-gpui::init`, because `fastedit` and a future
  host embed the same front end.
- Typed text goes through `editor.insert` so recordings include it. IME composition doesn't yet.

## Implementation notes

- The editor registry is built once per thread in a `thread_local!` (`Registry` holds non-`Sync`
  closures, so a `OnceLock` can't hold it; GPUI runs on one thread). The app registry and the
  invocation observers live in a GPUI global that `init` and `observe_invocations` create.
- `KEY_CONTEXT` is defined as `fe_editor::commands::CONTEXT`, so the two names can't drift.
- A command that fails (`DispatchError::Failed`) logs its `CommandError`, which already reads
  "undo failed: …" or "save failed: …". As before, a failed undo or redo then calls `changed`
  (it may have moved carets) and a failed save only repaints: the listener picks by the
  command's `mutates_buffer`.
- Other dispatch errors are logged; an ID the app registry doesn't know is logged at warn and
  otherwise ignored.
