---
task_uid: "01m7p7e4dnhpwyg6654yqfxhcd"
task: "T687"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::close_folder::close_folder_returns_to_no_folder passes: a workbench on a root with two tabs (one pinned), Project panel shown; run workbench.close_folder; root() is None, no tab is open, the active pane shows the Start page, the Project panel shows its no-folder state, the title is the no-root name, quick open lists nothing from the old root, and git state, tree watcher and root settings watch are rootless"
    required: true
    manual: false
  - id: "C2"
    text: "tests::close_folder::a_dirty_tab_asks_first passes: with an unsaved tab, close_folder asks as Close All Editors does (Discard unsaved changes to …? Cancel / Discard: the app's bulk-close prompt has no Save button); Cancel keeps the root and every tab; Discard closes them and the folder"
    required: true
    manual: false
  - id: "C3"
    text: "tests::close_folder::the_roots_servers_stop passes: with the fake python server running on tool.py, close_folder shuts it down (shutdown and exit recorded) and no server starts for that root afterwards"
    required: true
    manual: false
  - id: "C4"
    text: "a_closed_folder_is_not_reopened passes (crates/fastedit/tests/close_folder_reopen.rs, with reopen_last_session.rs's harness): launch on a folder, close the folder, quit; a launch with no argument reopens one window with no root; a folder launch on that folder still opens on it"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-keymap/tests/close_folder_key.rs passes: through the kiln Matcher, cmd-k f on macOS and ctrl-k f on Linux run workbench.close_folder in workbench, editor and project_panel stacks; secondary-k secondary-f still runs lsp.format_selection in an editor; on macOS and Linux the File menu lists Close Folder between Close Pane and Close Window with that key"
    required: true
    manual: false
  - id: "C6"
    text: "workbench.close_folder is in the palette as Close Folder and does nothing with no folder open; command_catalog_tests, typed_command_ids_tests and the fe-keymap l10n and menu tests pass with it"
    required: true
    manual: false
  - id: "C7"
    text: "Checked in the real Linux app (run-fastedit-linux): open a folder, dirty a file, File \u003e Close Folder, Discard; the Start page and No folder open show; ctrl-k f on another folder does the same; quit and relaunch with no argument: no folder reopens (screenshots in the decision file)"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-workbench close_folder command_catalog typed_command_ids passes, cargo nextest run -p fe-keymap passes, cargo nextest run -p fastedit --test close_folder_reopen passes; cargo clippy -p fe-workbench -p fe-keymap -p fastedit --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# File > Close Folder: the window goes back to no folder (Start page, No folder open), the root's servers and watchers stop, and the next launch doesn't reopen it (Mac T1943)

User request (approved by the user): there is no way to close a project folder. Closing the window and relaunching reopens it (last-quit restore), and File > Open Folder only re-roots the window (Workbench::set_root, crates/fe-workbench/src/workbench_root.rs:144-211).
Design: workbench.close_folder next to open_folder (crates/fe-workbench/src/workbench_commands.rs:1948-1952). It closes every tab of every pane, pinned ones included, through kiln_shell::close::confirm as close_many.rs:32-75 does; Cancel keeps everything. On commit, a rootless counterpart of set_root (share its body) runs: ProjectPanel::new(None), GitPanel::new(None), new_quick_open(None), new_git(None), search and nav lose their root (search_panel.rs:350 and nav.rs:252 take a PathBuf today), watch_root_settings(None), resolve_trust(None), toolbar root None, title_bar::name(None), layout_keys.root None. New terminal sessions start in home; running ones stay, as on a re-root. It emits a new RootClosed event (next to RootChanged, workbench.rs:131, re-exported in lib.rs). lsp.rs subscribes to it as it does RootChanged (lsp.rs:991-994), so new_lsp_manager (lsp.rs:1047) shuts that root's servers down. The other RootChanged subscribers (debug_panel, chat_panel, plugin_host, settings_editor, markdown_preview) are out of scope; note in the decision file what each keeps. The Start page shows by itself once no tab is open (sync_start_page, workbench.rs:986-998). The folder stays in Open Recent.
Restore: LayoutKeys::save writes "root" only when there is one (workbench_chrome.rs:1143-1145), and an entry with no root reopens on its store's root (entry_root, kiln/crates/kiln-core/src/layout.rs:405-410), so a closed window would come back on its launch folder. Write "root": "" for a rootless window; reopen.rs:54-66 already reads an empty root as no folder.
Key and menu: secondary-k f, context workbench (VS Code's cmd-k f / ctrl-k f). It is unused in crates/fe-keymap/keymaps/fastedit.toml. Only secondary-k secondary-f (:516, editor) shares the prefix, and secondary-k p/u/w (:976, :1264, :1269) already use this prefix-then-plain-key form. On Linux the vim panel ctrl-k (dock.focus_up, :2627) already shares the prefix with those; leave it as it is. The File menu gets Close Folder after Close Pane, before Close Window (crates/fe-keymap/menus/fastedit.toml:184-189), with keys keymap-menu-close-folder and command-workbench-close_folder.
Overlaps: T1938 and T1942 (lent to helium) on fastedit.toml and commands.ftl; T1917 (lent to neon) on workbench_root.rs; T1939 and T1940 (lent to helium) on lsp.rs. T1937 (helium) adds Open Folder… to the no-folder Project panel; it is related but not a dependency. Rebase on whichever lands first. Do not widen --scope; if a file outside it is needed, stop and say so.

## Helium filing note

- USER-approved feature, lent from the Mac as y144. Overlaps several helium copies: T681 (T1938) and T685
  (T1942) on fastedit.toml/commands.ftl/workbench_root.rs, T682/T683 (T1939/T1940) on lsp.rs, and T662/T664
  on tests/mod.rs. No dependency is set; rebase as they land.
- T680 (T1937) adds Open Folder… to the no-folder Project panel; related, not a dependency.
- The real-app check (Xvfb + xdotool) uses the run-fastedit-linux skill; screenshots in the decision file.
  Kill only processes you started, by PID; never pkill/killall by name (other lanes share this host).
- Ask the lead before rgt finish if anything in the criteria can't be met as written.

## Implementation notes

- Respecced C2 and C7 (revision 2): Close All Editors' prompt, which the
  spec says Close Folder asks as, is "Discard unsaved changes to …?" with
  Cancel / Discard (fe-gpui `gpui-close-discard`), not Save / Don't Save /
  Cancel. The criteria now name that prompt; nothing else changed.
- `set_root` and the new `close_root` share `re_root`; `close_folder`
  (close_many.rs) closes the tabs through the Close All Editors path, pinned
  ones included, then calls `close_root` unless the root changed meanwhile.
- File menu: a chord has no menu accelerator (as Close Pane's cmd-k cmd-w),
  so the item shows no key; `close_folder_key.rs` checks the key through
  `shortcuts()` and that the item shows none.
- What the other `RootChanged` followers keep is in
  `docs/decisions/654yqfxhcd-close-folder.md`.
