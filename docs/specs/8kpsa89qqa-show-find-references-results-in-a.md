---
task_uid: "01m48zh7h6y7rf0h8kpsa89qqa"
task: "T112"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::references_panel::shift_f12_shows_the_panel: with the caret on helper in src/main.rs and the dock hidden, shift-f12 shows the dock switched to References with the header References to `helper`; the dock header has no References tab before the first shift-f12 and has one after; focus is in the panel list with the first reference row selected"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::references_panel::results_stream_grouped_by_file: with a fake provider releasing 3 FileReferences batches one at a time, rows grow after each release without earlier rows moving; there is one header row per file with its count and one row per reference with its line number and preview_ranges highlighted; an unverified reference row is dimmed and an is_definition row is tagged definition; the status reads Finding references to `helper`… until the summary, then 5 references in 3 files"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::references_panel::search_provider_end_to_end: over Rust files where helper is defined in src/util.rs, called in src/main.rs and src/a.rs, and mentioned in a // comment, shift-f12 with SearchNavigation lists src/main.rs first, lists the definition and both calls but not the comment hit, and the status ends with (by search)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::references_panel::click_and_enter_open_a_reference: clicking a reference row opens its file in the active pane with the selection equal to Location.range and the editor focused, also for a CRLF file; down then enter in the panel opens the next reference likewise, skipping header rows; ctrl-minus then returns the caret to where shift-f12 was pressed"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::references_panel::a_new_run_cancels_the_old: a second shift-f12 on another word before the first run's batches are released sets the first run's cancel flag and every row afterwards comes from the second word; switching the dock to Project and back to References keeps the rows and does not call the provider again"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::references_panel::empty_states: caret on whitespace shows No symbol at the caret; a word with no uses shows No references to `word`; a workbench with no project root shows No folder open and calls no provider; a truncated summary shows Showing the first N references"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test tests::references_panel::only_visible_rows_are_built: with 1,000 references a frame builds no more rows than fit in the panel plus the uniform_list overscan"
    required: true
    manual: false
  - id: "C8"
    text: "nav.find_references (shift-f12, workbench context) and references_panel.select_prev/select_next/open (up/down/enter, ReferencesPanel context) are registered through the command registry and bound in default-macos.toml; rg 'KeyBinding::new|actions!' finds no match in the files this card adds; the binding counts in fe-gpui command_tests.rs and keymap_tests.rs are updated and pass; Cargo.toml and Cargo.lock are unchanged; T81's golden screenshot check passes unchanged"
    required: true
    manual: false
---

# Show find-references results in a References panel in the right dock (shift-F12)

# Show find-references results in a References panel in the right dock (shift-F12)

Milestone M4 "Navigate". The user decided (2026-10-06) that references show in a **dock panel**, not
in a picker. T106 finds references by ripgrep word search, streamed in per-file batches; T107 wires
`Navigation` into the workbench but leaves references out. This card adds `nav.find_references`
(shift-F12) and a References panel in the right dock, beside T77's Search and T78's Outline, and
modelled on them.

It builds on:

- T106: `Navigation::references(req, cancel, sink)`, `FileReferences { rel, refs }`, `Reference {
  location, line_number, preview, preview_ranges, verified, is_definition }`, `ReferencesSummary`, and
  `Answer { provider, precision }`
- T107: `nav.rs`'s `pub(crate)` seams: build a `NavRequest` from the active editor, run a request on
  the background executor with a generation and cancel flag, `navigation()`, `push_history`, and
  "open `rel` at `range` in the active pane"
- T77: `Dock`, `SearchPanel`'s streaming pattern (`cx.background_spawn`, batches over T63's channel,
  generation check, `uniform_list` rows, header rows per file, status line), `HighlightedLabel`
- T78: the Outline entry in `Dock`, and its "focus stays / follows" conventions
- T80: `project_panel.select_prev/select_next/open` as the pattern for keyboard row selection in a
  dock panel
- the command registry (T68)

## Problem

After T107, shift-F12 does nothing. References can number in the thousands and arrive over seconds,
so they need a list that streams, stays put while you work through it, and survives switching
panels, which a modal picker doesn't.

## Change

