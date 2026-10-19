---
task_uid: "01m55v1bm05ykr384x9hwzvgns"
task: "T178"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::diagnostics::unopened_files::unopened_files_diagnostics_are_reported_raw passes for UTF-8 and UTF-16: a publication for a never-opened file is one LspEvent::FileDiagnostics with its URI, path, the session's encoding and the items with ranges as sent; decode_file_diagnostics gives from_lsp's bytes past an emoji; an empty publication gives no items; a custom: URI has path None; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::diagnostics::unopened_files::open_documents_are_unchanged passes: a publication for an open document still reports Diagnostics and no FileDiagnostics; tests/diagnostics.rs only gains mod unopened_files;"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: report a server's diagnostics for files it doesn't have open, with their ranges as sent, instead of dropping them (Mac T928)

## Problem

LSP completeness track, item 2 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md` §3:
"Keep pushed diagnostics for unopened files, decoded against disk text as decode_closed does, in Problems").
Servers publish diagnostics for files the client hasn't opened: rust-analyzer's `cargo check` (flycheck)
results cover the whole crate, and gopls and pyright report other packages' errors after an edit. VS Code keeps
every publication per URI and lists them all in Problems. fe-lsp drops any publication whose URI has no
current incarnation in the session (`Actor::published`, `crates/fe-lsp/src/server.rs:1250-1266`, debug log
"with no current incarnation or several"), so Problems shows open buffers only (its module docs' "known
limit", `crates/fe-workbench/src/problems_panel.rs:25-26`).

Positions in such a publication can't be decoded to bytes on the server thread: fe-lsp has no text for the file.
`decode_closed` (`crates/fe-lsp/src/workspace_edit.rs:94`) set the pattern for edits to closed files: hand the
host the raw LSP edits and the encoding, and let it decode them against the file it reads.

This card is the fe-lsp half. Listing them in Problems is the next card.

## Change

1. `crates/fe-lsp/src/diagnostics.rs`:
   - New `#[non_exhaustive] pub struct FileDiagnostic`: `range: lsp_types::Range` as sent, and `severity`,
     `message`, `source`, `code` and `tags` decoded exactly as `Diagnostic`'s are (share the code).
   - New `pub fn decode_file_diagnostics(text: &Text, items: &[FileDiagnostic], encoding: PositionEncoding) ->
     Vec<Diagnostic>`: each range through `from_lsp` both ends, ordered as `decode` orders them, the other
     fields copied.
2. `crates/fe-lsp/src/event.rs`: new variant `LspEvent::FileDiagnostics { server: ServerId, uri: Url, path:
   Option<PathBuf>, encoding: PositionEncoding, items: Vec<FileDiagnostic> }`: the server's diagnostics for a
   file it doesn't have open, replacing those it sent for that URI before (empty clears them). `path` is set
   for `file:` URIs only. Its doc says the host keeps them until the next for that URI, or until the server's
   `Status` becomes `Restarting` or `Stopped` (the session that sent them has ended).
3. `crates/fe-lsp/src/server.rs` `published`: when **no** document of the session matches the URI (no
   incarnation at all, synced or not), report `FileDiagnostics` with the session's encoding, whatever the
   publication's `version`. A URI matching one or several documents is handled exactly as today (an opening
   still in flight, or two buffers of one file, still drop it). Update the diagnostics.rs module docs' list.
4. `crates/fe-lsp/src/lib.rs`: export `FileDiagnostic` and `decode_file_diagnostics`.
5. Tests: new `crates/fe-lsp/src/tests/diagnostics/unopened_files.rs`, declared by `mod unopened_files;` in
   `crates/fe-lsp/src/tests/diagnostics.rs` (its only change), for UTF-8 and UTF-16 sessions:
   - `unopened_files_diagnostics_are_reported_raw`: with `a.rs` open, the fake publishes, unversioned, two
     diagnostics for `/fake/root/b.rs` (never opened), one with severity, source, code and an `Unnecessary`
     tag. The sink gets one `FileDiagnostics` with that URI, `path == Some("/fake/root/b.rs")`, the session's
     encoding and the two items with their ranges as sent; `decode_file_diagnostics` against a text with an
     emoji before the second range gives the byte ranges `from_lsp` gives. An empty publication for `b.rs`
     gives a `FileDiagnostics` with no items. A `custom:` URI gives `path == None`.
   - `open_documents_are_unchanged`: a publication for the open `a.rs` still reports `Diagnostics` and no
     `FileDiagnostics`.

Overlap: T914 (held for sign-off) rewrites the unversioned branch of the same `published`; whichever lands
second keeps both rules (this card's applies only where no document matches). Linux-OK.
