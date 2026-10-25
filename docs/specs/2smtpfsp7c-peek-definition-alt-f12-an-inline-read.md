---
task_uid: "01m5qnyv9ybmaq802smtpfsp7c"
task: "T1149"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "peek_tests::peek_opens_below_the_caret_row: with a fake definition result, editor.peek_definition reserves a block of 12 rows directly below the caret's row, pushing later rows down, and shows the target's lines with the definition range highlighted. Known limit (follow-up T1174, review c6w9bc F9/F16): a resize that leaves the reservation row count unchanged does not re-reveal the widget, and refit scrolls to the line cached at open"
    required: true
    manual: false
  - id: "C2"
    text: "peek_tests::esc_closes_and_restores_focus: Esc closes the peek, removes the reserved rows, leaves the caret where it was and focuses the editor; Esc pressed before the first definition answer arrives cancels that request, so nothing opens when it lands (review F23)"
    required: true
    manual: false
  - id: "C3"
    text: "peek_tests::enter_opens_target: Enter in the excerpt (and double-click) opens the target file at the definition and closes the peek; editing above the peek or moving the caret off its row closes it. Known limit (follow-up T1174, F12/F13/F14/F15): CRLF, BOM and non-UTF-8 targets, target buffers edited while the peek is open, and multi-line definition ranges (edited or not) may highlight or open at a shifted or truncated range (the range mapper collapses columns and keeps only the first row)"
    required: true
    manual: false
  - id: "C4"
    text: "peek_tests::several_definitions_list: two definitions show a list of two rows that selects what the excerpt shows; a row selected before its target has loaded shows that target once it loads, never the previous excerpt (review F8); a single one shows no list; an empty or failed answer opens nothing and closes a peek that is already open (review F22)"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap test peek_keys: alt-f12 resolves to editor.peek_definition in the editor context; catalog tests updated; the block reservation does not change existing host_blocks tests; nextest and clippy pass"
    required: true
    manual: false
---

# Peek Definition (alt-F12): an inline read-only widget under the caret's line showing the target, Esc closes, as VS Code's editor.action.peekDefinition

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Peek Definition (editor.action.peekDefinition, alt-F12, also in the right-click Peek menu) opens an inline widget under the caret's line showing the target's source; here go-to-definition always jumps away. Verified: no peek widget exists (the "peek views" were listed out of scope in T107, T112, T114). This card is the widget shell plus Peek Definition; Peek References and the list are the next card.