### `crates/fe-workbench/src/references_panel.rs` (new): `ReferencesPanel`

- **Running.** `nav.find_references` builds a `NavRequest` through T107's seam (the newest
  selection's head in the active editor) and runs `Navigation::references` on the background
  executor. Each run has its own generation and cancel flag (separate from T107's go-to-def
  generation); starting a run sets the previous run's flag and batches from older generations are
  dropped. Dropping the panel or closing the workbench cancels the run.
- **Rows** (a `uniform_list`, only visible rows built), in arrival order, never re-sorted:
  - a header row per `FileReferences`: the `rel` path and its count. The request's own file comes
    first because T106 emits it first.
  - one row per `Reference`: the 1-based `line_number` and the `preview` in a `HighlightedLabel` with
    `preview_ranges`. Unverified references (`verified: false`) are dimmed; definitions
    (`is_definition`) carry a muted "definition" tag.
- **Header and status line:**
  - "References to `word`" while showing results for `word`
  - "Finding references to `word`…" while running
  - "N references in M files", with " (by search)" when the answering provider's precision is
    `Search`
  - "No references to `word`"; "No symbol at the caret" when the request has no word; "No folder
    open" with no project root (no request runs)
  - "Showing the first N references" when the summary is `truncated`
- **Selection and opening.** The panel keeps a selected reference row (header rows are skipped).
  `up`/`down` move it, clamped, keeping it in view; `enter` or a click opens it: T107's
  `push_history` with the caret where it was, then open `rel` in the active pane and select
  `location.range` (offsets are into the loaded text, per T103/T106, so CRLF files land right), with
  the editor focused.
- **Keeping results.** Switching the dock to another panel and back shows the same rows without a
  new run; results stay until the next shift-F12.

### `crates/fe-workbench/src/dock.rs`

`DockPanel::References`, its body, and its header tab. The References tab appears in the header strip
only once the panel has been used in this workbench, so the dock at startup (and T81's golden
screenshot) is unchanged. Nothing else in `dock.rs` changes.

### Wiring

- `crates/fe-workbench/src/workbench.rs`: create the panel and hand it to `Dock`; a small hunk.
- `crates/fe-workbench/src/nav.rs`: only if a seam the panel needs is missing from T107; keep it to a
  small hunk.
- `crates/fe-workbench/src/lib.rs`: a `mod` line and one line in `init` to register commands.

### Commands

Registered through the registry. No `KeyBinding::new` or `actions!`.

| ID | Default binding | Context | Effect |
|---|---|---|---|
| `nav.find_references` | `shift-f12` | `workbench` | shows the dock, switches it to References, starts a run and focuses the panel's list with the first reference selected as soon as one arrives |
| `references_panel.select_prev` / `references_panel.select_next` | `up` / `down` | `ReferencesPanel` | move the selected reference |
| `references_panel.open` | `enter` | `ReferencesPanel` | open the selected reference |

Update the binding counts pinned in `crates/fe-gpui/src/command_tests.rs` and
`crates/fe-gpui/src/keymap_tests.rs`.

### Tests

New file `crates/fe-workbench/src/tests/references_panel.rs`, declared in `tests/mod.rs`: headless
GPUI over a `tempfile` fixture, commands triggered by their default keystrokes, clicks on laid-out
row bounds as in T77's tests. For deterministic streaming, a test registers a fake
`NavigationProvider` through `Workbench::navigation()` at a higher precedence that sends batches
when the test releases them; one test runs the real `SearchNavigation` end to end. Test-only
counters record rows built and provider calls.

## Out of scope

- A references picker, peek views, "find all implementations", rename.
- Grouping by containing symbol, collapsing file groups, sorting, filtering.
- F4-style next/previous reference from the editor, escape to return to the editor.
- Highlighting references inside the editor.
- Cmd-click (a later card).

## Decided (lead defaults; the user may revisit)

- **References live in the right dock** (user decision), one panel at a time with Project, Search and
  Outline, as VS Code's references view lives in a sidebar.
- **Shift-F12 moves focus to the panel** (unlike `outline.show`), so up/down/enter work at once.
- The References header tab appears on first use, not at startup.
- Results stay until the next shift-F12; opening one pushes the back history like a go-to-def jump.
