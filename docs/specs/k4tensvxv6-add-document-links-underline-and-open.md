---
task_uid: "01m5qycgs7shnckak4tensvxv6"
task: "T330"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "LspManager::document_links and resolve_document_link decode links to bytes, Ok(None) without the provider, resolve only when the server offers it and the link has no target"
    required: true
    manual: false
  - id: "C2"
    text: "With editor.links on (default), the whole link under the pointer is underlined with the modifier held, and cmd/ctrl-click opens it: file: targets in an editor tab at the #L12,5 fragment, http, https and mailto through the system opener, other schemes refused with a toast; a click off a link still goes to definition. Limit (Known limits, review round 3): the #line,col fragment's column is kept when the target opens in an editor tab, but for viewer targets it first reveals column zero and the later landing can be lost [F5]; the single workbench-wide column-landing subscription is cleared by another link opening, so overlapping opens in different panes may lose their landings [F6]; a link to a target reserved by an ongoing Save As loses its column [F7]; an oversized fragment column is truncated rather than clamped [F8]. Follow-up T338 fixes these. Known limits (review F9, F10, carried by T339): a target-less link clicked before the post-edit refresh may resolve against the old version and toast that it has no target; a link open superseded by a pane switch or newer navigation is not yet dropped."
    required: true
    manual: false
  - id: "C3"
    text: "Command editor.open_link opens the link at the caret and is in the command catalog"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in document_link_tests.rs, link_ranges_tests.rs, lsp_links_tests.rs and links_setting.rs pass headless"
    required: true
    manual: false
---

# Add document links: underline and open the language server's links on cmd-click with editor.links and Open Link, as VS Code does (Mac T1112)

## Problem

fastedit has no `textDocument/documentLink` (`rg documentLink crates` is empty). In VS Code, a language server's links (URLs and file paths in comments and strings, `import` paths in some servers, package.json dependencies) are underlined while ctrl/cmd is held over them and open on ctrl/cmd-click (`editor.links`, default on; command `editor.action.openLink`). fastedit's secondary-click already runs `nav.goto_definition` (fastedit.toml:494-497), which does nothing on a URL.

## Change

1. fe-lsp: new `crates/fe-lsp/src/document_link.rs` (+ lib.rs mod and `pub use DocumentLink`): `LspManager::document_links(buffer, version) -> Future<Result<Option<Vec<DocumentLink>>, LspError>>` with `DocumentLink { range: Range<usize>, target: Option<String>, tooltip: Option<String>, data: Option<serde_json::Value> }` (bytes of the request version, sorted, empty ranges dropped); `LspManager::resolve_document_link(buffer, version, link) -> Future<Result<DocumentLink, LspError>>` (`documentLink/resolve`, only when the provider says `resolveProvider` and the link has no target). `Ok(None)`/nothing sent without `documentLinkProvider`. Client capability `document_link: { dynamic_registration, tooltip_support: true }` in server.rs; both methods in the hover timeout group in requests.rs.
2. fe-gpui: new `crates/fe-gpui/src/link_ranges.rs`: `EditorView::set_link_ranges(Vec<Range<usize>>)` (follow edits until the next set). In `EditorView::hover_at` (view.rs :1103) the hover range for a bound modifier click is the link range under the pointer when there is one, else the word as now, so it underlines (pointing hand) the whole link. `EditorView::link_at(offset) -> Option<Range<usize>>`.
3. Setting `editor.links` (bool, default true): fe-config settings.rs, settings-template.toml, settings.schema.json, propagated like `code_lens_on` (workbench.rs, workbench_root.rs).
4. fe-workbench: new `lsp_links.rs` (mod in lsp.rs; `sync_links`): per open editor, ask on open and 500 ms after an edit settles, `set_link_ranges`, keep the links by editor. Command `editor.open_link` ("Open Link", `#[command]` in workbench_commands.rs, no default key as in VS Code) opens the link at the caret. `nav.goto_definition` (what secondary-click runs after placing the caret) first checks for a link at the caret and opens it instead of navigating; resolving a target-less link first. Opening: `file:` URIs open in an editor tab (fragment `#L12`, `#L12,5` or `#12,5` reveals that 1-based line/column; a missing file toasts "Can't open <name>"); `http`, `https` and `mailto` go to the system opener (`App::open_url`); any other scheme is refused with a toast "Can't open link: unsupported scheme". No extra confirmation prompt.
5. `crates/fe-workbench/src/command_catalog_tests.rs` gains `editor.open_link` in its list of commands (the catalog test names every command).

## Tests

