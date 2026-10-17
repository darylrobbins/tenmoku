---
task_uid: "01m53q0vcccey5wkbbwcds6drz"
task: "T119"
rev: 3
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "dock.rs has no match on a DockPanel: name, id, of, show_command, show_op and panel_icon read the SidebarPanel each panel module declares (const SIDEBAR), collected once in PANELS; DockPanel is a PanelId newtype whose associated consts keep every existing DockPanel::\u003cName\u003e use compiling"
    required: true
    manual: false
  - id: "C2"
    text: "sidebar_panels_tests.rs: the rail lists Project, Search, Outline, Changes and the Git Log panel in their base order with their base icons and titles, References and Problems join once shown, and each panel's show command shows it"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited except the one import change at dock_position_tests.rs:252 the lead's note allows (use DockPanel::* replaced for the new spec type, every assertion kept), and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Replace the sidebar's closed DockPanel enum and its five match tables with a SidebarPanel spec declared by each panel's module (Mac T692)

Filed on argon from Mac T692 (card ~/coordination/argon-cards/zo/T692.json rev 3; note zo, the kiln phase-3 chain, card 13 of 17, serial). Mac task uid 01m4vbjkkxv1p4a7arzed1vbdz. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T118 (Mac T703), the card before. Held in the inbox: waits on Mac T609 (Git Log panel); the Mac lead clears it. Title the host commit "... (Mac T692)".

## Problem

The sidebar's panels are a closed `pub enum DockPanel { Project, Search, Outline, Changes, References, Problems }`
with five parallel tables, `name`, `id`, `of`, `show_command`, `show_op`, plus `panel_icon` and `ALL`
(`crates/fe-workbench/src/dock.rs` ~36-125), and a `dock_item!` macro per panel (~127). Adding a panel (T609's Git
Log) means editing every table. kiln's `Contributions::add_panel`/`#[kiln::panel]` don't fit this dock: they
target an `ItemPane` workspace's own docks (`AnyDockPanel` takes `&mut Workspace`, `kiln/crates/kiln/src/contributions.rs`
~238) and add a `<id>.toggle` command per panel, while fastedit's sidebar is one `Dock` showing one panel at a time
under fastedit's own show commands. 59 uses of `DockPanel::<Variant>` in 11 test files must keep compiling.

## Change

- `DockPanel` becomes `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct DockPanel(PanelId)` with an
  associated `pub const` per built-in panel spelled as today's variants (`DockPanel::Project`, ...;
  `#[allow(non_upper_case_globals)]`), so every existing use compiles unchanged.
- New `pub(crate) struct SidebarPanel { panel: DockPanel, name: &'static str, icon: Icon, show_command: &'static str,
  show_op: DockOp, order: i32, always_on_rail: bool }` in `dock.rs`. Each panel's module declares its own
  `pub(crate) const SIDEBAR: SidebarPanel` (`project_panel.rs`, `search_panel.rs`, `outline_panel.rs`,
  `git_panel.rs`, `references_panel.rs`, `problems_panel.rs`, and T609's Git Log panel's module), and `dock.rs`
  keeps a single `const PANELS: &[&SidebarPanel]` sorted by `order` for the rail. `name`, `id`, `of`,
  `show_command`, `show_op`, `panel_icon` and `ALL` become lookups in it.
- Rail order, rail membership (References and Problems join once shown), titles, icons and show commands are
  unchanged.
- New tests in `crates/fe-workbench/src/sidebar_panels_tests.rs`.

## Rules for every card of this chain

Phase 3 of the Kiln app layer (`docs/decisions/kiln-app-layer.md`, approved 2026-10-13). The chain runs serially,
in this order: T700 (kiln), T690, T691, T693, T694, T695, T697, T698, T699, T701, T702, T703, T692, T696, T704,
T705, T706.

- No command ID or key context is renamed (`app-framework.md` §4): every moved command keeps its ID with
  `id = ".."` and its context with `context = ".."`.
- No existing fastedit test is edited; behaviour is proven by the existing suites plus the new test file each
  card names.
- Nothing on the paint path changes: `EditorElement`, `TerminalElement` and `FileViewerElement` stay raw
  `impl Element`.
- The keymap and menus stay TOML tables (`crates/fe-keymap/keymaps/*.toml`, `menus/*.toml`), untouched.
- `crates/fastedit/src/main.rs` is not touched: `fe_workbench::init` registers what the chain adds (neon's T707
  owns `main.rs` and the keymap files).
## Lead's note (argon, 2026-10-17; approved by the Mac lead in note zzg)

C1 and C3 conflicted at crates/fe-workbench/src/dock_position_tests.rs:252, which glob-imports the old enum's variants
(`use DockPanel::*;`). A per-panel spec type can't be glob-imported from. C3 is narrowed to allow exactly the one change
that line needs: replace `use DockPanel::*;` with whatever import or names the new SidebarPanel/PanelId spec takes, and
keep every assertion in that file as it is. dock_position_tests.rs is added to the scope.

## Implementation notes

- Done as specified: `DockPanel(PanelId)` with `#[allow(non_upper_case_globals)]` consts, each read from
  its module's `SIDEBAR` (`DockPanel::Project = project_panel::SIDEBAR.panel`, ...), so the panel's id is
  declared in its module too. `SidebarPanel` also carries `focus_on_show`, the last per-panel argument of
  `dock_item!`, which now takes the panel type and its `SIDEBAR` (plus a title method for History and Git Log).
- argon's base also has the History panel (`git_history.rs`, not in the card's scope); it declares a
  `SIDEBAR` as well, or `dock.rs` would keep a match for it.
- `PANELS` is written in rail order; a `const` assertion checks `order` rises along it, and `DockPanel::ALL`
  is computed in a `const fn` from its `always_on_rail` entries. `Dock::run` finds the panel a show op shows
  by `show_op` in `PANELS` instead of a `DockOp` → `DockPanel` table.
- Rail order is still registration order (kiln-shell's `DockState`): the four always-on panels, then
  References and Problems as registered in `Dock::new`, then History and Git Log in the order first made.
- **C1/C3 conflict.** `dock_position_tests.rs:252` (T603) does `use DockPanel::*;`, a glob import of the
  enum's variants. No struct can be glob-imported from (`DockPanel` is a struct, not a module, E0432), and a
  module of the same name would clash with the type, so the newtype C1 requires can't compile that test
  unedited, as C3 requires. Verified locally with that one line rewritten to spell `DockPanel::Project`, ...
  (not committed): `cargo test -p fe-workbench -p fastedit` passes. Blocked for the lead to narrow C3 to
  allow that edit, or to keep `DockPanel` an enum with every table read from `SIDEBAR`.
- Resolved by the lead's note (rev 3): `assert_panels_fill` drops `use DockPanel::*;` and names the six panels
  `DockPanel::Project`, ... in its loop's array, every assertion as it was. A one-line swap to a glob-importable
  module of consts would leave the file's `DockPanel` import (line 19) unused, which `-D warnings` rejects.
