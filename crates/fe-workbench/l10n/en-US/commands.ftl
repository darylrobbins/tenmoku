# fe-workbench's command titles: `command-<id>`, with the id's `.` as `-`, as
# kiln-command keys them (i18n design, section 4). The doc comment a
# `#[kiln::command]` method has, or the title a registry registers, stays
# the English fallback; tests/l10n.rs checks the two match.

command-ai-changes_accept_all = Accept All AI Changes
command-ai-changes_next_file = Select Next AI Changed File
command-ai-changes_previous_file = Select Previous AI Changed File
command-ai-changes_reject_all = Reject All AI Changes
command-ai-changes_revert = Revert AI Turn
command-ai-inline_edit = Inline Edit with AI
command-ai-show_changes = Show AI Changes

command-app-check_for_updates = Check for Updates…
command-app-install_shell_command = Install 'tenmoku' Command in PATH
command-app-set_up = Set Up Tenmoku…
command-app-uninstall_shell_command = Uninstall 'tenmoku' Command from PATH

command-buffers-show = Show Buffers

command-chat-ask_about_selection = Chat: Ask About Selection
command-chat-clear_api_key = Chat: Clear API Key
command-chat-delete = Delete Chat
command-chat-new = New Chat
command-chat-open = Chat: Open
command-chat-review_latest_changes = Review Latest Chat Changes
command-chat-set_api_key = Chat: Set API Key…

command-diagnostics-next = Go to Next Problem
command-diagnostics-previous = Go to Previous Problem
command-diagnostics-search_document = Search Diagnostics in File
command-diagnostics-search_workspace = Search Diagnostics in Workspace

command-diff-compare_file = Compare Active File With…
command-diff-compare_saved = Compare Active File with Saved

command-dock-focus_down = Focus Below Dock
command-dock-focus_editor = Focus Active Editor
command-dock-focus_left = Focus Left of Dock
command-dock-focus_right = Focus Right of Dock
command-dock-focus_up = Focus Above Dock
command-dock-grow = Grow Dock
command-dock-move_sidebar_left = Move Sidebar to Left
command-dock-move_sidebar_right = Move Sidebar to Right
command-dock-show_project = Show Project
command-dock-shrink = Shrink Dock
command-dock-toggle = Toggle Dock

command-editor-open_link = Open Link
command-editor-peek_definition = Peek Definition
command-editor-peek_references = Peek References
command-editor-toggle_provenance = Toggle Edit Provenance

command-find-close = Close Find
command-find-next = Find Next
command-find-previous = Find Previous
command-find-replace_all = Replace All
command-find-replace_next = Replace
command-find-show = Find
command-find-show_replace = Replace…
command-find-toggle_case = Match Case
command-find-toggle_in_selection = Find in Selection
command-find-toggle_preserve_case = Toggle Preserve Case
command-find-toggle_regex = Use Regular Expression
command-find-toggle_replace = Toggle Replace
command-find-toggle_whole_word = Match Whole Word

