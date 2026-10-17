---
task_uid: "01m53hnmcskdrqbzan87hh6cs3"
task: "T122"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp test navigation::uri_spelling::definition_in_own_file_with_respelled_uri_decodes_against_the_buffer passes: a definition in the own edited file at app/[slug]/page.rs answered as %5Bslug%5D is Tier::SameFile with a snapshot and a range decoded against the buffer; one in another open edited file under app/(group)/ answered as %28group%29 is decoded against that buffer with its snapshot; with no file on disk the own-file answer is not Unavailable; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-lsp -p fe-nav -p fe-workbench passes with no existing test edited; workspace_edit.rs changes only file_path's visibility; tests/navigation.rs changes only by the mod line; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Match LSP navigation locations to the request's own file and open files by decoded path, so a server that escapes [ ] ( ) in URIs still gets the edited buffer (Mac T727)

## Problem

LSP navigation (definition, references and the rest decoded by `Decoder::decode` in
`crates/fe-lsp/src/navigation.rs`, around lines 643-680) recognises the request's own file with
`own == uri` and an open document with `document.uri == *uri`. `url::Url` equality compares the
serialized string, so the same file spelled with different percent-escapes isn't recognised.

fe-lsp builds its URIs with `Url::from_file_path` (`manager.rs`), whose path-segment set leaves
`[ ] ( ) @ ! $ & ' * + , ; =` unescaped. Servers built on vscode-uri, and typescript-go's
`FileNameToDocumentURI` (internal/ls/lsconv/converters.go), escape them: fe-lsp sends
`file:///proj/app/[slug]/page.tsx` and gets back `file:///proj/app/%5Bslug%5D/page.tsx`. Next.js route
folders such as `app/[slug]/` and `app/(group)/` make this common. Then:

- a location in the request's own file falls through to the disk path: decoded against the file on disk,
  not the edited buffer (wrong bytes when unsaved), `Tier::Project` instead of `Tier::SameFile`, no
  snapshot, so own-file references no longer sort first; with no file on disk it is dropped as
  "couldn't be read" and the request can answer `NavError::Unavailable`;
- a location in another open, edited file is likewise read from disk.

`workspace_edit.rs` `decode_file` (around line 256) already matches open documents by URI or by decoded
path (`file_path(&document.uri).is_ok_and(|path| path == file.path)`); navigation doesn't. Verified by
reading at 32c938f9.

## Change

- `crates/fe-lsp/src/workspace_edit.rs`: make `file_path` `pub(crate)` (visibility only).
- `crates/fe-lsp/src/navigation.rs` `Decoder::decode`: treat a location as the own file when `own == uri`
  or both decode to the same path (`file_path(own)` and `file_path(uri)` both `Ok` and equal), and find an
  open document the same way (`document.uri == *uri` or same decoded path). Compute the decoded path once.
  Everything else in `decode` (rel paths, the disk read cache, dropped reasons) stays.
- New test module `crates/fe-lsp/src/tests/navigation/uri_spelling.rs`, declared with `mod uri_spelling;`
  beside `mod encoding;` in `crates/fe-lsp/src/tests/navigation.rs` (the only change there). Use the
  existing fake setup (it stands in for rust-analyzer, so use a `.rs` file). Test
  `definition_in_own_file_with_respelled_uri_decodes_against_the_buffer`: open an edited buffer at
  `<root>/app/[slug]/page.rs` whose text differs from the file on disk; the fake answers
  `textDocument/definition` with a range in that file spelled `%5Bslug%5D`; the answer is one item,
  `Tier::SameFile`, with a snapshot, and its range is decoded against the buffer's text. Second case in
  the same test: the same with another open edited file `<root>/app/(group)/other.rs` spelled `%28group%29`
  decodes against that buffer with its snapshot; and with no file on disk at all the own-file location
  still answers (not `Unavailable`). It fails at the base.

Diagnostics' `published` URI match in `server.rs` is out of scope.

## Implementation notes

- `Decoder::decode` decodes the location's path once and matches the own file and open documents by URI or by decoded path.
- An open document other than the request's own is decoded against the text the server was last sent, as before.
- The own-file match also covers a request whose own URI is spelled differently from the answer's.
