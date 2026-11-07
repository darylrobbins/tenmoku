# Grammar comment tokens and shebangs (T298)

A language an extension registers (`[[contributes.grammars]]`) can now say how
it comments and which `#!` interpreters run it:

```toml
[[contributes.grammars]]
language = "mylang"
wasm = "tree-sitter-mylang.wasm"
extensions = ["my"]
line_comment = "//"            # optional, non-empty
block_comment = ["(*", "*)"]   # optional, exactly two non-empty strings
shebangs = ["mylang"]          # optional interpreter base names
```

1. **Keys carried by `fe_syntax::GrammarSpec`.** `line_comment`,
   `block_comment` and `shebangs` sit beside the extensions and file names.
   `fastedit`'s `ExtensionGrammar` keeps `deny_unknown_fields`, and its JSON
   schema documents the shapes: a line token of at least one character, a
   block pair of exactly two non-empty strings, and interpreter names with no
   `/` or whitespace. kiln's manifest check doesn't descend into an array of
   tables such as `[[contributes.grammars]]`, so `extension_grammars` is what
   enforces them: serde refuses a wrong type (and with it the extension's
   grammars, as before), and a value of the right type but the wrong shape
   refuses that grammar alone. Both are logged with the extension's ID, as a
   grammar naming a file outside its extension is.
2. **Tokens live in the registry entry, not the `Language`.** Registering a
   grammar again with unchanged files returns the language already loaded, so
   tokens that change on their own must still be taken: they go on the
   registry entry, which is replaced each time. `CommentTokens` holds
   `&'static str`, so the strings are interned in a process-wide set: a grammar
   registered again with the same tokens leaks nothing more.
3. **`comment_tokens` falls back to the registry.** Built-in names win; a name
   no built-in has gets its registered entry's tokens. A grammar that gives no
   tokens (or only empty ones) gets `None`, as every registered language did
   before, so toggle-comment stays a no-op for it.
4. **Shebangs: built-ins first, then registered interpreters in registration
   order.** A registered name matches as the built-in table does: the base name,
   through `env` and `env -S`, with a version suffix (`elixir1.17`, `foo3`).
5. **An interpreter a built-in claims is refused**, through the same
   `GrammarError::Clash` path an extension or file name takes
   (`Clash::Interpreter`). It's refused when some program name both would
   match, not only when the names are equal: `python3` and `zsh5` clash with
   the built-ins `python` and `zsh` (as would `node2` with `node`), while
   `pythonista` and `sh.` don't. Only built-ins compiled into the build count,
   as for names and extensions: a language registered as `ruby` in a build
   without the `ruby` feature isn't the built-in, so it may claim `ruby`, keeps
   it when registered again, and is picked only by the shebangs it declares.
6. **The eight first-party extensions are left alone.** Their languages are
   built-ins as well, whose tokens and interpreters already apply; the keys are
   for languages that are only ever wasm.