command-git-accept_all_both = Git: Accept All Both
command-git-accept_all_current = Git: Accept All Current
command-git-accept_all_incoming = Git: Accept All Incoming
command-git-accept_both = Git: Accept Both Changes
command-git-accept_current = Git: Accept Current Change
command-git-accept_incoming = Git: Accept Incoming Change
command-git-blame_line = Git: Show Line Blame
command-git-commit = Git: Commit Staged
command-git-commit_amend = Git: Commit Staged (Amend)
command-git-commit_box-commit = Commit from the Message Box
command-git-compare_conflict = Git: Compare Conflict Changes
command-git-create_branch = Git: Create Branch…
command-git-diff_head = Git: Compare with HEAD
command-git-diff_index = Git: Compare with Index
command-git-discard_file = Git: Discard File Changes
command-git-fetch = Git: Fetch
command-git-file_history = Git: File History
command-git-log = Git: Log
command-git-log_all = Git: Log (All Branches)
command-git-next_change = Go to Next Change
command-git-next_conflict = Git: Next Conflict
command-git-open_line_commit = Git: Open Line's Commit Change
command-git-previous_change = Go to Previous Change
command-git-previous_conflict = Git: Previous Conflict
command-git-pull = Git: Pull
command-git-push = Git: Push
command-git-push_force_with_lease = Git: Push (Force with Lease)
command-git-refresh = Refresh Changes
command-git-reset_buffer = Git: Revert Editor to Index
command-git-revert_hunk = Git: Revert Change
command-git-show_changes = Show Changes
command-git-show_files = Go to Tracked File
command-git-stage_file = Git: Stage File
command-git-stage_hunk = Git: Stage Change
command-git-stash = Git: Stash
command-git-stash_apply = Git: Apply Stash…
command-git-stash_include_untracked = Git: Stash (Include Untracked)
command-git-stash_pop = Git: Pop Stash…
command-git-switch_branch = Git: Switch Branch…
command-git-toggle_blame_annotations = Git: Toggle Blame Annotations
command-git-toggle_line_blame = Git: Toggle Line Blame
command-git-unstage_file = Git: Unstage File
command-git-unstage_hunk = Git: Undo Stage Change

command-git_log-compare_with_working_tree = Git: Compare with Working Tree
command-git_log-copy_commit_hash = Git: Copy Commit Hash
command-git_log-copy_subject = Git: Copy Commit Subject
command-git_log-open_changes = Git: Open Changes
command-git_log-open_file_at_revision = Git: Open File at This Revision

command-go_to_line-toggle = Go to Line/Column…

command-hierarchy-toggle_direction = Toggle Incoming and Outgoing Calls

command-history-restore_local = Local History: Restore This Version
command-history-show_local = Local History: Show History

command-image-open_as_image = Open as Image
command-image-open_as_text = Open as Text

command-inline_edit-cancel = Cancel Inline Edit
command-inline_edit-submit = Send Inline Edit

command-keymaps-search = Search Keymaps

command-language-select = Change Language Mode…

command-location-next = Location List: Next Item
command-location-previous = Location List: Previous Item
command-location-show = Location List: Show List
command-location-toggle = Location List: Toggle List

command-lsp-code_actions = Code Actions…
command-lsp-fix_all = Fix All
command-lsp-format_document = Format Document
command-lsp-format_selection = Format Selection
command-lsp-organize_imports = Organize Imports
command-lsp-refactor = Refactor…
command-lsp-rename = Rename Symbol…
command-lsp-restart = Restart Language Server
command-lsp-run_lens = Run Code Lens
command-lsp-show_log = Language Server Log
command-lsp-source_action = Source Action…

command-markdown-show_preview = Markdown: Open Preview
command-markdown-show_preview_to_side = Markdown: Open Preview to the Side
command-markdown-toggle_preview = Markdown: Toggle Preview

command-nav-back = Go Back
command-nav-document_symbols = Go to Symbol in File…
command-nav-find_references = Find All References
command-nav-forward = Go Forward
command-nav-goto_declaration = Go to Declaration
command-nav-goto_definition = Go to Definition
command-nav-goto_implementation = Go to Implementations
command-nav-goto_type_definition = Go to Type Definition
command-nav-show_call_hierarchy = Show Call Hierarchy
command-nav-show_type_hierarchy = Show Type Hierarchy
command-nav-workspace_symbols = Go to Symbol in Workspace…

command-outline-open_selected = Go to Selected Symbol
command-outline-select_first = Select First Symbol
command-outline-select_last = Select Last Symbol
command-outline-select_next = Select Next Symbol
command-outline-select_previous = Select Previous Symbol
command-outline-show = Show Outline

