---
task_uid: "01m48txxgzb8e7ty1gzxbe38vw"
task: "T107"
rev: 8
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::nav::goto_definition_jumps_across_files: with src/main.rs open and the caret on helper in helper(), f12 opens src/util.rs in the active pane with the selection equal to the name range of fn helper, and the editor focused; no picker opens"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::nav::ambiguous_definitions_open_a_picker: with two Project-tier helper definitions, f12 opens the definitions picker with 2 rows in answer order and a status naming the search provider; down then enter jumps to the second; with no definition, the picker shows No definition found for `word`"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::nav::works_before_the_index_is_built: with the build paused by a test hook, the first f12 still jumps to a definition in another file (fallback answer with partial set); no nav build starts after Workbench::new until the first nav command, and a second command does not start another build"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::nav::back_and_forward: after an f12 jump, ctrl-minus reopens main.rs with the caret at its offset before the jump and ctrl-shift-minus returns to the definition; a new jump after going back clears forward; history holds at most 100 entries"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::nav::workspace_symbols_picker: cmd-t then typing hlp lists helper with highlighted matched chars and its rel:row, enter jumps there; while the build is paused the status reads Indexing… N files; at most one symbols query runs at a time and the newest pending query replaces older ones"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::nav::watcher_batches_reach_the_nav_index: after the build, writing a new file with fn fresh() and delivering its batch through the fan-out hook makes cmd-t fresh list it; deleting the file and delivering its batch removes it"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test tests::nav::stale_requests_are_dropped: two f12 presses on different words with the first answer held by a test hook jump only to the second word definition, and the first request cancel flag is set"
    required: true
    manual: false
  - id: "C8"
    text: "nav.goto_definition (f12), nav.workspace_symbols (cmd-t), nav.back (ctrl-minus) and nav.forward (ctrl-shift-minus) are registered through the command registry in the workbench context and bound in default-macos.toml; nav.find_references is not registered by this card; fe-workbench adds no KeyBinding::new or actions!; the binding counts in fe-gpui command_tests.rs and keymap_tests.rs are updated and pass"
    required: true
    manual: false
  - id: "C9"
    text: "Cargo.lock changes only fe-workbench dependency list (gains fe-nav, no new crates); scripts/check-no-gpui.sh still prints ok: fe-nav"
    required: true
    manual: false
  - id: "C10"
    text: "The status bar (T125) shows the navigation source ('by search') for the last go-to-definition or symbol answer, added through StatusBar::set_item"
    required: true
    manual: false
---

# Go to definition and search workspace symbols in the workbench, by search, with back and forward

# Go to definition and search workspace symbols in the workbench, by search, with back and forward

Milestone M4 "Navigate", the UI half of `fe-nav`. M4 is done when "go-to-def works instantly by
search, then precisely via" the language servers. This card puts T105's `Navigation` (with the
search provider) behind workbench commands for go-to-definition (F12) and workspace symbols (cmd-T),
keeps T103's index built and current (T104), and adds back/forward. Find references (shift-F12) is
the next card: the user decided references show in a **dock panel**, not a picker, and that panel
would push this card past the 1,000-line cap. `fe-lsp` later adds its provider to the same
`Navigation`; this UI doesn't change for it.

It builds on:

- T105: `Navigation`, `SearchNavigation`, `NavRequest`, `Definitions` (`unambiguous`, `partial`),
  `workspace_symbols`, `Answer { provider, precision }`
- T103 / T104: `fe_nav::build`, `SymbolIndex::{extend, finish, plan}`, `extract`
- T76: `Picker`/`PickerDelegate`, `PaletteFrame`, `HighlightedLabel`, the lazy-build pattern of
  quick-open, and its watcher fan-out hook from `ProjectPanel::apply_fs_changes`
- T77: opening a path in the active pane then `select_and_reveal` once its editor exists (and the
  `pane.rs` hook for files still loading, if T77 added one)
- T79: the active pane among splits
- the command registry (T68): commands with stable IDs, default bindings and contexts

It also waits for T81, T82 and T97 (they touch `fe-workbench/Cargo.toml`, `tests/mod.rs` and
`pane.rs`) and for T108's numbers, which may tune T105's fallback before the UI depends on it.

## Change

### `crates/fe-workbench/src/nav.rs` (new)

- **The index is built lazily**, like quick-open's: the first nav command in a workbench with a
  project root starts `fe_nav::build` on `cx.background_spawn`, with batches `extend`ed into an
  `Arc<RwLock<SymbolIndex>>` and `finish` at the end (its returned `Reindex` extracted as below).
  Later commands don't build again. With no root, nav commands say "No folder open" and build nothing.
