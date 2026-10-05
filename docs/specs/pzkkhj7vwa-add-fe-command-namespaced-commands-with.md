---
task_uid: "01m46tyswvt99vpjpzkkhj7vwa"
task: "T68"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-command test tests::predicate::parses_and_evaluates: editor is true in the stack [workbench, editor]; editor \u0026\u0026 !read_only is false when the editor frame has the read_only flag; vim.mode == normal reads the innermost frame setting vim.mode and is false when unset; a || b \u0026\u0026 c parses as a || (b \u0026\u0026 c); Display output parses back to an equal predicate; \"editor \u0026\u0026\" is an error carrying its byte offset"
    required: true
    manual: false
  - id: "C2"
    text: "fe-command test tests::registry::ids_args_and_metadata: Editor.x, editor and editor..x are rejected as CommandIds; registering an ID twice is an Err; commands() lists metadata in registration order with title, context and mutates_buffer, args_schema None for a no-arg command and an object schema naming the fields for a typed one; validate() reports UnknownCommand, BadArgs and NotInContext without running the handler"
    required: true
    manual: false
  - id: "C3"
    text: "fe-command test tests::registry::invocations_round_trip: an Invocation with actor Ai{agent, session}, target View(7), typed args and a two-frame context round-trips through JSON unchanged; its args round-trip through TOML (toml dev-dependency); a no-arg invocation serializes with no args key; observe() sees each successful dispatch and no failed one"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test tests::commands::every_editor_command_is_registered: fe_editor::commands::registry() lists exactly the 41 editor.\u003csnake_case of each current fe-gpui action\u003e IDs plus editor.insert and editor.move, all with context editor and a non-empty title; mutates_buffer is true exactly for backspace, delete, delete_word_left, newline, tab, cut, paste, undo, redo and insert"
    required: true
    manual: false
  - id: "C5"
    text: "fe-editor test tests::commands::headless_dispatch_matches_model_calls: dispatching through the registry with HeadlessHost on parse() notation gives the same render() as the direct model calls for every move_*/select_* command, page_down (host page_rows), editor.move {motion: word_right, count: 3}, editor.insert, copy/cut/paste through the host clipboard, fold/unfold/fold_all/unfold_all on indented text, toggle_soft_wrap and undo/redo; copy returns Effect::Nothing, save on a temp file Effect::Notify, a move Effect::Changed"
    required: true
    manual: false
  - id: "C6"
    text: "fe-editor test tests::commands::actors_stamp_and_undo_their_own_transactions: editor.insert with actor Ai{agent, session} returns exactly one TransactionId and a move returns none; after a User insert and an Ai insert, editor.undo with the User actor removes only the user text and editor.undo with the same Ai actor removes only the AI text"
    required: true
    manual: false
  - id: "C7"
    text: "fe-editor test tests::commands::recorded_invocations_replay: invocations observed while dispatching insert, move, select, cut, paste and undo on model A, written as JSON lines and read back, dispatched on a fresh model B from the same text with a fresh HeadlessHost, leave B with the same text and selections as A"
    required: true
    manual: false
  - id: "C8"
    text: "docs/decisions/command-model.md has the sections Where it lives, IDs args and schemas, Context predicates, Invocation records, Actors and transactions, Vim composition (showing d3w, 3dd, \"ayy, ciw and . as invocations without a command per combination), Dry run and proposed changes, GPUI adapter and Not done here; it states that args and targets carry no process-local handles"
    required: true
    manual: false
  - id: "C9"
    text: "scripts/check-no-gpui.sh prints ok: fe-command and ok: fe-editor; Cargo.lock gains no [[package]] entry other than fe-command; nothing under crates/fe-gpui or crates/fastedit changes; the existing fe-editor tests pass unchanged"
    required: true
    manual: false
---

# Add fe-command: namespaced commands with typed args, context predicates and headless dispatch

# Add fe-command: namespaced commands with typed args, context predicates and headless dispatch

Foundation card 1a, ahead of M3's remaining work (approved 2026-10-05). The plan's Architecture table
lists a `CommandRegistry` and keymap contexts as extension seams, and `fe-editor` as the headless
model that carries "commands". This card builds that layer, GPUI-free, and registers every existing
editor command on it. Card 1b moves `fe-gpui` and the `cmd-q` binding onto it. The keymap engine
(cards 4a and 4b) and a future `fe-vim`, scripting host and AI agents all dispatch through it.

## Problem

