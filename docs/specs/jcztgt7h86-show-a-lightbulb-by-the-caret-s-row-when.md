---
task_uid: "01m55v1f234xkp77jcztgt7h86"
task: "T183"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test lsp_code_actions::tests::lightbulb::shows_where_actions_exist_after_the_caret_rests passes: 250 ms after the caret rests one codeAction with triggerKind 2 is sent and gutter_glyph() is a LightbulbAutofix on its row; moves inside 250 ms send one; only source actions, or none, show nothing; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit tests lsp_code_actions::tests::lightbulb::clicking_the_lightbulb_lists_the_quick_fixes and lsp_code_actions::tests::lightbulb::the_setting_and_other_editors_clear_it pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show a lightbulb by the caret's row when the language server has code actions there, after the caret rests, opening Quick Fix on click, as VS Code does (Mac T939)

## Problem

LSP completeness track, item 4 (user-approved 2026-10-17: "lightbulb"). Once the cards this depends on land, an
editor can show a lightbulb glyph on a row (`EditorView::set_gutter_glyph`), fe-lsp can ask for code actions
with `triggerKind: Automatic` and filter by kind (`code_actions_for`), Quick Fix leaves `source` actions out,
and `editor.lightbulb` (default true) exists. Nothing asks. VS Code, after the caret rests, asks the server for
the actions at the caret (`triggerKind` Automatic, `only` unset, source actions left out) and shows the
lightbulb on the caret's row when any enabled one comes back, blue when one is a preferred quick fix; a click
opens the Quick Fix list there.

## Change

1. New `crates/fe-workbench/src/lsp_lightbulb.rs`, declared from `crates/fe-workbench/src/lsp.rs` as
   `#[path = "lsp_lightbulb.rs"] mod lightbulb;`, run from `Workbench::sync_lsp` (`lsp.rs:1114`), reusing
   T854's caret-rest pattern (`lsp_highlights.rs`: one 250 ms timer restarted by each caret move or edit,
   stale answers dropped by (version, selection)):
   - For the active editor only, with `editor.lightbulb` on and its buffer on a server with a
     `codeActionProvider`: 250 ms after its primary selection or version last changed, ask
     `code_actions_for` at the primary selection with the diagnostics the editor shows overlapping it,
     `automatic: true`, `only: None`.
   - An answer still current shows the glyph on the caret's buffer row when it has at least one enabled
     action that isn't `source`-kind: `LightbulbAutofix` if one is a preferred quick fix, else `Lightbulb`.
     Otherwise no glyph. A move or edit hides the glyph at once, until the next answer.
   - Clicking the glyph runs Quick Fix (`lsp.code_actions`) at that selection.
   - Another editor becoming active, `editor.lightbulb` turning off, or the buffer leaving its server
     clears the glyph and the pending request.
2. `crates/fe-workbench/src/lsp.rs`: the `sync_lsp` call and the setting read.
3. Tests: new `crates/fe-workbench/src/lsp_lightbulb_tests.rs`, declared at the end of
   `crates/fe-workbench/src/lsp_code_actions_tests.rs` as `#[path = "lsp_lightbulb_tests.rs"] mod lightbulb;`,
   with that file's `setup` and GPUI's clock advanced as T854's tests do:
   - `shows_where_actions_exist_after_the_caret_rests`: 250 ms after the caret rests on the diagnostic, one
     `textDocument/codeAction` with `triggerKind` 2 was sent and the editor's `gutter_glyph()` is a
     `LightbulbAutofix` on its row; two moves inside 250 ms sent one request; a reply of only a
     `source.organizeImports` action, or of none, shows no glyph.
   - `clicking_the_lightbulb_lists_the_quick_fixes`: invoking the glyph's click shows the code actions
     picker listing the quick fix.
   - `the_setting_and_other_editors_clear_it`: `editor.lightbulb = false` removes the glyph and sends nothing
     after further moves; activating another split clears the first editor's glyph.

Linux-OK.

## Implementation notes

- `editor.lightbulb` is a `Cell` on the LSP state, set by `reload_settings` (`workbench_root.rs`) and at start (`workbench.rs`), as `code_lens_on` is: the scope (rev 2) now lists both files, as nothing else can see the editor settings.
- The test reaches the glyph's click through a `#[cfg(test)]` accessor, as `EditorView` has no public way to invoke it.
