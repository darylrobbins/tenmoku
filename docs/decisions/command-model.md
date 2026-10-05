# Command model: namespaced commands, context predicates and invocations

Status: **build** (foundation card 1a). Decided 2026-10-05. Source card T68.
Card 1b moves `fe-gpui` and `cmd-q` onto it; the keymap engine (cards 4a and
4b), a future `fe-vim`, scripting and AI agents dispatch through it.

Before this, commands were 41 GPUI unit actions (`crates/fe-gpui/src/lib.rs`),
each mapped onto an `EditorModel` call by its own `on_action` closure in
`view.rs`. Nothing outside GPUI could invoke one, list them, pass one
arguments, say where one applies, or record what was invoked.

## Where it lives

A new GPUI-free crate, `crates/fe-command`, not a module of `fe-editor`:

- Its users are wider than the editor. `fe-keymap`, `fe-workbench`, `fe-vim`, a
  scripting host and agent tooling need IDs, args, predicates and the registry,
  but not the buffer, display map and ropey that `fe-editor` brings.
- Commands that aren't editor commands (`app.quit`, later
  `workbench.close_tab`) register the same way without depending on
  `fe-editor`.

`fe-command` knows nothing about editors. `fe-editor` depends on it and
registers its handlers in `fe_editor::commands::registry()`.
`scripts/check-no-gpui.sh` checks both crates. Its only new dependencies are
`serde`, `serde_json`, `schemars` and `thiserror`, all of which were already
in `Cargo.lock` through gpui. `toml` is a dev-dependency.

## IDs, args and schemas

**IDs.** A `CommandId` has two or more `.`-separated segments, each
`[a-z][a-z0-9_]*`: `editor.move_left`, `app.quit`. The first segment is the
namespace. `Editor.x`, `editor` and `editor..x` are rejected. An ID is a
stable name, so a recording or a keymap from today still resolves after a
rename of the Rust function behind it. It serializes as the plain string.
Editor IDs are `editor.` plus the snake_case of the old GPUI action names.
`editor.delete` is forward delete.

**Declaring commands.** Commands live in `Registry<T>`, where `T` is a
`CommandTarget` marker. It names the type handlers receive (`Target<'a>`,
which may borrow) and their transaction type (`Tx`). Registering a command:

- Without args, it takes one call: `register(id, title, handler)`. The handler
  gets `&mut T::Target` and the invoking `Actor`.
- With typed args, it's `register_with_args::<A>(id, title, handler)`, where
  `A: DeserializeOwned + JsonSchema`. The registry deserializes the
  invocation's `args` into `A` before the handler runs. Args that don't
  deserialize give `DispatchError::BadArgs` with serde's message and run
  nothing. Null args are read as an empty table, so a command whose args all
  have defaults can be invoked without any.
- Either call returns a builder for the optional metadata: `description`,
  `context` (a predicate) and `mutates_buffer`. A duplicate or malformed ID is
  an `Err`.

`commands()` lists every `CommandMeta` in registration order: `id`, `title`,
`description`, `args_schema` (the schemars JSON schema, `None` without args),
`context` and `mutates_buffer`. `CommandMeta` is `Serialize`, so the palette
and an agent's tool list read it the same way.

The editor has two typed commands:

- `editor.insert { text }` inserts as typing does.
- `editor.move { motion, extend = false, count = 1 }`. `motion` is a
  snake_case enum over the non-page movements, and the movement repeats
  `count` times. It stops early once the carets stop moving, and makes at
  most `MAX_MOVE_COUNT` (10,000) moves, because a count can come from a
  script or an agent as well as from keys.

**Representability.** Args are carried as `serde_json::Value` in an
invocation and typed only at dispatch, so the registry, recorders and
transports never need the arg types. They must be representable in both JSON
and TOML, for keymap files:

- They are a table or nothing (a no-arg command accepts `null` or `{}`).
- They never contain a null. `Option` fields use
  `#[serde(skip_serializing_if = "Option::is_none")]`.

## Context predicates

`ContextPredicate::parse` accepts:

- an identifier `[A-Za-z_][A-Za-z0-9_.]*` (`editor`, `read_only`)
- `key == value` and `key != value`, where the value is a bare
  `[A-Za-z0-9_.-]+` word or a `"quoted"` string with `\"` and `\\` escapes
- `!p`, `p && q`, `p || q` and parentheses

`!` binds tightest, then `&&`, then `||`. Both binary operators are
left-associative, so `a || b && c` is `a || (b && c)`. Errors carry the byte
offset where parsing failed: `editor &&` fails at byte 9. `Display` prints a
normalised form (single spaces and only the parentheses needed) that parses
back to an equal predicate. Predicates serialize as that string.