Commands exist only as GPUI unit actions. `crates/fe-gpui/src/lib.rs` declares 41 of them with
`actions!`, and `crates/fe-gpui/src/view.rs` maps each one onto an `EditorModel` method in its own
`on_action` closure. A test, a script or an agent can't invoke "move left" without GPUI. Nothing can
list the commands, and none takes arguments. There's nowhere to say which commands apply in which
context, and no record of what was invoked that could be logged or replayed.

## Where it lives

New crate `crates/fe-command` (GPUI-free), not a module of `fe-editor`:

- `fe-keymap`, `fe-workbench`, a future `fe-vim`, a scripting host and agent tooling need IDs,
  args, predicates and the registry. They don't all need the buffer, display map and ropey that
  `fe-editor` brings.
- Commands that aren't editor commands (`app.quit`, later `workbench.close_tab`) register in the
  same way without depending on `fe-editor`.

`fe-editor` depends on `fe-command` and registers the editor's handlers. `fe-command` knows nothing
about editors.

## Change

### Workspace

- `fe-command = { path = "crates/fe-command" }` goes in `[workspace.dependencies]`.
- Add `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"` and `schemars = "1"`
  there, plus `toml = "1"` for tests. Add each only if it isn't there already.
- All four are already in `Cargo.lock` through gpui (serde 1.0.229, serde_json 1.0.151,
  schemars 1.2.2, toml 1.1.6). `Cargo.lock` must gain no `[[package]]` entry except `fe-command`.
  If a feature you enable pulls in a crate that isn't locked yet, choose another feature set.
- `scripts/check-no-gpui.sh` checks every crate not in `UI_CRATES`, so it must print
  `ok: fe-command` with no change to the script.

### fe-command

Files: `crates/fe-command/Cargo.toml`, `crates/fe-command/src/lib.rs`, `src/id.rs`,
`src/invocation.rs`, `src/predicate.rs` and `src/registry.rs`.

**CommandId (`id.rs`).** A stable, namespaced string ID made of two or more `.`-separated segments,
each `[a-z][a-z0-9_]*`, for example `editor.move_left` or `app.quit`. Parsing validates the form.
Its serde form is the plain string.

**Invocation (`invocation.rs`).** One record of "this was asked for". It's plain serializable data
with no process-local handles, so it can be logged, recorded as a macro, sent as an AI tool call,
saved as a repro test, kept in an audit log or replicated to a remote host:

- `command: CommandId`
- `args: serde_json::Value`: `Null` for commands without args. It's omitted from the serialized
  form when null.
- `actor: Actor`: who asked. It mirrors `fe_buffer::Origin`:
  `User | Ai { agent, session } | Lsp | Format | Host(String)`. `fe-command` doesn't depend on
  `fe-buffer`. `fe-editor` provides the `Actor` to `Origin` conversion.
- `source: Source`: how it was triggered. `Keyboard | Mouse | Menu | Palette | Script | Replay |
  Test`, defaulting to `Test`.
- `target: Target`: what it applies to. `Focused` is the default. `View(u64)` and `Buffer { path }`
  are names the host resolves. They are never pointers or handles.
- `context: ContextStack`: the context it was dispatched in. It's stamped at dispatch, so a replay
  can check predicates the same way.

All of it is serde with snake_case tags. A JSON line looks like this:

```json
{"command":"editor.move","args":{"motion":"word_left","extend":true,"count":3},"actor":"user","source":"keyboard","target":"focused","context":[{"name":"editor"}]}
```

Args must also be TOML-representable. They are a table or nothing, and never contain a null, so
`Option` fields use `skip_serializing_if`.

**Predicates (`predicate.rs`).** `ContextPredicate::parse(&str)` accepts:

- an identifier `[A-Za-z_][A-Za-z0-9_.]*` (`editor`, `read_only`)
- `key == value` and `key != value`, where the key is an identifier (`vim.mode`) and the value is a
  bare `[A-Za-z0-9_.-]+` word or a quoted string
- `!p`, `p && q`, `p || q` and parentheses

Precedence runs from `!` to `&&` to `||`. Parse errors carry the byte offset. `Display` prints a
normalised form that parses back to an equal predicate. The grammar is deliberately a subset of
GPUI's key-context grammar, so card 1b can hand the same string to GPUI unchanged.

`ContextStack` is a list of frames from outermost to innermost. Each `ContextFrame` has a name, a
set of flags and `key = value` pairs, for example `ContextFrame::new("editor").set("mode", "normal")`.
`predicate.eval(&stack)` works like this:

