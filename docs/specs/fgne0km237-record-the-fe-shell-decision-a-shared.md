---
task_uid: "01m4f6m1247z23k5fgne0km237"
task: "T242"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/decisions/app-shell.md exists in the format of lsp-client.md/command-model.md (title, a Status line dated 2026-10-08) with six numbered sections: Decision and why, Crate boundary, PaneItem trait sketch, Day-one host needs, Staged extraction plan, Open questions"
    required: true
    manual: false
  - id: "C2"
    text: "Crate boundary marks each of crates/fe-workbench/src/{dock,pane,pane_group,pane_drag,pane_toolbar,picker,quick_open,toolbar_picker,title_bar,status_bar,persist,trust}.rs as generic, fastedit-specific or split, and limits fe-shell's dependencies to gpui, fe-ui, fe-command, fe-keymap and fe-config, explicitly excluding fe-editor, fe-gpui, fe-buffer, fe-syntax, fe-lsp, fe-nav and fe-project"
    required: true
    manual: false
  - id: "C3"
    text: "PaneItem trait sketch gives Rust signatures covering identity, title/icon, dirty and close veto, focus, toolbar contribution and serialise/restore, says how the closed enums DockPanel and Palette become host-registered, and shows EditorView and a non-editor (chat) item implementing it"
    required: true
    manual: false
  - id: "C4"
    text: "Day-one host needs states how each is met: multiple windows (what is per-window vs global), non-editor panes, host commands and keymaps via fe-command/fe-keymap with a command palette, host theming via T238, host status items, and layout persistence (noting persist.rs persists no layout today)"
    required: true
    manual: false
  - id: "C5"
    text: "Staged extraction plan lists follow-up cards in order, each sized s or m with its dependencies, PaneItem trait before the fe-shell crate, every extraction card after T144 and T188, ordered against T219, T221, T231, T145-T147 and T235-T237; the candidate changes no file other than docs/decisions/app-shell.md"
    required: true
    manual: false
---

# Record the fe-shell decision: a shared app shell the host app and fastedit both build on

# Record the fe-shell decision: a shared app shell the host app and fastedit both build on

A decision-record card, like T100 (`docs/decisions/lsp-client.md`) and T68
(`docs/decisions/command-model.md`). The output is one document; **no code
changes.** The extraction cards it lists are drafted afterwards from it.

## Problem

The user decided on 2026-10-08 (~22:05) that the host app — the AI-first dev
tool replacing Eclipse Theia (`~/.claude/plans/glittery-foraging-graham.md`,
line 4; "The host app is GPUI", line 14) — and fastedit's standalone app share
one app shell, a new `fe-shell` crate in this repo. fastedit's editor and panels
become pane items that shell hosts. Nothing records that decision, the crate
boundary, or the order of the cards that get there.

Today the shell is welded into `crates/fe-workbench`, which depends on every
fastedit crate (`fe-buffer`, `fe-editor`, `fe-gpui`, `fe-lsp`, `fe-nav`,
`fe-project`, `fe-syntax`, … — `crates/fe-workbench/Cargo.toml`):

- **Dock, panes, tabs, splits:** `crates/fe-workbench/src/dock.rs` (471 lines;
  `DockPanel` is a closed enum `Project | Search | Outline | References`),
  `pane.rs` (1153; `Tab` holds `editor: Entity<EditorView>` and its `TabId` is
  the editor's `EntityId`), `pane_group.rs` (716; `PaneGroup`, `Member`, `Axis`,
  `Direction`), `pane_drag.rs` (143), `pane_toolbar.rs` (246).
- **Pickers and palette:** `picker.rs` (322; `PickerDelegate`, `Picker<D>`),
  `quick_open.rs` (371), `toolbar_picker.rs` (640; imports `EditorView` and
  `fe_syntax::Outline`). `workbench.rs:88` `Palette` is a closed enum
  (`QuickOpen | Language | Toolbar | Definitions | Symbols`); there is no command
  palette listing `fe-command`'s registry yet.
