---
task_uid: "01m53k1awn665fbc43mp7s250y"
task: "T106"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp test diagnostics::uri_spelling::publications_with_a_respelled_uri_reach_the_open_buffer passes: a version-0 publication for the open synced buffer app/[slug]/page.txt spelled %5Bslug%5D reaches that buffer at its version, decoded against its text; the same for app/(group)/other.txt spelled %28group%29; a publication for app/%5Bother%5D/page.txt is still dropped; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited; tests/diagnostics.rs changes only by the mod line; workspace_edit.rs and navigation.rs are unchanged; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Match pushed LSP diagnostics to an open document by decoded path, so a server that escapes [ ] ( ) in URIs still shows them (Mac T728)

Filed on argon from Mac T728 (card ~/coordination/argon-cards/T728.json rev 1; note zn). Mac task uid 01m53jk7adg79z3n1jpm6avqww. Base: argon trunk aa81cec, with the sync of mac/main e776ee0 gating; the code it touches is the same at e776ee0. Title the host commit "... (Mac T728)".

## Problem

Pushed diagnostics (`textDocument/publishDiagnostics`) are matched to an open document in
`crates/fe-lsp/src/server.rs` `Server::published` (around line 1241) with
`*document.uri() == params.uri`. `url::Url` equality compares the serialized string, so the same file
spelled with different percent-escapes isn't recognised, and the publication is dropped with only a
debug log ("dropped diagnostics for …, with no current incarnation or several").

fe-lsp builds its URIs with `Url::from_file_path` (`manager.rs`), whose path-segment set leaves
`[ ] ( ) @ ! $ & ' * + , ; =` unescaped. Servers built on vscode-uri, and typescript-go's
`FileNameToDocumentURI` (internal/ls/lsconv/converters.go), escape them: fe-lsp opens
`file:///proj/app/[slug]/page.tsx` and the server publishes for `file:///proj/app/%5Bslug%5D/page.tsx`.
Next.js route folders such as `app/[slug]/` and `app/(group)/` make this common, so those files never show
diagnostics. Verified by reading at e776ee0e. The `diagnostics::Inbox` queue in between doesn't key by URI;
pulled diagnostics (`Server::pulled`) go by buffer and are fine.

`workspace_edit.rs` `decode_file` (around line 257) already matches open documents by URI or by decoded
path; Mac T727 does the same for navigation. This card is the diagnostics half.

## Change

- `crates/fe-lsp/src/server.rs` `Server::published`: a document is the publication's when it is synced and
  `*document.uri() == params.uri`, or both URIs have the `file` scheme and `Url::to_file_path` gives the
  same path for both. Decode `params.uri` once, before the filter. Keep the rest as is: still exactly one
  current incarnation or the publication is dropped, the same unversioned handling, the same
  `document.publish` call. Write the check as a small private fn in server.rs; don't use or change
  `workspace_edit.rs` `file_path` (T727 changes its visibility; this card stays independent of it).
- New test module `crates/fe-lsp/src/tests/diagnostics/uri_spelling.rs`, declared with
  `mod uri_spelling;` in `crates/fe-lsp/src/tests/diagnostics.rs` (the only change there), using that
  module's `publish`, `over`, `next` and `marker` helpers through `super::`. Test
  `publications_with_a_respelled_uri_reach_the_open_buffer`: open a synced buffer as
  `app/[slug]/page.txt` (`Started::open_synced`); publish version 0 for the same path spelled
  `%5Bslug%5D`; the next diagnostics are for that buffer, at its version, with the diagnostic decoded
  against its text. Same for a second buffer at `app/(group)/other.txt` published as `%28group%29`. A
  publication for `app/%5Bother%5D/page.txt` (no such open document) is still dropped (a `marker` on the
  first URI comes next). It fails at the base.

No change to manager.rs, document.rs, diagnostics.rs or navigation.rs.
