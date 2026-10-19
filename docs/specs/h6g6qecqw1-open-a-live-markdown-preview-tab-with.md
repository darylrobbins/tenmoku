---
task_uid: "01m55c2bncqf81vbh6g6qecqw1"
task: "T848"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap test markdown_preview_keys_are_bound_on_each_platform (tests/markdown_preview_bindings.rs): on macOS cmd-shift-v and cmd-k v, and on Linux and Windows ctrl-shift-v and ctrl-k v, resolve to markdown.show_preview and markdown.show_preview_to_side in the editor context with vim off and in vim normal, insert and visual; in workbench \u003e terminal neither resolves to them; and no other default binding uses those keys"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test workbench_commands_tests::markdown_preview_tests::open_preview_opens_a_read_only_preview_tab: cmd-shift-v in README.md opens an active tab titled Preview README.md after it in the same pane, not dirty and with no path; the palette lists Markdown: Open Preview and Markdown: Open Preview to the Side; and in a .rs file nothing opens"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test workbench_commands_tests::markdown_preview_tests::open_preview_to_the_side_splits_right_and_keeps_focus: with one pane, cmd-k v splits right with the preview and focus stays in the source; again, it activates that preview without opening a second; and with a pane already to the right, the preview goes there with no new split"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test workbench_commands_tests::markdown_preview_tests::preview_rerenders_after_edits_settle: after typing a heading, the preview's document is unchanged at 299 ms of quiet and has the heading after 300 ms; a reload to the light theme restyles the preview with the editor theme's background"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test workbench_commands_tests::markdown_preview_tests::preview_links_open_files_urls_and_anchors: docs/guide.md#L3 opens guide.md in the preview's pane with the caret on row 2; https://example.com reaches cx.opened_url(); #usage scrolls the preview to that heading; javascript: does nothing; and no existing test file's tests are edited, except the fe-gpui keymap inventory the two new bindings change: the binding count 272 -\u003e 274 in command_tests::table_is_the_only_binding_source and keymap_tests::default_keymap_matches_the_old_table, a markdown. prefix beside find./lsp./git. in that test's catalog filter, and cmd-shift-v and cmd-k v added to DEDICATED in tests::bindings_are_all_tested"
    required: true
    manual: false
---

# Open a live Markdown preview tab with Markdown: Open Preview (cmd-shift-v) and Open Preview to the Side (cmd-k v), re-rendered 300 ms after edits, whose links open files, the browser or scroll to anchors

## Problem

The user, 2026-10-17: "We also need a markdown preview view", modelled on VS Code's built-in one.

- **Commands:** "Markdown: Open Preview" (cmd-shift-v on macOS, ctrl-shift-v elsewhere) and "Markdown: Open
  Preview to the Side" (cmd-k v / ctrl-k v).
- **The preview:** a read-only preview tab that re-renders live as the source changes, debounced, with a tab
  title like "Preview README.md".
- **Links:** relative links open the file in fastedit, http(s) links open in the browser, and #anchors scroll.

