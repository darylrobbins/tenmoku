---
task_uid: "01m55v19jrrzj8ksrs6j9xs8h6"
task: "T173"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::router::refresh::refresh_requests_are_reported passes: each of workspace/semanticTokens/refresh, workspace/inlayHint/refresh and workspace/codeLens/refresh is answered null and reported once as LspEvent::Refresh with the matching Refresh kind and the fake's server id, in order; workspace/diagnostic/refresh reports none; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "tests/router.rs's existing assertions are unchanged (only mod refresh; is added); fe-workbench compiles unchanged (its LspEvent match has a catch-all)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: report a server's semanticTokens, inlayHint and codeLens refresh requests as LspEvent::Refresh, instead of answering them and doing nothing (Mac T923)

## Problem

A server asks the client to re-request its semantic tokens, inlay hints or code lenses with
`workspace/semanticTokens/refresh`, `workspace/inlayHint/refresh` and `workspace/codeLens/refresh` (rust-analyzer
sends them once indexing ends and after a `Cargo.toml` change). fe-lsp answers all three `Ok(())` and does
nothing else (`crates/fe-lsp/src/router.rs:162-164`; the replies are pinned in `crates/fe-lsp/src/tests/router.rs:141-143`).
The inlay-hint, semantic-token and code-lens cards of the LSP completeness track (user-approved 2026-10-17,
`~/.local/share/fastedit-lead/lsp-gap-audit.md` §3 items 3, 6, 7) each need to know when it happens. This card
reports it once, for all three, so those cards don't each edit the router and event files.

## Change

1. `crates/fe-lsp/src/event.rs`:
   - New `#[non_exhaustive] pub enum Refresh { SemanticTokens, InlayHints, CodeLens }`.
   - New variant `LspEvent::Refresh { server: ServerId, what: Refresh }`: the server asked for that feature's
     answers to be asked for again, in every document it has open. Its doc says the client capabilities that
     invite these requests are advertised by the feature cards, not here.
2. `crates/fe-lsp/src/router.rs`: the three handlers report the event through the session's sink, as
   `window/showMessage` is reported, then answer `Ok(())` as today. Nothing else changes.
3. `crates/fe-lsp/src/lib.rs`: export `Refresh`.
4. Tests:
   - `crates/fe-lsp/src/tests/router.rs`: no assertion changes (the three replies stay null). Declare
     `mod refresh;` in it.
   - New `crates/fe-lsp/src/tests/router/refresh.rs`, `refresh_requests_are_reported`: the fake sends each of the
     three requests from a thread; each is answered null and the sink gets exactly one
     `LspEvent::Refresh { server, what }` with the matching `what`, for the fake's server id, in the order sent.
     `workspace/diagnostic/refresh` still reports no `Refresh` (it raises the puller as today).

fe-workbench ignores the new event until the feature cards handle it: its `match` on `LspEvent` ends in a
catch-all (`crates/fe-workbench/src/lsp.rs:1598`), so no workbench file changes. Linux-OK.