- **Watcher batches** reach the index through T76's fan-out hook: `plan` on the background
  executor, `extract` the returned paths without the lock, then `extend`. Batches before the index
  exists are dropped (the build will see the disk as it is).
- The workbench owns one `Arc<Navigation>` with `SearchNavigation` at precedence 0, and exposes
  `Workbench::navigation()` so a host (and later `fe-lsp`) can `add` providers.
- **Requests** are built from the active pane's editor: its path relative to the root, its language
  name, its current text snapshot and the newest selection's head as `offset`. They run on the
  background executor. Starting a request sets the previous request's cancel flag; answers from an
  older generation are dropped.
- **Seams for the references panel card** (`pub(crate)`, small): building a `NavRequest` from the
  active editor, running a request on the background executor with the generation/cancel rule above (one
  generation per caller, so a references run doesn't cancel a go-to-def and vice versa),
  `navigation()`, and `push_history(cx)` plus "open `rel` at `range` in the active pane" used by
  jumps. The next card calls these rather than duplicating them.
- **Back and forward:** a `NavHistory` of `(path, offset)` entries, at most 100. Each jump made by a
  nav command first pushes where the caret was; a new jump clears the forward stack. Going back or
  forward opens the path and selects the offset (clamped by `select_and_reveal`).

### `crates/fe-workbench/src/nav_picker.rs` (new): two `PickerDelegate`s

- **Definitions**, when an answer is not `unambiguous`: rows `name  rel:row  container`, in the
  answer's order, the first selected. Confirm jumps.
- **Workspace symbols**: each keystroke runs `workspace_symbols(query, 50)` on the background
  executor, at most one in flight and the latest pending query replacing any older one (T76's rule).
  Rows: the name highlighted with `indices`, the kind in words, then `rel:row`. While the index
  builds, the status line reads "Indexing… N files".
- Status lines name the provider when it isn't exact: "… (by search)".

### Commands

Registered through the registry. No `KeyBinding::new` or `actions!`.

| ID | Default binding | Context | Effect |
|---|---|---|---|
| `nav.goto_definition` | `f12` | workbench | `unambiguous` → jump; several → definitions picker; none → a picker with no rows and "No definition found for `word`" |
| `nav.workspace_symbols` | `cmd-t` | workbench | symbols picker |
| `nav.back` | `ctrl--` (ctrl-minus; use `fe-keymap`'s spelling) | workbench | back |
| `nav.forward` | `ctrl-shift--` | workbench | forward |

Update the binding counts pinned in `crates/fe-gpui/src/command_tests.rs` and
`crates/fe-gpui/src/keymap_tests.rs`.

### Wiring

- `crates/fe-workbench/Cargo.toml`: `fe-nav.workspace = true` (so `Cargo.lock`'s fe-workbench entry
  changes; no new crates).
- `crates/fe-workbench/src/workbench.rs`: owns the nav state and renders the active nav picker in
  `PaletteFrame`, as quick-open does. Keep it to a small hunk.
- `crates/fe-workbench/src/lib.rs`: `mod` lines, re-exports, and one line in `init`.
- `crates/fe-workbench/src/pane.rs`: only if opening a path at a range needs a method T77 didn't add.

### Tests

New file `crates/fe-workbench/src/tests/nav.rs`, declared in `tests/mod.rs`: headless GPUI over a
`tempfile` fixture, commands triggered by their default keystrokes. Test hooks may pause the build
and count requests.

If the card still runs over the 1,000-line cap, move the workspace symbols picker into a card of its
own rather than trimming behaviour.

## Out of scope

- Find references and its dock panel (`nav.find_references`, shift-F12): the next card.
- Cmd-click and hover underline (needs mouse modifiers in `fe-gpui`): a later card that dispatches
  `nav.goto_definition` from a click.
- Peek views, "go to symbol in file", rename, anything LSP.
- Starting the index at project open; it starts on the first nav command.

## Decided (lead defaults; the user may revisit)

- **VS Code bindings:** F12, cmd-T, ctrl-minus and ctrl-shift-minus (shift-F12 comes with the
  references card).
- **References in a dock panel** (user decision, 2026-10-06), in the next card; this card no longer
  shows references.
- **The index starts on the first nav command**, not at project open, so it costs startup and idle
  CPU nothing; until it is complete, go-to-def uses T105's ripgrep fallback.
- A history of 100 positions, by path and byte offset (not anchors).