## Current state
- `nav.goto_definition` (`crates/fe-workbench/src/workbench_commands.rs:633`) runs `nav_command(NavOp::GotoDefinition, ..)`, resolving a target through the LSP or the index and opening it.
- A view can reserve rows between its text rows with `HostBlock { above, rows, kind, id }` (`crates/fe-gpui/src/view/host.rs:41`, painted by `crates/fe-gpui/src/element/host_blocks.rs`), but `HostBlockKind` is only `Filler` or static `Lines`: nothing interactive can sit in the reserved rect. `crates/fe-editor/src/viewer.rs` and fe-fileview give a read-only editor over a file.
- hit-testing trap: see memory gpui-hitbox-occlusion-facts (BlockMouse hides the div's mouse-down/wheel).

## Change
- Add `HostBlockKind::Reserve` (blank rows, no paint) in fe-gpui and have `EditorView` publish the reserved block's pixel bounds each frame so the workbench can draw a child element over them; the block scrolls and wraps with the text.
- New `crates/fe-workbench/src/peek.rs`: a `PeekView` entity placed in the block directly below the caret's row (`above = row + 1`), 12 rows tall by default (clamped to the viewport), with a header (file name and `line:col`), a read-only syntax-highlighted editor showing the target with the definition range highlighted and scrolled to show it with context, and a close button. When the server returns several definitions a left list of them (path and line) selects which one the excerpt shows.
- Command `editor.peek_definition` ("Peek Definition"), key `alt-f12` (VS Code's), context `editor`. Esc closes the peek and returns focus to the editor with the caret unmoved; Enter or double-click in the excerpt opens the target file at that position in the pane; the peek also closes when the document is edited above it, when the caret leaves its row, or when another peek opens. Only one peek per editor.
- No target: show nothing and keep the status message go-to-definition uses today.

## Tests
New `crates/fe-workbench/src/peek_tests.rs` (headless GPUI), `crates/fe-keymap/tests/peek_keys.rs`; catalog tests updated.

## Implementation notes

- fe-gpui: `HostBlockKind::Reserve` paints nothing; each prepaint publishes where the frame laid out its reserve blocks (`EditorView::reserved_blocks`, a `ReservedBlock { id, bounds, clip }` across the editor's width with the editor's frame as clip). `element.rs`, `view.rs` and `lib.rs` (export) carry the few lines that wire it; `host_blocks::layout` skips the kind, so no existing host-block test changed.
- The pane draws the peek (`peek::layer`, one `.children(...)` in `Pane::render` after the body), not the editor: a canvas prepainted after the editor reads the bounds of this same frame, so the peek scrolls with the text without lag, and it isn't nested in the outer editor's dispatch path (whose vim key listener would otherwise see keys the excerpt forwards). Its root occludes the editor under it, and re-emits the pane's activation, whose listener the occlusion hides.
- The peek lives on `Pane::peek` (one per pane, so one per editor). Esc/Enter are workbench commands `peek.close`/`peek.open` (context `peek`); bound at the end of the keymap with `peek && editor` too, so in the excerpt they win over the editor's and vim's own keys at the same depth.
- Go to definition's landing (`nav.rs`) takes an optional peek editor: with one, any items open in the peek; an empty answer or an error keeps today's picker status, opening nothing.
- The excerpt shares an open buffer's text (a split of its model, so the location's range maps onto unsaved text) or reads the file in the background; read-only, no soft wrap, the definition selected and highlighted (a `Highlight` decoration, or the row's band for an empty range) and scrolled to 3 rows above it. Enter and double-click open the file at the excerpt's selection (initially the definition), recording history as go to definition does.
- Carried T858 F2/F3 (host_blocks.rs): a lens's indentation is counted in columns up to the view's right edge rather than collected and shaped, items starting past the right edge aren't shaped, and a title over `MAX_SHAPED_ROW_BYTES` is sliced around the view as text blocks' long lines are; lens hover now honours `PointerOutside` and a window exit redraws. Tests: `view::lens_bounds_tests`.
- Review round 1 (F1–F10): `reserved_blocks` now has every reserve block the block map places, in view or not, so a peek under the bottom row (or a wrapped row running past it) is revealed by scrolling, the caret's row kept in view (blocks are at most the view's rows less one); a peek answer lands only if the caret and the text's version are as asked; the editor showing a peek has `peek_shown` in its key context (`EditorView::set_key_context_flag`), where escape closes it, but for the completion menu's and vim's insert session's; an empty or failed peek answer says so in a toast instead of the picker, leaving focus; a pane closes its peek when its editor leaves, and an editor arriving with a stale peek block loses it (`peek::sync`); the block goes under the first row shown after the caret's, past a fold; in a diff's side alt-F12 goes to the definition; each target is found where it is now as its list row is picked; the excerpt's context rows fit its height; an insertion at the end of the text, under the last row, is above the peek.
- Review round 2: a location on disk (no buffer version) is mapped into a buffer opened on its file since the peek opened, by the row diff from the file as read when it opened (`map_through`); Enter from an excerpt read from disk maps its selection the same way onto the file if it's open by then. A resize refits the block (`fit`) and the excerpt's context and reveals it again; a fold opened or closed under the caret's row moves the block (`Placement::under` on each change); the excerpt scrolls across to a definition far along its row (`select_and_reveal` after placing the rows above it).
- Rev 2 review round 1 (F17–F20; F8, F9, F12–F16 are rev 2's known limits, T1174): Esc and the close button also drop a peek still being asked for (`nav_state.goto.next()`); the peek shows, and an answer opens it, only while the pane shows its editor, not a Markdown preview in its place (`peek::shows`); an excerpt sharing an open buffer opens and names the buffer's path as it is now (`shown_path`); files are read through the pane's loader (`loader::read`, so decoded as it decodes them), one baseline per file, none for a file over `MAX_DIFF_BYTES`, and a file the pane would open in a viewer (`loader::is_large`) isn't read for the excerpt, which says so.
- Rev 2 review round 2: baselines stop at `BASELINE_BUDGET` (32 MiB on disk, all files') and read no further; files past it get none, as an oversized one does (T1174's limits).
- Rev 4 (F8, F21, F22, F23): baselines arriving re-show the selected target if it's a location on disk shown from an editor opened since (`PeekView::baselines_read`); only files a baseline is read for count against the budget, at the bytes kept; an empty or failed peek answer closes the peek open in that editor (`Workbench::close_peek_in`), focusing the editor if the peek had focus; while a peek is asked for, a keystroke interceptor (`Nav::peek_escape`, ahead of any binding, which may stop the key) cancels it on Esc in its editor, whatever else Esc does there: the editor's own, closing the completion menu, ending vim's insert session. It goes once the answer lands, and is inert once another request replaces it.
