---
task_uid: "01m5ak9ybsjp0xc21p4rw64w2b"
task: "T229"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "extension_grammar_reregisters_when_only_its_wasm_changes: with a registered grammar extension, replacing only its .wasm (manifest unchanged) registers the grammar again (a new Language) and it still highlights, without a restart"
    required: true
    manual: false
  - id: "C2"
    text: "extension_grammar_reregisters_when_only_its_query_changes: changing only the highlights .scm changes the registered language's highlights"
    required: true
    manual: false
  - id: "C3"
    text: "cargo deny check passes and the existing extension_grammars tests still pass"
    required: true
    manual: false
  - id: "C4"
    text: "The two limits in the spec's Known limits section hold: a swap to a module with different highlighting is not proven here (one healthy fixture), and a same-length, mtime-preserving rewrite is not detected; the second is documented in the doc comment of kiln's extension_files fingerprint"
    required: true
    manual: false
---

# kiln re-registers a grammar extension when only its .wasm or .scm changes (T845 review F3) (Mac T1004)

## Problem

T845 registers contributed grammars from `crates/fastedit/src/config.rs`'s `on_extensions`
callback, but kiln calls it only when an extension's manifest or diagnostics change. Editing only
a contributed `.wasm` or highlights/injections `.scm` file leaves the old grammar registered until
restart (T845 review nrbw1j F3).

## Change

Observe changes to the contributed grammar and query files independently of manifest equality and
schedule a re-registration (through `fe_syntax::register_language`, off the main thread, serialized
as T845 does). The fix may be in kiln (re-run `on_extensions` when a referenced file's content
changes) or in fastedit's callback; pick the smaller. Out of scope: unloading the old language.

## Lent

Lent from Mac T1004. Depends on Mac T845 (commit 6acda579), which helium trunk lacks: ready only after the Mac lead syncs helium. Scope narrowed to the files the fix needs (kiln extensions.rs reload path plus its test, fastedit config.rs, extension_grammars test).

## Known limits (lead decision, 2026-10-19, after review round 2; these are NOT part of this card)

Review round 2 left two mediums this card cannot fix within its scope; they are follow-up cards:

- **F3, one fixture:** the repo has a single healthy grammar module (`tree-sitter-json.wasm`; the other fixtures are hostile), and building a second one with different highlighting needs the tree-sitter CLI and wasi-sdk. So this card proves that a change to only the `.wasm` re-registers the grammar (a new `Language` is registered) and that it still highlights; it does not prove that a module with DIFFERENT highlighting is served after the swap (follow-up: a differing fixture).
- **F4, the change fingerprint:** kiln's fingerprint (`extension_files`: size and mtime of the listed files) misses a same-length, mtime-preserving rewrite of a listed file. Fixing it needs content hashing off the UI thread (follow-up). Document this in the doc comment of the fingerprint.

Review findings that only ask for those two are out of scope for this card.
