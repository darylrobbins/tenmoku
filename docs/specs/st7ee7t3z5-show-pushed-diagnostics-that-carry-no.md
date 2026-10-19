---
task_uid: "01m57zd6rbgbx78gst7ee7t3z5"
task: "T195"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::diagnostics::unversioned::unversioned_pushes_land_on_the_text_last_sent passes: on a server with no diagnosticProvider, an unversioned push before any edit is reported at the opened version and one after two synced edits at the buffer's current version, each range decoded on that version's text; an older versioned push after it is still dropped"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit tests tests::diagnostics::unversioned::a_pulling_server_still_drops_unversioned_pushes (a server advertising diagnosticProvider) and tests::diagnostics::unversioned::a_drop_spec_drops_unversioned_pushes (UnversionedDiagnostics::Drop: before an edit, after one, and across a reopen) pass: no Diagnostics event for any of them"
    required: true
    manual: false
  - id: "C3"
    text: "UnversionedDiagnostics has Drop, Latest and the default LatestUnlessPulled, each documented; docs/decisions/lsp-client.md §4.1 item 4 and its test list, and the diagnostics.rs module docs, state the new rule and the accepted reopen trade-off"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with tests::diagnostics::dropped_publications edited only to remove its three unversioned steps (no other existing test edited, except tests::diagnostics::unopened_files::open_documents_are_unchanged, edited only to drop its unversioned-push-to-an-open-document step, which the new default changes by design) and crates/fe-lsp/src/registry.rs gains exactly one line, unversioned_diagnostics: Drop on the typescript-language-server spec, keeping tsserver_diagnostics::builtin_specs passing unedited; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope (unopened_files.rs, registry.rs, that one line, included in scope) except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show pushed diagnostics that carry no version against the text last sent, as VS Code does, from servers whose diagnostics aren't pulled, instead of always dropping them (Mac T914)

## Problem

LSP gap audit (2026-10-17, main c7b6a04a): "Unversioned diagnostics are always dropped". Re-verified at
main 40608138:

- `Actor::published` (`crates/fe-lsp/src/server.rs:1250-1282`): a `publishDiagnostics` with no `version`
  goes to `match self.spec.unversioned_diagnostics`, whose only arm is `Drop` (`:1268-1277`), with a debug
  log.
- `UnversionedDiagnostics` (`crates/fe-lsp/src/launcher.rs:80-85`) has only `Drop`, and no `[lsp]` key
  sets it.

So a server that pushes without versions and isn't pulled shows no diagnostics at all. That covers any
user-added server that does so (`[lsp.servers.*]`). Among the built-ins, only typescript-language-server
pushes unversioned, and its diagnostics are pulled through tsserver instead
(docs/decisions/42t4r87mwk-pull-diagnostics.md). VS Code shows unversioned pushes against the document as
it is.

This is by design today: docs/decisions/lsp-client.md §4.1 item 4 drops unversioned publications "whatever
the incarnation has sent so far", allowing an exception only for "a named, verified server-specific
guarantee". Its reason: after a close and reopen, a publication computed on the old text can arrive after
the new `didOpen`. This card changes that decision for servers whose diagnostics aren't pulled. It is a
feature. The user approved the §4.1 change on 2026-10-18 ("do as VS Code"); the decision doc is updated in this card.

## Change

1. `crates/fe-lsp/src/launcher.rs` `UnversionedDiagnostics`:
   - Add `Latest`: an unversioned publication is decoded against the newest version in the incarnation's
     ring (the text the server was last sent).
   - Add `LatestUnlessPulled`, the new `#[default]`: `Latest` while the session doesn't pull diagnostics
     (`Puller::pulls()` is false), `Drop` while it does. A pulling server's unversioned pushes stay
     dropped (typescript-language-server's, beside its tsserver pulls).
   - Keep `Drop`. Each variant's doc says what it does.
2. `crates/fe-lsp/src/server.rs` `published`, and `crates/fe-lsp/src/document.rs` `Document::publish`:
   - For an accepted unversioned publication, take the ring's newest `lsp_version`.
   - Decode, version-check against the newest decoded (equal is accepted) and report it exactly as a
     versioned publication of that version. Nothing else about versioned publications changes.
3. `crates/fe-lsp/src/diagnostics.rs` module docs: the unversioned bullet describes the new rule.
   `docs/decisions/lsp-client.md`: rewrite §4.1 item 4 and its test list (~line 1253) to match. Record
   the accepted trade-off: across a reopen, a delayed unversioned publication from the old text can show
   on the new text until the server's next publication replaces it, as in VS Code.
4. Tests:
   - `crates/fe-lsp/src/tests/diagnostics.rs` `dropped_publications`: remove its three unversioned steps
     (before an edit, after one, across a reopen). They move to the new file under a `Drop` spec. This is
     the only existing test edited. Declare `mod unversioned;` in the same file.
   - New `crates/fe-lsp/src/tests/diagnostics/unversioned.rs`:
     - `unversioned_pushes_land_on_the_text_last_sent`: a fake with no `diagnosticProvider`. An
       unversioned push before any edit is reported at the opened version, its range decoded on that
       text. After two synced edits, one is reported at the buffer's current version, decoded on the
       edited text. An older versioned push after it is still dropped as out of order.
     - `a_pulling_server_still_drops_unversioned_pushes`: a fake advertising `diagnosticProvider`. An
       unversioned push produces no `Diagnostics` event.
     - `a_drop_spec_drops_unversioned_pushes`: `ServerSpec.unversioned_diagnostics = Drop`. The three steps
       removed from `dropped_publications` (before an edit, after one, across a reopen) each produce no
       event.

Not in this card: an `[lsp.servers.<name>] unversioned_diagnostics` setting. `crates/fe-lsp/src/registry.rs`
maps settings to specs, and T844 owns that file. File it after T844 if a server needs the override.
