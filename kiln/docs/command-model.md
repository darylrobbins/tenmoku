# The command model: namespaced commands, context predicates and invocations

This is kiln's standalone copy of the command model `kiln-command`
implements, without the history of the app it was first designed for. The
typed GPUI side (`kiln::commands!`, `dispatch_invocation`) is in
`framework.md` §3.

`kiln-command` is GPUI-free. Its users are wider than any one view: keymaps,
menus, the palette, a modal editing layer, a scripting host and AI agents
need IDs, args, predicates and a registry, but none of an app's models. It
knows nothing about any app; an app, or a component such as a text editor,
registers its commands on it.

## IDs, args and schemas

**IDs.** A `CommandId` has two or more `.`-separated segments, each
`[a-z][a-z0-9_]*`: `editor.move_left`, `app.quit`. The first segment is the
namespace. `Editor.x`, `editor` and `editor..x` are rejected. An ID is a
stable name, so a recording or a keymap from today still resolves after a
rename of the Rust function behind it. It serializes as the plain string.

**Declaring commands.** Headless commands live in `Registry<T>`, where `T` is
a `CommandTarget` marker. It names the type handlers receive (`Target<'a>`,
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
  `context` (a predicate) and `mutates_buffer` (whether it can change the
  document's text). A duplicate or malformed ID is an `Err`.

`commands()` lists every `CommandMeta` in registration order: `id`, `title`,
`description`, `args_schema` (the schemars JSON schema, `None` without args),
`context` and `mutates_buffer`. `CommandMeta` is `Serialize`, so the palette
and an agent's tool list read it the same way.

A command with typed args looks like `editor.move { motion, extend = false,
count = 1 }`. A count can come from a script or an agent as well as from
keys, so a command that repeats bounds it, rejecting an excessive one as
`BadArgs` (and giving the bound as the schema's `maximum`) rather than
doing less than asked and misreporting what ran.

**Representability.** Args are carried as `serde_json::Value` in an
invocation and typed only at dispatch, so the registry, recorders and
transports never need the arg types. They must be representable in both JSON
and TOML, for keymap files:

- They are a table or nothing (a no-arg command accepts `null` or `{}`).
- They never contain a null. `Option` fields use
  `#[serde(skip_serializing_if = "Option::is_none")]`.

The registry enforces both before typing args, in `validate` and `dispatch`
alike: anything else is `BadArgs`, and runs neither the handler nor the
observers. Serde alone would read a positional array (`["x"]`) into a struct,
and pass over a null in a field it ignores or reads as `None`, so a record
that can't be written as TOML would dispatch and be observed.

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
such as `[app, editor]`. Each frame has a name, a set of flags and
`key = value` pairs:
`ContextFrame::new("editor").flag("read_only").set("vim.mode", "normal")`.
Evaluation:

- An identifier is true if any frame has it as its name or a flag.
- `key == value` reads the innermost frame that sets `key`.
- An unset key equals nothing, so `key != value` is true when the key is unset.

`dispatch` evaluates the command's predicate against the invocation's
context. A false predicate gives `DispatchError::NotInContext`.

**Why GPUI's shape.** GPUI's key-context predicates (`gpui::keymap::context`,
Apache-2.0) have the same operators and precedence, so a binding's predicate
reads the same in both. Two differences remain:

- **What this grammar adds.** It allows `.` in identifiers and quoted values;
  GPUI's identifiers are `[A-Za-z0-9_-]`. An element that sets a key for a
  `vim.mode` predicate sets GPUI's `vim_mode`, and
  `kiln_platform::keys::register_context_namespace("vim")` maps it back.
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
  `Format` or `Host(name)`. A document model can map it onto its own record
  of who made each edit.
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
- **Repro tests**: a recorded session dispatched on a fresh model.
- **Audit logs**: who (`actor`) asked for what (`command`, `args`), and how
  (`source`).
- **Remote replication**: the same lines sent to another process.

`Registry::observe(f)` sees every successfully dispatched invocation and none
that failed. It's the seam for all of these. On the GPUI side,
`kiln_platform::commands::observe_invocations` does the same for typed
commands that some element handled.

**Args and targets carry no process-local handles**: no pointers, entity IDs
of a running GPUI app, file descriptors or clipboard handles. Everything an
invocation names must mean the same thing when it's read back in another
process, a later session, or a test. That's why a target is a view ID or a
path the host resolves, and why a paste reads the host's clipboard at
dispatch rather than carrying the text.

## Effects and transactions

`dispatch` returns `Dispatched { effect, transactions: Vec<Tx> }`:

- `effect` is `Nothing`, `Notify` or `Changed`, telling the host whether to
  repaint (`Notify`), or repaint and follow the change (`Changed`, such as
  scrolling to the cursor). A command that changed nothing returns
  `Nothing`.
- `transactions` holds the transactions the command produced, in the
  target's own terms (`T::Tx`), so an agent can keep them to undo, inspect
  or attribute its own changes.

A handler that fails returns a `CommandError`, which comes back as
`DispatchError::Failed` with its message.
