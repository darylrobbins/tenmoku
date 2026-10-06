---
task_uid: "01m46tzw65v2zwr8nbp3cz5tzj"
task: "T76"
rev: 4
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::quick_open::the_index_is_built_on_first_open: no walk has started after Workbench::new; cmd-p shows the palette with its input focused and starts one walk; after the executor runs, progress is complete; escape then cmd-p again starts no second walk"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::quick_open::typing_filters_and_highlights: after simulate_input of a query, the rows are FinderIndex::query(query, 50) results in order, and each row's highlighted byte ranges cover exactly the chars at that match's indices (including a non-ASCII path)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::quick_open::keys_move_and_confirm: down and up move the selected row, clamped at both ends; enter opens the selected file in a focused tab and closes the palette; for a file already open its tab activates and the tab count is unchanged; escape closes the palette and focus returns to the editor focused before"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::quick_open::queries_stay_bounded_while_typing: typing 6 chars without running the executor in between, then running it, runs at most 2 queries (test counter) and the rows match the final query"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::quick_open::watcher_batches_reach_the_index: after the walk, a file created on disk and passed through ProjectPanel::apply_fs_changes is found by the next query and a deleted one is not; a batch delivered while the walk runs is applied after it finishes"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::quick_open::no_folder_open: with no root, cmd-p shows No folder open and starts no walk"
    required: true
    manual: false
  - id: "C7"
    text: "quick_open.toggle (cmd-p), picker.select_prev/select_next/confirm/dismiss and the text input commands are registered through the fe-command registry (default bindings in crates/fe-keymap/keymaps/default-macos.toml); rg 'KeyBinding::new|actions!' finds no match in crates/fe-ui or in the fe-workbench files this card adds"
    required: true
    manual: false
  - id: "C8"
    text: "fe-ui still depends on no workspace crate; every file copied or adapted from gpui-kit has the source, commit and Apache-2.0 header and a THIRD_PARTY.md row; nothing is taken from Zed GPL crates"
    required: true
    manual: false
  - id: "C9"
    text: "Size: about 1,180 non-test changed lines is accepted by the lead over the 1,000-line cap; TextInput and Picker stay in this card rather than being split out"
    required: true
    manual: false
---

# Add the Cmd-P quick-open palette over the finder index

# Add the Cmd-P quick-open palette over the finder index

Milestone M3 "Browse", third batch. It builds on:

- T62's `fe_project::FinderIndex`: `walk`, `extend`, `finish`, `progress`, `query` returning
  `FinderMatch { path, score, indices }`, `apply`, and `stats`
- T63's `fe-ui` and `fe-workbench` shell: `Workbench`, `ProjectPanel`, `Pane`, `UiTheme`, and the
  headless GPUI test pattern under `crates/fe-workbench/src/tests/`
- the action-model task's command registry, which has stable string command IDs, default bindings,
  context predicates, and a GPUI adapter

The plan's M3 scope includes "Cmd-P", with a finder budget of "first results under 50 ms on 100k files".

## Problem

There's no way to open a file by name. The tree is lazy, so a file deep in a large repo takes many
clicks. T62 indexes paths and matches them, but nothing shows it.

## Change

### fe-ui (stateless primitives, plus one input)

- `crates/fe-ui/src/text_input.rs`: `TextInput`, a single-line input entity with a focus handle.
  - It supports typed text, backspace, alt-backspace (delete a word), cmd-backspace (clear) and cmd-v
    (paste, first line only).
  - It emits `Changed` and `Submitted`. There's no caret movement within the text and no IME
    composition.
  - Its editing keys register as commands in a `TextInput` context through the registry, like
    everything else in this card.
  - It's adapted from gpui-kit's input if that fits. If so, it gets the header, a `THIRD_PARTY.md` row
    and the `NOTICE` mention, as `CLEAN_ROOM.md` requires.
- `crates/fe-ui/src/highlighted_label.rs`: `HighlightedLabel { text, highlights: Vec<Range<usize>> }`.
  It's a `RenderOnce` that draws the highlighted byte ranges in an accent colour via `StyledText`.
  It's also given a helper that turns ascending char indices (T62's `indices`) into byte ranges,
  merging adjacent ones.
- `crates/fe-ui/src/palette.rs`: `PaletteFrame`, a `RenderOnce` overlay. It's a centred, top-anchored
  box 600 px wide (narrower if the window is), with an input slot, a status line and a list slot. It
  uses `UiTheme` tokens. If a token is missing, add it to `theme.rs` only if unavoidable; that file is
  not in this card's scope, so prefer reusing tokens.
- `crates/fe-ui/src/lib.rs`: `mod` and `pub use` lines only.