The model (`fe-markdown`) and the view (`fe-markdown-view`'s `MarkdownView`) come from the two cards this
depends on. Nothing in fe-workbench opens one, and no command or key exists.

Keys, checked against `crates/fe-keymap/keymaps/fastedit.toml` at 92c57c57:

- `secondary-shift-v` is unbound in every context. Only `secondary-v` is bound: `editor.paste` and
  `text_input.paste`.
- `secondary-k` is already a chord prefix in `editor` and `workbench` (`secondary-k secondary-0/j/i/f`,
  `secondary-k secondary-\\` and the rest), and `secondary-k v` is free.
- Vim: no `vim.*` binding uses ctrl-k or ctrl-shift-v. fe-vim has no ctrl-k digraphs.
  - `fe_gpui::vim::vim_key` maps an unbound ctrl-shift-v to `<C-v>`, so in a Markdown file on Linux the new
    binding takes ctrl-shift-v from vim normal mode. Plain ctrl-v (`vim.key`, `!macos`) still starts visual
    block. Terminal Vim never sees ctrl-shift-v as a separate key either.
- The terminal pastes on ctrl-shift-v off macOS (`fe-terminal-view/src/input.rs` `clipboard_key`). So the
  bindings go in the `editor` context, never `workbench`, which the terminal is inside.

## Change

New `crates/fe-workbench/src/markdown_preview.rs`, declared from `workbench_commands.rs` with
`#[path = "markdown_preview.rs"] mod markdown_preview;`, as `git_commands.rs` declares its modules. This keeps
`lib.rs` and `workbench.rs` out of scope.

- `pub struct MarkdownPreview`, a `kiln_shell::PaneItem` with `KIND = "fastedit.markdown_preview"`. It wraps
  an `Entity<MarkdownView>` and keeps a `WeakEntity<EditorView>` for its source.
  - The tab title is `Preview <file name>` (`Preview Untitled` with no path), which the window title uses
    too.
  - `is_dirty` is false. `path()` is `None`, so opening the file still finds its editor tab rather than the
    preview. `serialize` is `None`, so previews aren't restored on relaunch. `split` returns a second preview
    of the same source.
- **Live update:** it `cx.observe`s the source editor. When the buffer's `version()` has moved, it (re)starts a
  300 ms timer, VS Code's preview delay. When the timer fires it parses `fe_markdown::parse` on
  `cx.background_spawn` and applies the result with `MarkdownView::set_document` only if no newer version
  started meanwhile.
  - On every notify it rebuilds `MarkdownStyle::new(source.theme(), &kiln_ui::ui_theme(cx))`, so a theme
    reload, light or dark, restyles the preview.
  - If the source closes, the preview keeps its last document. It follows only its own source, like VS Code's
    locked preview; following the active editor is out of scope.
- **Commands:** `impl Workbench` gets `open_markdown_preview(&mut self, to_side: bool, window, cx)`, which acts
  when `Workbench::active_editor` is Markdown: its `syntax()` language is `markdown`, or, while the language
  loads, `fe_syntax::language_name_for_path` of its path is `markdown`. Otherwise it does nothing and logs
  why.
  - Without `to_side`, the preview opens as a new tab after the source's tab in the same pane, and is
    activated and focused.
  - With `to_side`, it goes to `pane_group.neighbour(active, Direction::Right)`, or to a new split right of
    the active pane when there is none (the `Destination::Split` route that `Workbench::move_tab` uses).
    Focus goes back to the source editor afterwards, as VS Code's preserveFocus does.
  - A preview of the same source already in the target pane is activated instead of opening a second.
- **Links:** it subscribes to `MarkdownViewEvent::LinkClicked` and acts on
  `fe_markdown::link_target(dest, doc_path, Workbench::root())`.
  - `File { path, line }` opens through `Pane::open_at` in the preview's pane, with
    `Reveal { row: line, columns: 0..0, within_row: true }` when there is a line, as VS Code's
    `markdown.links.openLocation` default ("currentGroup") does.
  - `Web(url)` calls `cx.open_url(&url)`.
  - `Anchor(slug)` calls `MarkdownView::scroll_to_heading`.
  - `Unsupported` does nothing.
- In `crates/fe-workbench/src/workbench_commands.rs`, add two methods to the one
  `#[kiln::command(context = "workbench")] impl Workbench` block, each run through `self.in_editor(..)` as
  `lsp.rename` is:
  - `/// Markdown: Open Preview` with `#[command(id = "markdown.show_preview", context = "editor")]`;
  - `/// Markdown: Open Preview to the Side` with `#[command(id = "markdown.show_preview_to_side",
    context = "editor")]`.
- In `crates/fe-keymap/keymaps/fastedit.toml`, add `secondary-shift-v` for `markdown.show_preview` and
  `secondary-k v` for `markdown.show_preview_to_side`, both with `context = "editor"`.
- Dependencies: `fe-markdown` and `fe-markdown-view` go in `crates/fe-workbench/Cargo.toml`.

Tests:
- New `crates/fe-workbench/src/markdown_preview_tests.rs`, declared at the end of the existing
  `crates/fe-workbench/src/workbench_commands_tests.rs` with
  `#[cfg(test)] #[path = "markdown_preview_tests.rs"] mod markdown_preview_tests;`. Use `tests::open_root`,
  `simulate_keystrokes` and `executor().advance_clock`.
- New integration test `crates/fe-keymap/tests/markdown_preview_bindings.rs`, as `lsp_bindings.rs` resolves
  keys per `Os`.

## Implementation notes

- **Theme reloads:** in fastedit a reload sets `fe_gpui::ActivePalette` and kiln's `ActiveUiTheme`, but nothing
  recolours editors already open (`EditorView::set_colors` has no production caller). So the preview doesn't take
  its colours from its source: it observes both globals and draws in the source's font and metrics with the
  active palette's colours, which is what an editor opened after the reload shows. Recolouring open editors is
  out of this card's scope.
- **Paths:** the preview follows its source's path (Save As, rename) and `RootChanged`, and passes both to the
  view through `MarkdownView::set_paths`, so links and images resolve against the current file and root.
- **Splits:** splitting a preview whose source has closed copies its last document.
- The two bindings sit after `app.quit` in `fastedit.toml`, past the rows fe-gpui's `OLD_TABLE` compares in order.