A `ContextStack` is a list of `ContextFrame`s from outermost to innermost,
such as `[workbench, editor]`. Each frame has a name, a set of flags and
`key = value` pairs:
`ContextFrame::new("editor").flag("read_only").set("vim.mode", "normal")`.
Evaluation:

- An identifier is true if any frame has it as its name or a flag.
- `key == value` reads the innermost frame that sets `key`.
- An unset key equals nothing, so `key != value` is true when the key is unset.

`dispatch` evaluates the command's predicate against the invocation's
context. A false predicate gives `DispatchError::NotInContext`. Every editor
command's context is `editor`, and `fe_editor::commands::context()` gives the
one-frame stack a focused editor dispatches in.

**Why GPUI's shape.** GPUI's key-context predicates (`gpui::keymap::context`,
Apache-2.0) have the same operators and precedence, so card 1b can hand a
binding's predicate string to GPUI without a second grammar. Two differences
remain:

- **What this grammar adds.** It allows `.` in identifiers and quoted values;
  GPUI's identifiers are `[A-Za-z0-9_-]`. A predicate passed to GPUI must
  avoid them, or the adapter must map `vim.mode` onto a GPUI key such as
  `vim_mode`.
- **What it leaves out.** It has no `>` (descendant) operator. Its `!` takes
  a comparison operand, which GPUI rejects, so `Display` parenthesises every
  operand of `!` except a bare identifier.

GPUI evaluates against its own dispatch path, matched at each depth. This
crate evaluates against one explicit stack, which the host stamps into the
invocation, so the same check runs headlessly and on replay.

## Invocation records

An `Invocation` is one record of "this was asked for". It's plain serde data,
with snake_case tags and one JSON object per line:

```json
{"command":"editor.move","args":{"motion":"word_left","extend":true,"count":3},"actor":"user","source":"keyboard","target":"focused","context":[{"name":"editor"}]}
```

- `command`: the `CommandId`.
- `args`: `serde_json::Value`, left out when null. It's typed at dispatch.
- `actor`: who asked, one of `User`, `Ai { agent, session }`, `Lsp`,
  `Format` or `Host(name)`. It mirrors `fe_buffer::Origin`.
- `source`: how it was triggered, one of `Keyboard`, `Mouse`, `Menu`,
  `Palette`, `Script`, `Replay` or `Test` (the default).
- `target`: what it applies to. `Focused` (the default), `View(u64)` and
  `Buffer { path }` are names the host resolves.
- `context`: the `ContextStack` it was dispatched in, stamped by the host at
  dispatch so a replay checks predicates against the same frames.

Every field but `command` has a default when deserializing, so
`{"command":"app.quit"}` is a valid line.

The same record serves several purposes:

- **Macros**: a recorded list of invocations, replayed in order.
- **AI tool calls**: a command's ID and `args_schema` are the tool definition,
  and its args are the call.
- **Repro tests**: a recorded session dispatched on a fresh model with a fresh
  `HeadlessHost`. The `recorded_invocations_replay` test does this through
  JSON lines.
- **Audit logs**: who (`actor`) asked for what (`command`, `args`), and how
  (`source`).
- **Remote replication**: the same lines sent to another process.

`Registry::observe(f)` sees every successfully dispatched invocation and none
that failed. It's the seam for all of these.

**Args and targets carry no process-local handles**: no pointers, entity IDs
of a running GPUI app, file descriptors or clipboard handles. Everything an
invocation names must mean the same thing when it's read back in another
process, a later session, or a test. That's why a target is a view ID or a
buffer path the host resolves, and why `paste` reads the host's clipboard at
dispatch rather than carrying the text. A replay therefore needs the earlier
`cut` or `copy` in the same recording, which is what a macro needs too. An
agent that wants to paste specific text invokes `editor.insert`.

## Actors and transactions

`fe_editor::commands::origin(&Actor)` maps an actor onto `fe_buffer::Origin`
field for field. `fe-command` doesn't depend on `fe-buffer`.

- **Stamping.** All model edits go through `apply_with_carets` into
  `Buffer::edit`. While a command runs (`EditorModel::run_as`), that edit is
  stamped with the invoking actor's origin. Direct method calls keep using
  `Origin::User`, as before.
- **Undo per actor.** `editor.undo` and `editor.redo` act on the actor's own
  origin. A `User` actor undoes `User` transactions, which is the old
  behaviour. An `Ai { agent, session }` actor undoes only that session's
  edits, leaving the user's in place, and the other way round.
- **Returned IDs.** `dispatch` returns
  `Dispatched { effect, transactions: Vec<TransactionId> }`. `transactions`
  holds the transactions the command produced: the one an edit made, or the
  one an undo or redo reverted or reapplied. Movements produce none. An agent
  keeps these to undo, inspect or attribute its own changes.