- An identifier is true if any frame has it as its name or as a flag.
- `key == value` reads the innermost frame that sets `key`.
- An unset key is not equal to anything.

`ContextStack` is serde.

**Registry (`registry.rs`).** `Registry<T>` is generic over the target type. Handlers receive
`&mut T` together with the invocation's actor, so the editor can stamp transactions. Two ways to
register:

- Commands without args take one call: an ID, a title and a handler.
- Typed args use `A: DeserializeOwned + JsonSchema`. The registry deserializes `args` into `A`
  before calling the handler. Bad args give `DispatchError::BadArgs` with serde's message and run
  nothing.

Each command's `CommandMeta` holds:

- `id`, `title` and `description`
- `args_schema: Option<serde_json::Value>`, from schemars, and `None` without args
- `context: Option<ContextPredicate>`, the predicate it applies in
- `mutates_buffer: bool`

Behaviour:

- A duplicate ID is an `Err` from `register`.
- `commands()` lists the metadata in registration order.
- `validate(&Invocation)` checks the ID, the args and the context without running anything. The
  keymap and the palette use it.
- `dispatch(&mut T, &Invocation)` checks the predicate against the invocation's context. A false
  predicate gives `DispatchError::NotInContext`. Otherwise it runs the handler.
- An unknown ID gives `DispatchError::UnknownCommand`.
- A handler returns `Result<Dispatched<Tx>, CommandError>`, where
  `Dispatched<Tx> { effect: Effect, transactions: Vec<Tx> }` and the target type picks `Tx`. The
  editor's `Tx` is `fe_buffer::TransactionId`. An associated type or a second generic parameter are
  both fine.
- `Effect` is `Nothing`, `Notify` or `Changed`. `Changed` means the text or the selections moved.
  It lets the GPUI adapter keep today's notify and autoscroll behaviour exactly.
- `observe(f)` registers a callback that receives every successfully dispatched `Invocation`. That
  is the seam for recording, logging and replication. Recording UI is out of scope.

### fe-editor

Files: `crates/fe-editor/Cargo.toml`, `crates/fe-editor/src/lib.rs` (the module declaration and serde
derives) and a new `crates/fe-editor/src/commands.rs`.

**EditorHost** is a trait for what the model can't do alone:

- `page_rows()` for the viewport
- `read_clipboard()` and `write_clipboard(text)`
- `fold_range_at(row)` and `fold_ranges(rows)`, which come from syntax in the GPUI view

`HeadlessHost` is a public, plain implementation for tests, scripts and agents:

- an in-memory clipboard
- 20 page rows
- no syntax, so folding uses the model's indentation fallback, as the view does with no syntax
  today

**The command target** is `EditorTarget<'a> { model: &'a mut EditorModel, host: &'a mut dyn EditorHost }`,
or an equivalent the implementer prefers. `fe_editor::commands::registry()` returns the editor
registry with every existing command. The ID is `editor.` plus the snake_case of the current GPUI
action name, and the handler does what `view.rs` does today:

- `move_left`, `move_right`, `move_up`, `move_down`, `move_word_left`, `move_word_right`,
  `move_line_start`, `move_line_end`, `move_document_start`, `move_document_end`, `page_up` and
  `page_down`
- the 12 matching `select_*` commands, plus `select_all`
- `backspace`, `delete`, `delete_word_left`, `newline` and `tab`
- `fold`, `unfold`, `fold_all`, `unfold_all` and `toggle_soft_wrap`
- `copy`, `cut` and `paste`, which go through the host clipboard
- `undo`, `redo` and `save`

That makes 41. Their context is `editor`. `copy` returns `Nothing`, `save` returns `Notify`, and the
rest return `Changed` unless nothing happened. Two commands take typed args, so scripts and replay
have them:

- `editor.insert { text }` inserts text the way typing does.
- `editor.move { motion, extend = false, count = 1 }` takes `motion`, a serde snake_case enum over
  the non-page `Movement`s. It repeats the movement `count` times.

**Transactions and actor.** All model edits go through one place (`self.buffer.edit(merged, Origin::User)`).

- While a command runs, edits are stamped with the invocation's actor converted to
  `fe_buffer::Origin`.
- `editor.undo` and `editor.redo` act on the actor's own origin. A `User` actor undoes `User`
  transactions, which is today's behaviour.
- Direct method calls keep using `Origin::User`.
- `dispatch` returns the `TransactionId`s the command produced. Movements produce none.

### Decision doc: `docs/decisions/command-model.md`

