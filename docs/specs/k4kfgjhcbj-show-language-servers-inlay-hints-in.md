---
task_uid: "01m55v38amgh0ba9k4kfgjhcbj"
task: "T171"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::lsp_hover::inlay_hints::server_hints_show_and_follow_the_view passes: 250 ms after opening, one textDocument/inlayHint for the viewport range and the editor's inlay_hints() hold the fake's hints; two edits inside 250 ms send one request at the newer version; an older answer is dropped; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test tests::lsp_hover::inlay_hints::refresh_and_the_setting passes: workspace/inlayHint/refresh asks again at once; editor.inlay_hints = false clears the hints and sends nothing after edits; a server without inlayHintProvider gets no request"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show language servers' inlay hints in editors: asked for the rows in view after edits and scrolls, and on refresh, behind editor.inlay_hints (Mac T934)

Filed on argon from Mac T934 (card ~/coordination/argon-cards/zzr/T934.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txsk8ch2qpadtgyqddh0h. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T169 (Mac T932), T170 (Mac T933). Held in the inbox: waits until Mac T923, Mac T922, Mac T854 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T934)".

## Problem

LSP completeness track, item 3b (user-approved 2026-10-17). Once the cards this depends on land:

- fe-gpui shows hints an editor is given (`EditorView::set_inlay_hints(version, hints)`);
- fe-lsp asks a server for the hints in a byte range (`LspManager::inlay_hints`) and advertises
  `inlayHint` with `refreshSupport`;
- fe-lsp reports `workspace/inlayHint/refresh` as `LspEvent::Refresh { what: Refresh::InlayHints, .. }`;
- `editor.inlay_hints` (default true) exists in fe-config.

Nothing connects them, so no editor shows a hint. VS Code asks for the hints of the visible range when an
editor shows, after edits (debounced), when it scrolls to rows it has no hints for, and on a refresh request,
and drops them all when `editor.inlayHints.enabled` is off.

## Change

1. New `crates/fe-workbench/src/lsp_inlay_hints.rs`, declared from `crates/fe-workbench/src/lsp.rs` as
   `#[path = "lsp_inlay_hints.rs"] mod inlay_hints;` (as T854 declares `lsp_highlights.rs`), run from
   `Workbench::sync_lsp` (`lsp.rs:1114`, after every frame):
   - For each open editor whose buffer has a server, with `editor.inlay_hints` on: when its buffer version or
     its viewport rows (`EditorView::viewport_rows`) differ from those last asked for, ask 250 ms after the
     last such change (one timer per editor, restarted by each change, on GPUI's executor) for the hints in
     the byte range of the viewport rows extended by one viewport above and below, at the buffer's current
     version.
   - An answer lands through `set_inlay_hints(version, ..)` only if the editor still exists and its asked
     version and range are still the latest asked; otherwise it is dropped. `Ok(None)` (no provider) clears
     and stops asking for that buffer until its server changes. Hints keep their fe-lsp item in `data`.
   - `LspEvent::Refresh { server, what: Refresh::InlayHints }` asks again, at once, for every editor whose
     buffer that server has.
   - `editor.inlay_hints` turning off clears every editor's hints and cancels pending requests; turning on
     asks again. A buffer leaving its server (closed, the server stopped) clears its hints.
2. `crates/fe-workbench/src/lsp.rs`: the `sync_lsp` call, the `Refresh` arm in the event match, and the
   setting read where the workbench reads `editor.*` settings.
3. Tests: new `crates/fe-workbench/src/tests/lsp_inlay_hints.rs`, declared at the end of
   `crates/fe-workbench/src/tests/lsp_hover.rs` as `#[path = "lsp_inlay_hints.rs"] mod inlay_hints;`, with
   the fake of `tests/lsp.rs` advertising `inlayHintProvider`, advancing GPUI's clock as T854's tests do:
   - `server_hints_show_and_follow_the_view`: 250 ms after opening `src/main.rs`, one
     `textDocument/inlayHint` was sent for the viewport's range, and the editor's `inlay_hints()` holds the
     fake's two hints at their offsets; two edits inside 250 ms send one more request, at the newer version;
     an answer for the older version is dropped.
   - `refresh_and_the_setting`: a `workspace/inlayHint/refresh` from the fake sends a new request at once
     and its answer replaces the hints; setting `editor.inlay_hints = false` clears them and sends nothing
     after further edits; a server without `inlayHintProvider` gets no request.

Linux-OK.