- `crates/fe-lsp/src/document_link_tests.rs` (#[path] mod in document_link.rs): `links_decode_to_bytes`, `no_provider_sends_nothing`, `resolve_fills_the_target`, `resolve_without_provider_is_unavailable`, `empty_ranges_are_dropped`.
- `crates/fe-gpui/src/link_ranges_tests.rs` (#[path] mod in link_ranges.rs): `hover_underlines_the_whole_link_with_the_modifier`, `without_the_modifier_nothing_is_underlined`, `links_follow_edits`.
- `crates/fe-workbench/src/lsp_links_tests.rs` (#[path] mod in lsp_links.rs, fake server): `cmd_click_on_a_file_link_opens_it_at_the_fragment`, `cmd_click_on_a_url_opens_it_through_the_platform` (assert the test platform's recorded opened URL), `unsupported_scheme_toasts`, `a_target_less_link_resolves_first`, `cmd_click_off_a_link_still_goes_to_definition`, `editor_links_off_asks_nothing`, `open_link_command_opens_the_link_at_the_caret`.
Linux-OK (headless). Overlaps: view.rs (T198, T1030, T1031, T1036 hold it; keep the hover edit small), workbench_commands.rs (T937, T1063, T1079, T1080), settings.rs (T1031).

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y58), not truncated.

## Implementation notes

- `DocumentLink` also has `scheme()` and `file_target()` (path and fragment of a `file:` target), since fe-workbench has no `url` dependency. `resolve_document_link` sends the link back with its range re-encoded against the admitted version, so only while the server has the version the link was found at; a link with a target is answered as it is, a server without `resolveProvider` is `Unavailable("documentLink/resolve")`, and a timeout leaves the link as it was.
- fe-gpui anchors the link ranges at the version they're set at (start after, end before, so text typed at an edge stays outside) and drops them past the buffer's oldest mappable version.
- The workbench keeps each buffer's links anchored, as `Slot::links`, and hands them to every editor of the buffer. A bound click notes the link under the pointer before placing the caret (`EditorView::take_clicked_link`), since the caret may land at either edge of the glyph clicked and the hover is cleared before the command runs; `nav.goto_definition` from `Source::Mouse` opens that link, and a click off a link opens none. F12 opens the link holding the caret's char; `editor.open_link` also takes one ending at the caret, as VS Code's Open Link does.
- The pane opens a missing path as a new file, so a `file:` link checks `is_file` off the main thread first and toasts "Can't open <name>" otherwise. A fragment's column counts UTF-16 units, as VS Code's do, of the row as the target's editor decoded it: the pane reveals the row, and once `Pane::revealed` reports that reveal for the open (and the caret hasn't moved since), the caret moves along the row with `Text::point_utf16_to_offset`. So unsaved edits, a dropped BOM, a detected encoding and CRLF rows all count as the editor has them. A large file's viewer gets the row only.
- Setting or clearing an editor's link ranges notifies it, and the next frame finds the hover again even when there was none (the pointer over a space inside a link), so a hover under a still pointer finds or loses the link at once. A `ContentModified`/`ServerCancelled` reply (`LspError::Retry`) is asked again after 500 ms, up to 8 times, while the request is still current.
- A target-less link whose resolve fails or gives no target says "This link has no target" in an info toast.

## Known limits

From review round 3 (review `2ntq4g`); the lead disputed these, and follow-up T338 fixes them. A file link's `#line,col` fragment opens the target at its line, and the column lands correctly in an editor tab with its text loaded, but:

- **F5:** a target that opens in the large-file viewer reveals column zero first; the column correction is made only for editors, so the viewer's column is lost.
- **F6:** column landing goes through a single workbench-wide subscription (`Links::landing`). Opening another column-bearing link clears it, so when opens overlap in different panes, the first can lose its column.
- **F7:** a link to a target held by an ongoing Save As loses its column: `Pane::open_once_saved` resumes through `open_tab`, which bumps `open_serial`, and the landing takes that as a superseding open.
- **F8:** a fragment column past `u32::MAX` is truncated (`as u32`) rather than clamped to the line's end.

Doing the conversion inside `Pane::apply` (pane.rs), against the loaded editor's text or the viewer's row, fixes all four. T338 has that file in scope.

From review `1y1h22` (F9, F10), disputed by the lead and carried by follow-up T339:

- **F9:** a link without a target resolves against the version its links were found at. After an edit, before the refresh 500 ms later, the server refuses that resolve as "buffer changed", so a cmd-click or Open Link says "This link has no target" (`Workbench::open_link_at` in `lsp_links.rs`). The fix is to ask for the current links and match the clicked one before resolving.
- **F10:** link opens that finish later (after a resolve, or after the file-exists check in `open_link_file`) don't carry the pane they started in or a navigation token. A pane switch or a newer open meanwhile doesn't cancel them: the link opens in whichever pane is active then.
