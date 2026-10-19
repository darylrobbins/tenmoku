# tree-sitter 0.27.0, patched

This is the `tree-sitter` crate 0.27.0 from crates.io, built from upstream commit
[`6070dbf`](https://github.com/tree-sitter/tree-sitter/commit/6070dbfefd326bd735e5683eb128cc1b57dad0c0)
(its `lib/` directory), with the patch below. `Cargo.toml` at the repository
root uses it in place of the crates.io crate through `[patch.crates-io]`. It's
MIT; see `LICENSE`.

## Why

fastedit loads extensions' grammars at run time as WebAssembly, through this
crate's `wasm` feature (`crates/fe-syntax/src/runtime.rs`). A grammar's code is
someone else's, and 0.27.0 trusts it in two ways the extensions decision
(`docs/decisions/vmtnmkay62-extensions-and-plugins.md` §6) doesn't allow:

- **No execution deadline.** A module initializer, lexer or external scanner
  that never returns hangs the thread running it, and the parse's progress
  callback can't stop it. The crate creates each Wasmtime store inside
  `ts_wasm_store_new` and exposes neither the store nor a deadline, and a
  Wasmtime store's epoch deadline starts at 0, so epoch interruption can't be
  turned on from outside.
- **Aborts on a malformed module.** `wasm_store.c` asserts (`ts_assert`, live
  since `cc` doesn't define `NDEBUG`) that its calls into the module succeed.
  A valid module whose `tree_sitter_<name>` export has another type aborts
  the process, and lexer and scanner function pointers read from the module
  are called with `wasmtime_func_call_unchecked` without checking their types.
- **Trusts what the module passes back.** The host callbacks a module can call
  dereference the running lexer when none is running, and print a debug
  message's string from any address. The token a lexer or scanner says it
  found indexes the grammar's tables unchecked.

`crates/fe-syntax/tests/runtime_grammar.rs` checks these stay fixed
(`a_looping_grammar_is_stopped_at_its_deadline`,
`a_looping_injection_is_stopped_at_its_deadline`,
`a_bad_export_is_an_error_not_an_abort`,
`hostile_grammars_are_errors_not_crashes`), with the fixtures
`tests/fixtures/hostile-grammars.md` describes.

So a grammar's Wasm code can't touch host memory. A malformed grammar's
tables, though, can still crash the editor until **T1005** validates them.
The grammar's static tables are still trusted: the parse table, actions, lex
modes, alias sequences, field and supertype maps, and symbol names. The loader
copies them out of the module's memory after checking only that they lie
inside it, and the parser then indexes them unchecked, so a module with
correct exports but corrupt tables can still make it read out of bounds. The
user accepted this narrowed claim for now, since grammars are user-installed
(decision 2026-10-19, `docs/decisions/vmtnmkay62-extensions-and-plugins.md`
§6). T1005 is the follow-up that adds the validation.

## What changed

Every change is marked `fastedit patch` in a comment.

`src/wasm_store.c`:

- Each new store gets an epoch deadline of one epoch and an epoch-deadline
  callback, `ts_wasm_store__epoch_deadline`. It asks the Rust binding whether
  this thread's code should stop (`ts_wasm_thread_interrupted`). If so, the
  running code stops with an error. Otherwise it runs on to the next epoch.
  On an engine without epoch interruption, none of this has any effect.
- The module initializer (`_initialize`, `__wasm_apply_data_relocs`,
  `__wasm_call_ctors`) and the language function `tree_sitter_<name>` no
  longer assert their calls succeed. An error (a mistyped function, or the
  deadline) is reported as the load's error, and so is an export of these
  names that isn't a function.
- The lexer and scanner functions a language's instance calls are looked up
  with `ts_wasm_store__get_typed_function`, which checks each table entry is
  a function of the type its caller passes. A bad entry, or a missing main
  lexer, fails the load or the instantiation instead of asserting. A call of a
  function the language doesn't have fails the call.
- `ts_wasm_store_reset_heap` puts the stack pointer back, which a stopped call
  leaves lowered. A failed reset sets the store's error instead of asserting,
  and `ts_wasm_store_start` returns false.
- The lexer callbacks trap when no lexer is running. `tree_sitter_debug_message`
  traps unless its string ends inside the module's memory.
- A main or keyword lexer's token must be below the language's `token_count`,
  and an external scanner's below its `external_token_count`. Otherwise the call
  fails and the parse stops. The memory is re-read after the call, in case
  the call grew it.

`src/parser.c`: a failed call into a Wasm main or keyword lexer stops the
parse, as a failed external scanner call already did (`has_scanner_error`).

`binding_rust/wasm_language.rs`: `with_wasm_interrupt(check, f)`, which runs
`f` with Wasm code on this thread asking `check` once an epoch whether to stop,
and `ts_wasm_thread_interrupted`, which `wasm_store.c` calls. The check is the
embedder's: fe-syntax's combines a grammar deadline with the caller's own
budget. The epoch is the embedder's to advance too
(`Engine::increment_epoch`). fe-syntax runs one ticker thread for its engine,
while any check is in place.

Besides the patch, this copy differs from the crates.io package in a few
ways. The packaged files are kept except the cargo metadata files
(`.cargo_vcs_info.json`, `Cargo.toml.orig`, `Cargo.lock`), and this README is
added. `Cargo.toml` is the package's manifest, unchanged.

## When to drop it

Drop this directory and the `[patch.crates-io]` entry once a tree-sitter
release lets the embedder bound a Wasm grammar's execution, through an epoch
deadline or fuel on stores it creates or a hook to set one, and reports
failed calls into a module as errors instead of asserting. Bump the workspace
dependency to that release, move `crates/fe-syntax/src/runtime.rs` to its API,
and check that `runtime_grammar` still passes. Until then, a tree-sitter bump
means re-applying this patch to the new version's files.
