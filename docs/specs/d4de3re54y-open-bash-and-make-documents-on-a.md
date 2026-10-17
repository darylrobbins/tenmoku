---
task_uid: "01m551h19x00h6r6d4de3re54y"
task: "T150"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-lsp tests::registry::language_ids::shell_and_make_documents_open_with_the_spec_language_ids (src/tests/registry/language_ids.rs, declared by mod language_ids; in src/tests/registry.rs) passes: language_id maps bash (a.sh, .bashrc) to shellscript and make (Makefile, rules.mk) to makefile and passes rust, python, json and the jsx/tsx cases through as before, and on the fake server a bash and a make document opened on user-configured servers arrive in didOpen with shellscript and makefile; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "The languageId sentence in docs/decisions/lsp-client.md §3.2 lists shellscript and makefile; the other fe-lsp tests::registry tests pass as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Open bash and make documents on a language server with the LSP spec's languageIds shellscript and makefile, as VS Code does, instead of bash and make (Mac T782)

## Problem

The LSP `languageId` sent in `didOpen` is fe-syntax's language name passed through, except `jsx` and `tsx`
(crates/fe-lsp/src/document.rs `language_id`). For fe-syntax's `bash` and `make` that sends `"bash"` and
`"make"`, but the LSP specification's table of known language identifiers (and VS Code, which servers are
written against) uses `shellscript` for Shell Script (Bash) and `makefile` for Makefile. A user-configured
server (`[lsp.servers.bash-language-server] command = "bash-language-server", languages = ["bash"]`, which
`registry.rs` accepts) gets a languageId it doesn't know; servers that dispatch on it (efm-langserver,
diagnostic-languageserver, VS Code-derived servers) then do nothing for the file.

At bcb3ea60 the prototype test `shell_and_make_documents_open_with_the_spec_language_ids` (in the
fe-lsp crate's tests::registry module) fails: `language_id("bash", a.sh)` is `"bash"`, expected
`"shellscript"`. Every other compiled-in name (rust, python, c, cpp, csharp, go, java, json, ...) already
matches the spec's identifier.

## Change

In crates/fe-lsp/src/document.rs `language_id`, map `"bash"` to `"shellscript"` and `"make"` to
`"makefile"`; everything else unchanged. Put the test above in a new crates/fe-lsp/src/tests/registry/language_ids.rs, declared by one
`mod language_ids;` line in crates/fe-lsp/src/tests/registry.rs beside its `mod routed_save;` (the test needs
the crate-private `language_id` and the fake server, so it can't live under tests/): the table for bash (`a.sh`, `.bashrc`), make (`Makefile`, `rules.mk`) and a few pass-through names, and through
the manager on the fake, a bash and a make document opened on user-configured servers arrive with
`shellscript` and `makefile`. Update the languageId sentence of docs/decisions/lsp-client.md §3.2 to list
the two.