It records the design and has these sections:

- **Where it lives**: why it's a crate and not part of `fe-editor`.
- **IDs, args and schemas**: naming rules, how no-arg and typed commands are declared, and the
  JSON/TOML representability rules.
- **Context predicates**: the grammar, the evaluation against a stack, and why it is a subset of
  GPUI's.
- **Invocation records**: each field and what it's for: macros, AI tool calls, repro tests, audit
  logs and remote replication. Explain why nothing process-local may appear in args or targets.
- **Actors and transactions**: the mapping onto `fe_buffer::Origin`, undo per actor, and the
  returned transaction IDs.
- **Vim composition**: how `d3w`, `3dd`, `"ayy`, `ciw` and `.` work without a command per
  combination. The default to describe:
  - `fe-vim` owns a pending-input state machine for count, register, operator and motion or text
    object.
  - It emits one invocation of a parameterised command such as
    `editor.operate { operator, target: Motion | TextObject, count, register }`. `editor.move`
    already carries motion and count.
  - The keymap routes keys in `vim.mode == normal` contexts to `fe-vim`'s input handler, instead
    of having a binding per sequence.
  - Dot-repeat replays the last mutating invocation.
  - Macros (`q`/`@`) are recorded invocation lists.
- **Dry run and proposed changes**: design, don't build. A mutating command run in dry-run mode
  returns a proposed change (M8's `ProposedChange`, as edits against a snapshot version) instead of
  applying it, so agents and people share one command set. Describe how it would work, for example
  by running against a forked buffer or capturing the edit list before it's applied, and which
  commands can't dry-run (`save`, the clipboard). This card builds only the `mutates_buffer`
  metadata.
- **GPUI adapter**: what card 1b does, with a single data-carrying GPUI action in place of 41 unit
  structs.
- **Not done here**: recording UI, the palette, the keymap engine, and dry-run execution.

Don't read Zed's GPL crates (`editor`, `vim`, `settings`, `workspace` …) for any of this. gpui's own
`action.rs` and `keymap/` are Apache-2.0 and fine.

### Tests (new files)

- `crates/fe-command/src/tests/mod.rs`, declared `#[cfg(test)] mod tests;` from `lib.rs`, with
  `tests/predicate.rs` and `tests/registry.rs`.
- `crates/fe-editor/src/tests/commands.rs`, declared from `crates/fe-editor/src/tests/mod.rs`. It
  reuses that module's `parse` and `render` notation helpers.

Respect the workspace lints. `unreachable_pub` is warned and the gate runs clippy with
`-D warnings`.

## Out of scope

- Anything in `crates/fe-gpui` or `crates/fastedit` (card 1b).
- Keymap files and the keymap engine (cards 4a and 4b).
- Vim and dry-run execution.
- Recording UI and the palette.
- Mouse input as commands.

## Decided (lead defaults; the user may revisit)

- A new crate, `fe-command`, rather than a module of `fe-editor`.
- IDs are `editor.` plus the snake_case of today's GPUI action names. `editor.delete` is forward
  delete.
- Args are carried as `serde_json::Value` in invocations and typed at dispatch.
- `editor.undo` and `editor.redo` act on the invoking actor's origin.
- An invocation's `target` is a name the host resolves (`Focused`, a view ID or a buffer path),
  never a handle.

## Implementation notes

- `Registry<T>` takes a `CommandTarget` marker: `type Tx` and a borrowing
  `type Target<'a>`. The editor's is `Editor`, with
  `Target<'a> = EditorTarget<'a>` (the model plus `&mut dyn EditorHost`).
  This lets one long-lived registry dispatch to targets borrowed for a single
  call, which card 1b needs to borrow the view's model, `cx` and syntax.
  Handler closures annotate their first parameter
  (`|target: &mut EditorTarget<'_>, actor: &Actor| …`).
- Two deviations from strict GPUI compatibility are documented in the decision
  doc: the spec's grammar allows `.` in identifiers and quoted values, which
  GPUI's doesn't. `Display` parenthesises the operands of `!` so its output
  stays GPUI-parsable apart from those two.
- `editor.move` caps `count` at `MAX_MOVE_COUNT` (10,000) and stops once the
  carets stop moving. That bounds the work an invocation from a script or an
  agent can ask for.
- The `transactions` from `editor.undo` and `editor.redo` hold the transaction
  they reverted or reapplied.
- Save, undo and redo failures are `DispatchError::Failed`, where the view
  used to log them.
