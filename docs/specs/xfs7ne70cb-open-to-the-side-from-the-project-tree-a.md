---
task_uid: "01m7p6n8rbh2g6skxfs7ne70cb"
task: "T685"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::project_open_to_side::file_row_menu_opens_the_file_to_the_side passes: right-click a file row; the menu starts with Open and Open to the Side; choosing Open to the Side with one pane splits right, shows the file in the new right pane, focused, and the left pane keeps its tab"
    required: true
    manual: false
  - id: "C2"
    text: "tests::project_open_to_side::to_the_side_reuses_the_right_pane passes: with a pane already right of the active one, the file opens as a tab there; no third pane"
    required: true
    manual: false
  - id: "C3"
    text: "project_panel.open_to_side is a panel command (palette title 'Open to the Side'), bound to secondary-enter in project_panel, with the key shown in the menu; folder rows and the empty-space menu don't list it"
    required: true
    manual: false
  - id: "C4"
    text: "tests::project_menu and command_catalog_tests pass with the file-row menu now starting Open, Open to the Side, separator"
    required: true
    manual: false
  - id: "C5"
    text: "Checked in the real Linux app (run-fastedit-linux): right-click a file, Open to the Side, the file shows in a right split (screenshot in the decision file)"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-workbench project_open_to_side project_menu command_catalog passes; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Open to the Side from the project tree: a file row's menu has no Open or Open to the Side, so a file can't be opened in a right split (Mac T1942)

User request (Linux tester, approved by the user): "I need 'open in right split' :)"
State: a file row's tree menu is FOLDER_ITEMS[3..] (crates/fe-workbench/src/project_panel_menu.rs:64-82, 252), with no Open and no Open to the Side. The only ways today are pane.split_right (secondary-\), which splits the current file, and vim's :vs {file}.
Design: a MenuItem::Open and MenuItem::OpenToSide at the top of file rows; PanelOp::OpenToSide in the panel's command registry (project_panel.rs:1918-1965) emitting PanelEvent::OpenFileToSide(path). workbench_root.rs:62 handles it by opening in the right neighbour pane if there is one (like move_tab's neighbour lookup), else split_alone(Direction::Right) + pane.open + drop_second_view, the helpers :vs {file} uses (crates/fe-workbench/src/vim_windows.rs:97-170). Quick open's cmd/ctrl-enter needs kiln picker work and is a follow-up card.
Depends on T1938 (same helium chain; shares project_panel.rs, commands.ftl, fastedit.toml). Overlaps T1917 (lent to neon) on workbench_root.rs; rebase on whichever lands first. Do not widen --scope; if a file outside it is needed, stop and say so.

## Helium filing note

- USER-approved feature, lent from the Mac as y143. Depends on helium T681 (Mac T1938), which shares
  project_panel.rs, commands.ftl and fastedit.toml; rebase on it (and T680 / Mac T1937) before starting.
  T680/T681 put their tests under `src/tests/project_new_menu/`, not where the Mac cards named them.
- The real-app check (Xvfb + xdotool) uses the run-fastedit-linux skill; screenshot in the decision file.
  Kill only processes you started, by PID; never pkill/killall by name (other lanes share this host).
- Ask the lead before rgt finish if anything in the criteria can't be met as written.