command-pane-close = Close Pane
command-pane-equalize = Equalize Panes
command-pane-exchange = Exchange with Next Pane
command-pane-focus_down = Focus Pane Below
command-pane-focus_group1 = Focus First Editor Group
command-pane-focus_group2 = Focus Second Editor Group
command-pane-focus_group3 = Focus Third Editor Group
command-pane-focus_group4 = Focus Fourth Editor Group
command-pane-focus_left = Focus Left Pane
command-pane-focus_next = Focus Next Pane
command-pane-focus_previous = Focus Previous Pane
command-pane-focus_right = Focus Right Pane
command-pane-focus_up = Focus Pane Above
command-pane-grow_height = Grow Pane Height
command-pane-grow_width = Grow Pane Width
command-pane-move_tab_down = Move Tab Down
command-pane-move_tab_left = Move Tab Left
command-pane-move_tab_right = Move Tab Right
command-pane-move_tab_up = Move Tab Up
command-pane-shrink_height = Shrink Pane Height
command-pane-shrink_width = Shrink Pane Width
command-pane-split_down = Split Down
command-pane-split_right = Split Right
command-pane-toggle_maximize = Toggle Maximized Pane

command-peek-close = Close Peek
command-peek-open = Open Peeked Definition

command-problems-open = Open Problem
command-problems-select_first = Select First Problem
command-problems-select_last = Select Last Problem
command-problems-select_next = Select Next Problem
command-problems-select_previous = Select Previous Problem
command-problems-show = Show Problems

command-project_panel-clear_filter = Clear Filter
command-project_panel-collapse_all = Collapse All Folders
command-project_panel-collapse_or_parent = Collapse or Select Parent
command-project_panel-copy = Copy
command-project_panel-copy_path = Copy Path
command-project_panel-copy_relative_path = Copy Relative Path
command-project_panel-cut = Cut
command-project_panel-delete = Move to Trash
command-project_panel-duplicate = Duplicate
command-project_panel-expand_all = Expand All Folders
command-project_panel-expand_or_first_child = Expand or Select First Child
command-project_panel-filter = Filter by Name…
command-project_panel-new_file = New File…
command-project_panel-new_folder = New Folder…
command-project_panel-open = Open
command-project_panel-paste = Paste
command-project_panel-refresh = Refresh
command-project_panel-rename = Rename…
command-project_panel-reveal_active = Reveal Active File in Project
command-project_panel-select_first = Select First
command-project_panel-select_last = Select Last
command-project_panel-select_next = Select Next
command-project_panel-select_prev = Select Previous
command-project_panel-toggle = Toggle Folder or Open File
command-project_panel-toggle_ignored = Toggle Ignored Files

command-quick_open-toggle = Go to File

command-quickfix-first = Quickfix: First Item
command-quickfix-last = Quickfix: Last Item
command-quickfix-next = Quickfix: Next Item
command-quickfix-previous = Quickfix: Previous Item
command-quickfix-show = Quickfix: Show List
command-quickfix-toggle = Quickfix: Toggle List

command-recent-open_in_new_window = Open Recent in New Window

command-references_panel-open = Open Reference
command-references_panel-select_first = Select First Reference
command-references_panel-select_last = Select Last Reference
command-references_panel-select_next = Select Next Reference
command-references_panel-select_prev = Select Previous Reference

command-search-buffer_lines = Search Lines in Buffer
command-search-clear_results = Clear Search Results
command-search-collapse_all = Collapse All
command-search-collapse_or_parent = Collapse Result or Select Its File
command-search-expand_all = Expand All
command-search-expand_or_first_child = Expand Result or Select Its First Match
command-search-focus_results = Focus Search Results
command-search-open_selected = Open Selected Result
command-search-refresh = Refresh
command-search-replace_all = Replace All in Files
command-search-select_first = Select First Result
command-search-select_last = Select Last Result
command-search-select_next = Select Next Result
command-search-select_previous = Select Previous Result
command-search-show = Find in Files
command-search-show_replace = Replace in Files…
command-search-toggle_case = Toggle Match Case
command-search-toggle_details = Toggle Search Details
command-search-toggle_preserve_case = Toggle Preserve Case
command-search-toggle_regex = Toggle Regex
command-search-toggle_replace = Toggle Replace
command-search-toggle_whole_word = Toggle Whole Word
command-search-view_as_list = View as List
command-search-view_as_tree = View as Tree