`Effect` is `Nothing`, `Notify` or `Changed`:

- `copy`, and `paste` with an empty clipboard, return `Nothing`.
- `save` returns `Notify`.
- Every other command returns `Changed`, which means repaint and scroll to the
  cursor, as the view's `changed()` did.

Save, undo and redo failures come back as `DispatchError::Failed` with the
message the view used to log.

## Vim composition

`d3w`, `3dd`, `"ayy`, `ciw` and `.` should not each be a command, and should
not each be a keymap binding. The default design:

- **Pending input.** `fe-vim` owns a pending-input state machine. It collects
  a count, a register, an operator, and then a motion or text object.
- **One parameterised command.** When the input completes, `fe-vim` emits a
  single invocation of a command such as
  `editor.operate { operator, target: Motion | TextObject, count, register }`.
  `editor.move` already carries a motion and a count in the same way.
- **Keymap routing.** The keymap sends keys in `vim.mode == normal` contexts
  to `fe-vim`'s input handler, rather than having a binding per sequence.
- **Dot-repeat** replays the last invocation whose command `mutates_buffer`.
- **Macros** (`q` and `@`) are recorded invocation lists, via `observe`.

As invocations (`args` only):

| Keys | Invocation |
|---|---|
| `d3w` | `editor.operate {"operator":"delete","target":{"motion":"word_right"},"count":3}` |
| `3dd` | `editor.operate {"operator":"delete","target":{"motion":"line"},"count":3}` |
| `"ayy` | `editor.operate {"operator":"yank","target":{"motion":"line"},"register":"a"}` |
| `ciw` | `editor.operate {"operator":"change","target":{"text_object":"inner_word"}}` |
| `.` | the last mutating invocation again, such as the `ciw` above with its inserted text recorded as the following `editor.insert` |
| `5j` | `editor.move {"motion":"down","count":5}` |

The context names `vim.mode` and the registers are `fe-vim`'s to define. This
card only makes sure the record, the predicate grammar and the replay seam
can carry them.

## Dry run and proposed changes

This is a design only; nothing here is built. Agents and people should share
one command set. An agent shouldn't need a parallel "propose an edit" API.

- **Dry-run mode.** A mutating command run in dry-run mode returns a proposed
  change instead of applying it. That's M8's `ProposedChange`: edits against a
  snapshot version, which `Buffer::edit_from` can later apply or reject as
  conflicting.
- **How it would run.** Two options:
  - Run the handler against a forked model: a cheap clone of the buffer
    snapshot, its selections and display state. Then diff the fork's edit
    log since the fork point. This works for any handler unchanged.
  - Capture the merged edit list in `apply_with_carets` before it reaches
    `Buffer::edit`, and stop there. This is cheaper, but undo and redo don't
    go through it, so they would need their own capture.
- **What can't dry-run.** Commands with effects outside the buffer: `save`
  writes a file, and the clipboard commands touch host state (`copy`, and the
  clipboard half of `cut` and `paste`). A dry run of these is refused, or only
  the buffer part of `cut` and `paste` is proposed.

What exists now is the `mutates_buffer` flag on `CommandMeta`. It says which
commands a dry run would propose and which can run on a read-only buffer.

## GPUI adapter

Card 1b will do the following:

- Replace the 41 unit structs in `actions!` with a single data-carrying GPUI
  action: `Command { id: CommandId, args: Value }`, deserialized from keymap
  JSON. That action's `on_action` builds an `Invocation`:
  - the source is `Keyboard` or `Menu`
  - the context is the focused view's stack
  - the target is `Focused`
- Dispatch it through `fe_editor::commands::registry()` with an
  `EditorTarget` borrowing the view's model and a GPUI `EditorHost`. The host
  provides:
  - `page_rows` from the last layout
  - the clipboard through `cx`
  - `fold_range_at` and `fold_ranges` from the synced syntax tree
- Map the `Effect` exactly as today: `Changed` sets autoscroll and notifies,
  `Notify` notifies, and `Nothing` does neither. An `Err` is logged, as the
  view does now, and repaints.
- Bind `cmd-q` to `app.quit` on an app-level registry.
- Give the view's key context the frame name `editor`, matching `CONTEXT`.

Mouse input, IME composition and scrolling stay direct model calls.

## Not done here

- Recording UI (start and stop, a macro list): only the `observe` seam.
- The command palette: it will read `commands()`.
- The keymap engine and keymap files (cards 4a and 4b).
- Dry-run execution (designed above), and vim itself.
- Mouse input as commands.
- Any change to `crates/fe-gpui` or `crates/fastedit` (card 1b).