### fe-workbench

- `crates/fe-workbench/src/picker.rs`: `Picker<D: PickerDelegate>`, a reusable entity. The command
  palette follow-up on the action-model task will reuse it.
  - It holds a `TextInput`, a selected index and a `uniform_list` of rows.
  - The delegate supplies:
    - `update_matches(query, cx)` (it may be asynchronous and notify the picker when done)
    - `match_count()`
    - `render_match(ix, selected)`
    - `confirm(ix, cx)`
    - `dismissed(cx)`
  - Up and down move the selection, clamped at the ends, and keep it scrolled into view. Enter
    confirms the selected row. Escape dismisses. Clicking a row confirms it.
- `crates/fe-workbench/src/quick_open.rs`: `QuickOpen`, the delegate over a `FinderIndex`.
  - **The index is built lazily.** The first time the palette opens with a project root, `walk` runs on
    `cx.background_spawn`. Its batches reach the index through `extend` as they arrive, so queries see
    what has been indexed so far (T62's `queries_see_paths_indexed_so_far`). The index is kept for the
    life of the workbench, and later openings don't walk again.
  - While the walk runs, the status line reads "Indexing… N files". Afterwards it shows the result
    count.
  - **Queries run off the main thread.** The index sits behind an `Arc<Mutex<_>>`. Each keystroke
    triggers `update_matches`, which runs `query(text, 50)` on the background executor. At most one
    query is in flight. A query typed while one runs replaces any pending query, never queues behind
    it, and runs when the current one finishes. Results whose query text no longer matches the input
    are dropped.
  - Rows show the relative path in a `HighlightedLabel`, using the match's `indices`.
  - Confirming opens the path in the active pane through the same entry point a tree click uses (T63:
    a path that's already open activates its tab). Then the palette closes and the editor is focused.
  - Dismissing returns focus to the element focused before the palette opened.
  - With no project root, the palette opens with "No folder open" and starts no walk.
  - **Watcher batches** reach the index as well as the tree. Add the smallest hook in
    `project_panel.rs` to fan the batches that `ProjectPanel::apply_fs_changes` receives out to the
    quick-open index, for example an event the workbench subscribes to. They're then applied with
    `FinderIndex::apply`. Batches that arrive during the walk are held and applied once it finishes.
    Batches before the index exists are dropped, since the walk will see the disk as it is.
- `crates/fe-workbench/src/workbench.rs`: owns the `QuickOpen` picker. It renders `PaletteFrame` over
  the workbench while the picker is open, and closes it when it loses focus. Keep this to a small hunk.
  `pane.rs` changes only if opening a path needs exposing as a method.
- `crates/fe-workbench/src/lib.rs`: `mod` lines, and one line in `init` to register this card's
  commands.

### Commands

Register every new command through the action-model task's registry. Each gets a stable ID, a default
binding and a context. Don't add `KeyBinding::new` or `actions!` to `fe-workbench` or `fe-ui`. The
registry's GPUI adapter is the only bridge. Proposed IDs follow; use the registry's naming convention
if it differs.

| ID | Default binding | Context |
|---|---|---|
| `quick_open.toggle` | `cmd-p` | `Workbench` |
| `picker.select_prev` / `picker.select_next` | `up` / `down` (also `ctrl-p` / `ctrl-n`) | `Picker` |
| `picker.confirm` | `enter` | `Picker` |
| `picker.dismiss` | `escape` | `Picker` |
| text input editing commands | as above | `TextInput` |

### Tests

New file `crates/fe-workbench/src/tests/quick_open.rs`, declared in `tests/mod.rs`. It uses headless
GPUI, `fe_project::fixture`, and `tempfile`. Test-only counters record the walks started and the
queries run. Commands are triggered by their default keystrokes (`simulate_keystrokes`), and text by
`simulate_input`.

The merge cap is 1,000 changed lines, not counting tests and docs. If the card runs over, split
`TextInput` and `Picker` into a card of their own, rather than trimming behaviour.

## Out of scope

- Recent-files ordering.
- Opening at a line (`path:42`).
- Showing ignored files.
- A symbol picker.
- The command palette. It's a follow-up on the action-model task and reuses `Picker`.
- Caret movement and IME in `TextInput` (M6).

## Decided (lead defaults; the user may revisit)

- **The index is built on first Cmd-P, not at startup**, so it costs the 150 ms cold-start budget
  nothing. The first Cmd-P on a large repo shows partial results while indexing.
- 50 results at most, ranked as T62 ranks them. Rows show the whole relative path, not a separate file
  name and directory.
- `cmd-p` toggles: pressing it again closes the palette.
- With no folder open, Cmd-P says so. It doesn't list the open tabs.