command-spell-add_word = Spelling: Add Word to Dictionary
command-spell-ignore_word = Spelling: Ignore Word
command-spell-next = Spelling: Next Misspelling
command-spell-previous = Spelling: Previous Misspelling
command-spell-suggest = Spelling: Suggest Corrections…
command-spell-toggle = Spelling: Toggle Spell Check

command-start_page-open_recent_file = Open Recent File
command-start_page-open_recent_folder = Open Recent Folder

command-tasks-rerun_last = Tasks: Rerun Last Task
command-tasks-run = Tasks: Run Task…
command-tasks-run_build = Tasks: Run Build Task

command-terminal-close = Kill Terminal
command-terminal-find = Terminal: Find
command-terminal-move_to_bottom = Move Terminal to Bottom
command-terminal-move_to_left = Move Terminal to Left
command-terminal-move_to_right = Move Terminal to Right
command-terminal-new = New Terminal
command-terminal-next = Next Terminal
command-terminal-next_prompt = Terminal: Next Prompt
command-terminal-previous = Previous Terminal
command-terminal-previous_prompt = Terminal: Previous Prompt
command-terminal-rename = Rename Terminal
command-terminal-toggle = Toggle Terminal
command-terminal-vi_mode = Terminal: Vi Copy Mode

command-tests-rerun_last = Rerun Last Test
command-tests-run_at_cursor = Run Test at Cursor
command-tests-run_file = Run Tests in File
command-tests-run_row = Run Test at Row: the lens above a test, which runs that test, as

command-text_input-submit = Submit

command-theme-select = Change Color Theme…

command-toast-dismiss_all = Dismiss Notifications

command-todo-list = Search TODO Comments
command-todo-next = Next TODO Comment
command-todo-previous = Previous TODO Comment

command-view-toggle_breadcrumbs = Toggle Breadcrumbs
command-view-toggle_status_bar = Toggle Status Bar

command-vim_line-cancel = Cancel Command Line

command-window-new = New Window