- **Title bar and status bar:** `title_bar.rs` (107), `status_bar.rs` (420;
  `StatusItem`, `StatusSide`, `StatusBar`), on `crates/fe-ui/src/status_bar.rs`
  (118).
- **Persistence and trust:** `persist.rs` (221) saves only the Vim and status
  bar toggles to `settings.toml` — no pane, split or dock layout is persisted
  today. `trust.rs` (408) wraps `crates/fe-config/src/trust.rs`'s `TrustStore`.
- `workbench.rs` (1275) wires all of it to the editor; `crates/fastedit/src/main.rs:265`
  opens exactly one window.

## Change

Write `docs/decisions/app-shell.md` in the format of `lsp-client.md`,
`command-model.md` and `vim-engine.md`: a title, a `Status: **build**. Decided
2026-10-08.` line naming this card, then these numbered sections.

1. **Decision and why.** One shared `fe-shell` crate, in this repo, that the
   host app and fastedit's standalone app both build on; fastedit's editor and
   panels are pane items it hosts. Say why one shell beats two (the host gets
   docks, splits, pickers, trust for free; fastedit's app stays a thin client of
   the same shell, so the shell is exercised daily) and why it lives here.
2. **Crate boundary.** A table of the files above: each marked generic (moves to
   `fe-shell`), fastedit-specific (stays in `fe-workbench`), or split (and along
   what line). `fe-shell` may depend on `gpui`, `fe-ui`, `fe-command`,
   `fe-keymap` and `fe-config` only — never on `fe-editor`, `fe-gpui`,
   `fe-buffer`, `fe-syntax`, `fe-lsp`, `fe-nav` or `fe-project`. State where
   `fe-workbench` ends up: fastedit's pane items and panels on top of `fe-shell`.
3. **`PaneItem` trait sketch.** Rust signatures, not an implementation: identity
   (replacing `TabId = EntityId` of an editor), title and icon, dirty state and
   save/close veto, focus handle, optional toolbar/breadcrumb contribution (what
   `pane_toolbar.rs` reads from the editor today), split/clone behaviour, and
   serialise/restore for layout persistence. Say how `Tab`, `DockPanel` and
   `Palette` stop being closed enums (dock panels and pickers registered by the
   host). Show `EditorView` and one non-editor item (a chat pane) implementing it.
4. **Day-one host needs, each with how it's met:**
   - multiple windows — one shell root per window, what's per-window vs global
     (trust store, keymap, theme, command registry);
   - non-editor panes (chat, agent logs, terminals, diff review) — as
     `PaneItem`s and dock panels;
   - host-provided commands and keymaps — registered through `fe-command` and
     `fe-keymap`, with the command palette over that registry;
   - theming from the host — via T238's GPUI-free themes in `fe-config`, mapped
     to `fe-ui`'s `UiTheme`;
   - status items the host contributes, and layout persistence (pane tree, dock
     state, open items) — new, since `persist.rs` persists no layout.
5. **Staged extraction plan.** An ordered list of follow-up cards, one line
   each, sized `s` or `m`, with their dependencies: `PaneItem` trait in
   `fe-workbench` with `EditorView` behind it first, then the `fe-shell` crate
   and the moves. Every extraction card depends on T144 (LSP go-to-definition)
   and T188 (visual.rs split) landing, and must also be ordered after the open
   cards in the same files: T219 and T221 (`workbench.rs`, `status_bar.rs`),
   T231 (`status_bar.rs`), and the LSP workbench cards T145–T147 and T235–T237
   (`fe-workbench/src/lsp.rs`, `tests/mod.rs`) — say how for each (depends-on,
   or queued after).
6. **Open questions** for the user, each with a recommended answer.

Cite files by path and symbol as of the commit you read; don't invent APIs the
code doesn't have without marking them as proposed.
