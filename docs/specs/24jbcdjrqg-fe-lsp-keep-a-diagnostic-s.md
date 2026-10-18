---
task_uid: "01m55sbhx65xshtw24jbcdjrqg"
task: "T167"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::diagnostics::related::related_information_and_code_description_are_kept passes for UTF-8 and UTF-16 sessions, pushed and pulled: a same-document related location has its byte range in the published version, one in another file has range None, its path and row, both keep their messages, and code_description is the sent href"
    required: true
    manual: false
  - id: "C2"
    text: "fe_lsp::Related is exported; Diagnostic::new leaves related empty and code_description None; diagnostics::decode keeps its signature, so crates/fe-lsp/src/code_action_tests.rs is unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited (only the mod line added to tests/diagnostics.rs); cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: keep a diagnostic's relatedInformation and codeDescription link, which it advertises, instead of dropping them when decoding (Mac T915)

## Problem

LSP gap audit (2026-10-17, main c7b6a04a): "Advertised but discarded". Re-verified at main 40608138:

- `client_capabilities()` advertises `publishDiagnostics.relatedInformation: true` and
  `codeDescriptionSupport: true` (`crates/fe-lsp/src/server.rs:2108`, `:2113`).
- `fe_lsp::Diagnostic` (`crates/fe-lsp/src/diagnostics.rs:36-50`) has no field for either, and `decode`
  (`:113-139`) drops `related_information` and `code_description`.

So a server that relies on the capability sends its secondary spans in `relatedInformation`. Those are
rustc's "first borrow here" and "expected because of this", and TypeScript's "'x' is declared here". It
sends a doc link in `codeDescription.href` (rustc's error index, ESLint's rule pages). fe-lsp discards both,
and the user never sees them.

This card carries them through fe-lsp. Showing them is the next card (3b). Together they are a feature:
fe-lsp keeps advertising, and stops discarding.

## Change

1. `crates/fe-lsp/src/diagnostics.rs`:
   - `Diagnostic` gains `pub related: Vec<Related>` and `pub code_description: Option<lsp_types::Url>` (the
     `href`). `Diagnostic::new` leaves both empty. The struct stays `#[non_exhaustive]`.
   - New `#[non_exhaustive] pub struct Related`:
     - `uri: Url`;
     - `path: Option<PathBuf>`, for `file` URIs only;
     - `row: u32`, the location's start line, which needs no encoding;
     - `range: Option<Range<usize>>`, set only when the location is in the published document: decoded
       against the same text and encoding as the diagnostic (an end before its start is empty, as `decode`
       reads ranges);
     - `message: String`.
   - Decoding takes the published document's URI. A location in that document is matched by
     `uri == document uri` or the same decoded file path, as `same_file` does in `server.rs`. Keep
     `decode`'s current signature for `crates/fe-lsp/src/code_action_tests.rs:201`; add the URI-taking
     form beside it.
2. `crates/fe-lsp/src/pull.rs` `DiagnosticState::accept` decodes with the document's `uri` (it has it). If
   `merged` moves items' ranges between versions, it moves `related[].range` the same way.
3. `crates/fe-lsp/src/lib.rs`: export `Related`.
4. Tests:
   - Declare `mod related;` in `crates/fe-lsp/src/tests/diagnostics.rs`; no other change there.
   - New `crates/fe-lsp/src/tests/diagnostics/related.rs`, `related_information_and_code_description_are_kept`,
     run for UTF-8 and UTF-16 sessions with an emoji before the ranges, through push and through pull:
     - a diagnostic with two related locations, one in the same document and one in another file
       (`/fake/root/b.rs`, row 7), and a `codeDescription.href`;
     - the event's item has `related[0].range` equal to the same-document location's bytes in the
       published version; for the other, `range == None`, `path == Some("/fake/root/b.rs")` and
       `row == 7`; both messages as sent; and `code_description == Some(href)`.

The workbench's code-action path builds `fe_lsp::Diagnostic::new` (`crates/fe-workbench/src/lsp_code_actions.rs`),
so it is unaffected. Linux-OK.