command-workbench-cancel_vim_filter = Vim: Cancel Filter
command-workbench-change_encoding = Change File Encoding
command-workbench-change_line_ending = Change End of Line Sequence…
command-workbench-clear_recent = Clear Recently Opened
command-workbench-close_all_editors = Close All Editors
command-workbench-close_group_editors = Close Editors in Group
command-workbench-close_other_tabs = Close Other Tabs
command-workbench-close_saved = Close Saved Editors
command-workbench-close_tab = Close Tab
command-workbench-configure_snippets = Configure User Snippets
command-workbench-copy_active_file_path = Copy Path of Active File
command-workbench-copy_active_file_relative_path = Copy Relative Path of Active File
command-workbench-harpoon_add = Harpoon: Add File
command-workbench-harpoon_jump1 = Harpoon: Jump to File 1
command-workbench-harpoon_jump2 = Harpoon: Jump to File 2
command-workbench-harpoon_jump3 = Harpoon: Jump to File 3
command-workbench-harpoon_jump4 = Harpoon: Jump to File 4
command-workbench-harpoon_jump5 = Harpoon: Jump to File 5
command-workbench-harpoon_jump6 = Harpoon: Jump to File 6
command-workbench-harpoon_jump7 = Harpoon: Jump to File 7
command-workbench-harpoon_jump8 = Harpoon: Jump to File 8
command-workbench-harpoon_jump9 = Harpoon: Jump to File 9
command-workbench-harpoon_remove_current = Harpoon: Remove File
command-workbench-harpoon_toggle_list = Harpoon: Show List
command-workbench-insert_snippet = Insert Snippet
command-workbench-install_extension = Extensions: Install from File or Folder…
command-workbench-keep_editor = Keep Editor
command-workbench-last_editor_in_group = Last Editor in Group
command-workbench-link_extension_folder = Extensions: Install Folder for Development…
command-workbench-new_file = New File
command-workbench-next_tab = Next Tab
command-workbench-open_editor_at_index1 = Open Editor at Index 1
command-workbench-open_editor_at_index2 = Open Editor at Index 2
command-workbench-open_editor_at_index3 = Open Editor at Index 3
command-workbench-open_editor_at_index4 = Open Editor at Index 4
command-workbench-open_editor_at_index5 = Open Editor at Index 5
command-workbench-open_editor_at_index6 = Open Editor at Index 6
command-workbench-open_editor_at_index7 = Open Editor at Index 7
command-workbench-open_editor_at_index8 = Open Editor at Index 8
command-workbench-open_editor_at_index9 = Open Editor at Index 9
command-workbench-open_file = Open File…
command-workbench-open_folder = Open Folder…
command-workbench-open_folder_in_new_window = Open Folder in New Window…
command-workbench-open_keyboard_shortcuts = Open Keyboard Shortcuts
command-workbench-open_keyboard_shortcuts_json = Open Keyboard Shortcuts (JSON)
command-workbench-open_path = Open Path
command-workbench-open_recent = Open Recent…
command-workbench-open_settings = Open Settings (JSON)
command-workbench-open_settings_ui = Open Settings (UI)
command-workbench-pin_tab = Pin Tab
command-workbench-previous_tab = Previous Tab
command-workbench-reopen_closed_editor = Reopen Closed Editor
command-workbench-reopen_with_encoding = Reopen Editor with Encoding…
command-workbench-reveal_active_file_in_os = Reveal Active File in File Manager
command-workbench-revoke_plugin_grants = Extensions: Revoke Plugin Grants
command-workbench-save_all = Save All
command-workbench-save_as = Save As…
command-workbench-save_with_encoding = Save with Encoding…
command-workbench-save_without_formatting = Save without Formatting
command-workbench-show_config_files = Open Config File…
command-workbench-show_config_problems = Show Config Problems
command-workbench-show_extensions = Extensions: Show Installed
command-workbench-show_plugin_log = Extensions: Show Plugin Log
command-workbench-tab_switcher_next = Switch Tab
command-workbench-tab_switcher_previous = Switch Tab Backward
command-workbench-toggle_zen_mode = Toggle Zen Mode
command-workbench-trust_folder = Trust This Folder
command-workbench-uninstall_plugin = Extensions: Uninstall Plugin
command-workbench-unpin_tab = Unpin Tab
command-workbench-untrust_folder = Untrust This Folder

## What the commands say

# Spelling (spell_commands.rs).
workbench-spell-no-misspelling-here = No misspelling here
workbench-spell-no-misspellings = No misspellings
workbench-spell-read-only = This editor is read-only
workbench-spell-asking = Finding suggestions…
workbench-spell-no-suggestions = No suggestions
# $word is the misspelled word.
workbench-spell-replace = Replace { $word }…
workbench-spell-word-changed = { $word } changed while the suggestions were open
workbench-spell-word-changed-detail = It wasn't replaced. Try again.
workbench-spell-added = Added { $word } to dictionary
workbench-spell-not-added = Can't add { $word } to dictionary
# $error is the system's reason the dictionary's file wasn't written.
workbench-spell-not-added-detail = It counts until you quit: { $error }

# Tasks (task_commands.rs).
workbench-task-none-run = No task has run yet
workbench-task-reading = Reading tasks…
# $file is the tasks file's path in the project, `.tenmoku/tasks.toml`.
workbench-task-none = No tasks: add { $file }
# $group is the tasks file's group name, as written there: build, test or run.
workbench-task-none-in-group = No { $group } tasks: add { $file }
workbench-task-run-placeholder = Run task…
workbench-task-run-build-placeholder = Run build task…

# Git's ex commands (git_commands.rs).
# $line is the ex line as typed, such as `Git commit --fixup`.
workbench-git-ex-not-supported = Not supported: :{ $line }

# The keymaps picker (keymaps_picker.rs).
workbench-keymaps-placeholder = Search keymaps…
workbench-keymaps-none = No keymaps
workbench-keymaps-no-matches = No matching keymaps

# Plugins (workbench_commands.rs).
workbench-plugins-none-installed = No plugins are installed
