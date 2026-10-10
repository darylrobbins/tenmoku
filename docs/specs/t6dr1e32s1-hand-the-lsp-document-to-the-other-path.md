---
task_uid: "01m4j7v546mdewmpt6dr1e32s1"
task: "T315"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::lsp_aliases_language::changing_the_owners_language_hands_the_document_to_the_other_alias: with two aliases of src/main.rs open, changing the owner to another loaded language sends didClose then didOpen for the other alias to the Rust server by the next frame, and the former owner does not reopen it there"
    required: true
    manual: false
  - id: "C2"
    text: "tests::lsp_aliases_language::owner_switching_to_a_language_without_a_server_leaves_the_alias_served: changing the owner to a language with no configured server leaves the other alias owning the Rust document"
    required: true
    manual: false
  - id: "C3"
    text: "T236's tests in crates/fe-workbench/src/tests/lsp_aliases.rs and the existing tests in crates/fe-workbench/src/tests/lsp.rs pass unchanged"
    required: true
    manual: false
---

# Hand the LSP document to the other path alias when the owning buffer's language changes

# Hand the LSP document to the other alias when the owner's language changes

Follow-up to T236 (review zfegpa, finding F1). T236 gives one buffer the LSP document when path
aliases of a file (e.g. `src/main.rs` and `src/../src/main.rs`) are open as separate buffers, and
hands it to the other alias when the owner closes. A language change on the owning buffer is missed:

- Changing the owner to another loaded language releases ownership but immediately reopens the same
  buffer and reclaims the path, so the waiting alias never acquires it (`handed_over` stays false).
- Changing the owner to a language with no configured server leaves the remaining Rust alias with no
  LSP service at all.

## Change (`crates/fe-workbench/src/lsp.rs`, around line 632)

When a language change releases an owner and another eligible alias (same path, a language with a
server) is shown, that alias takes ownership at the next frame; the former owner does not reacquire
the path in the same step. The former owner, now in the other language, opens its own document under
that language's server only if no alias of the path already owns it there.

## Tests (new file)

`crates/fe-workbench/src/tests/lsp_aliases_language.rs`, declared in `tests/mod.rs`, using
`crate::tests::lsp` helpers as T236's `lsp_aliases.rs` does. Commit the `mod` line with the test
file: the regression check at base needs it.

## Out of scope

Anything outside alias ownership; T236's existing tests stay unchanged.

## Implementation notes

- `Lsp` keeps a `reserved` map (path → alias buffer): when a language change releases an owner and another shown alias has a server for its language, the path is reserved for that alias, so the former owner can't reclaim it on any later frame regardless of editor order.
- Needed `Manager::language_server` in fe-lsp to be `pub` (was `pub(crate)`), outside the listed scope, to ask whether a language has a server.
