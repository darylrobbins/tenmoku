---
task_uid: "01m7r7tw20sfxpfqdg8hy7er04"
task: "T428"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::tool_window_placement_followups::references_results_reveal_where_references_is_placed passes: with References moved to the bottom panel, the paths at workbench_chrome.rs:759 and references_panel.rs:704/741 show it there (not sidebar.show), and Find Usages, Show References and History's equivalents reveal results in the dock that holds the panel"
    required: true
    manual: false
  - id: "C2"
    text: "tests::tool_window_placement_followups::moved_panels_keep_their_callers_working passes: a Chat moved to the bottom still refreshes its editor context (chat_panel.rs:1702); Quickfix moved either way closes and toggles (quickfix_commands.rs:255/299); the History and Git log quit handlers (commit_rows.rs:155, git_log.rs:2197) hide the dock that holds them; Show Debugger with Debug visible in the sidebar does not hide it (run_widget_debug.rs:728)"
    required: true
    manual: false
  - id: "C3"
    text: "tests::tool_window_placement_followups::lazy_panels_restore_into_their_saved_dock passes: a saved layout placing History, Call Hierarchy, Quickfix and AI Changes (with group membership and active state) in a fresh workbench where they are not yet registered brings each back in its saved place with its foot action and title (create them before restoring, or keep pending group/active state; dock.rs:1053, workbench_chrome.rs:1533/1561/1569/1610)"
    required: true
    manual: false
  - id: "C4"
    text: "tests::tool_window_placement_followups::relocating_a_focused_tool_clears_the_window_key_return passes: enter Problems from the sidebar by a window key, move Problems to the sidebar, show Terminal, \u003cC-w\u003ek from Terminal lands in the editor, not the sidebar (bottom_from_sidebar and bottom_watch recomputed on relocation, workbench.rs:522, workbench_chrome.rs:306)"
    required: true
    manual: false
  - id: "C5"
    text: "tests::tool_window_placement_followups::foot_actions_read_titles_when_drawn passes: a moved sidebar panel's foot action shows its current title after a locale switch or title change with no re-sync (dock.rs:1053)"
    required: true
    manual: false
  - id: "C6"
    text: "tests::tool_window_placement, tests::bottom_panel_groups, tests::bottom_panel_b3, tests::activity_rail, tests::rail_b3, tests::sidebar_b3, tests::runs_panel, tests::debug_panel, dock_leave_tests, dock_focus_tests, quickfix_commands_tests, chat_panel_tests and git_log_tests pass unmodified; no golden moves"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench::l10n.ui_text_is_in_the_catalogue and fe-workbench::l10n.keys_used_here_exist pass"
    required: true
    manual: false
---

# Follow-ups from review of T1935: placement-aware routing for every show/hide/refresh caller, lazy panels restored into their saved dock, bottom_from_sidebar cleared on relocation, foot action titles read live (Mac T1960)

# Follow-ups from review of T1935

Four medium findings of argon's review of T1935 (merged as 8022dbf7c), below merge.block_severity. Source: argon lead, ~/coordination/from-argon-lead-t1935-medium-findings.md. Fix all four; one regression test per caller in the new tests/tool_window_placement_followups.rs (declared from tests/mod.rs).

- F1 medium placement-aware routing is incomplete. After References moves to the bottom, the paths at workbench_chrome.rs:759 and references_panel.rs:704/741 still call sidebar.show, so results are not revealed. A moved Chat stops refreshing its editor context (chat_panel.rs:1702). Quickfix close and toggle stop closing (quickfix_commands.rs:255/299). The History and Git log quit handlers hide the wrong dock (commit_rows.rs:155, git_log.rs:2197). Show Debugger hides an already visible Debug panel placed in the sidebar (run_widget_debug.rs:728). Fix: route each through the placement-aware show/shown/hide helpers T1935 added; one regression per caller.
- F2 medium restoring placements only transfers panels already registered (workbench_chrome.rs:1533/1561/1569/1610, dock.rs:1053). History, Call Hierarchy, Quickfix and AI Changes are absent in a fresh workbench, so their saved group membership and active state are dropped and their foot actions omitted (panel_title None). Fix: create the lazy panels before restoring, or keep pending group/active state until they register.
- F3 medium relocating a focused bottom tool keeps a stale bottom_from_sidebar (workbench.rs:522, workbench_chrome.rs:306). Enter Problems from the sidebar with a window key, move Problems to the sidebar, show Terminal: <C-w>k from Terminal returns to the sidebar, not the editor. Fix: clear or recompute bottom_from_sidebar and bottom_watch on relocation.
- F4 medium moved sidebar panels' foot actions capture their titles only when synced (dock.rs:1053). Fix: read the title when the action is drawn.

Not in this card: Explorer moving, plugin panels (T1708), groups in the sidebar. With no panel moved nothing changes.

Overlap: T1708 names crates/fe-workbench/src/dock/mod.rs (conflicts if dock.rs becomes dock/mod.rs); T1793 and T1794 name chat_panel.rs; many ready cards add one line to tests/mod.rs. Land one, rebase the other.
