---
task_uid: "01m5nh3aqg0jhana8w77r5qwwz"
task: "T1120"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Evidence records real rust-analyzer timings on this repo (crates/fastedit/src/config.rs): the post-refresh semantic tokens answer time and whether it exceeds the old 3000 ms hover timeout; if the cause found differs, the evidence says which and the fix and test match it"
    required: true
    manual: false
  - id: "C2"
    text: "A semantic tokens ask that times out is asked again; the fake server answering the first ask after the timeout and the second at once yields the second answer landed, with its Underline and Foreground decorations (new file lsp_semantic_timeout.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench -p fe-lsp passes with no existing test edited; clippy passes"
    required: true
    manual: false
---

# Ask a language server again for semantic tokens that time out, so rust-analyzer's analysed tokens and mutable underline land once it finishes analysing, as VS Code does

## Problem

The user sees no mutable underline (T1016) at all in the app built from main, with rust-analyzer running on this repo (e.g. `let mut servers` in crates/fastedit/src/config.rs ~397). T1016's tests pass with a fake server.

Diagnosis so far (lead's subagent, 2026-10-23), ruled out with evidence: modifier decoding (real rust-analyzer 1.98.1 over stdio sends `mutable`, legend index 17, regardless of client caps; fe-lsp decode at crates/fe-lsp/src/semantic_tokens.rs:355-390 and delta `apply` :338 are right), painting (Layer::Underlines painted; Tint::Muted = theme.foreground), settings (editor.semantic_highlighting defaults true).

Leading hypothesis (not yet proven): rust-analyzer's first full answer is syntactic only (no `mutable`); the analysed answer comes after its `workspace/semanticTokens/refresh` (~5.5 s after open). fastedit gives semantic tokens the `lsp.timeouts.hover` limit (3000 ms, settings.rs:669); a timeout becomes `Ok(None)` (fe-lsp semantic_tokens.rs ~291) and crates/fe-workbench/src/lsp_semantic_tokens.rs ~262 returns on `Ok(None)` without asking again, so the syntactic-only tokens stay until the next edit or refresh. On this large workspace the first analysed answer can plausibly exceed 3 s.

## Do first: confirm

Run real rust-analyzer headless against this repo (stdio, fastedit's client caps, open crates/fastedit/src/config.rs, follow refreshes) and time the post-refresh semantic tokens answer. Record the timings in the candidate's evidence. If the answer does not exceed the timeout and something else drops `mutable` (e.g. a second refresh dropping the in-flight ask in `ask_semantic_tokens_in` without a later ask landing), fix that instead with the matching regression test, and say so in the evidence. If neither explains it, block with what you found rather than guessing.

## Fix (if confirmed)

A semantic tokens ask that times out is asked again (map the timeout to the existing retry path, `attempt < RETRIES`), and semantic tokens get their own timeout setting instead of the hover one if that is the cleaner seam (follow the existing `lsp.timeouts.*` pattern; default generous, e.g. 10 s). The answer that lands replaces the syntactic-only tokens, with its Foreground and Underline decorations.

Test: new file crates/fe-workbench/src/tests/lsp_semantic_timeout.rs, declared at the end of crates/fe-workbench/src/tests/lsp_semantic_tokens.rs like `underline` (`#[path = "lsp_semantic_timeout.rs"] mod timeout;`), reusing lsp_semantic_underline.rs's setup: the fake server answers the first ask after the timeout and the next at once with a `variable`+`mutable` token; the second answer lands with its Underline and Foreground. No existing test edited.

## Implementation notes

Measured with real rust-analyzer 1.98.1 over stdio on this repo, opening crates/fastedit/src/config.rs: the first full answer (syntactic, 399 of 1701 tokens with modifiers) came in 0 ms; after `workspace/semanticTokens/refresh` (at 62.7 s) the analysed answer took 15.08 s (1161 of 1933 tokens with modifiers), well past the 3000 ms hover timeout. A timeout was `Ok(None)` and dropped. It is now `Err(Retry)`, so the existing retry path asks again.
