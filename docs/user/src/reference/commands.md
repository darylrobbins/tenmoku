# Commands

> Generated; do not edit. `cargo run -p fastedit --example gen_user_docs` writes this page from the command catalog (`fe_workbench::command_catalog()`) and the built-in keymap.

Every command by ID, the name a keymap, a menu or a plugin runs it by, with its title and its default keys in any context.

## `agent`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `agent.diff_lane_file` | Show Lane Changes to Active File | — | — |
| `agent.show_audit_log` | Show Agent Audit Log | — | — |
| `agent.show_panel` | Show Agents | — | — |

## `ai`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `ai.changes_accept_all` | Accept All AI Changes | — | — |
| `ai.changes_next_file` | Select Next AI Changed File | <kbd>⌥&#93;</kbd> | <kbd>Alt+&#93;</kbd> |
| `ai.changes_previous_file` | Select Previous AI Changed File | <kbd>⌥&#91;</kbd> | <kbd>Alt+&#91;</kbd> |
| `ai.changes_reject_all` | Reject All AI Changes | — | — |
| `ai.changes_revert` | Revert AI Turn | — | — |
| `ai.inline_edit` | Inline Edit with AI | <kbd>⌘I</kbd> | <kbd>Ctrl+I</kbd> |
| `ai.show_changes` | Show AI Changes | — | — |

## `ai_changes`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `ai_changes.accept_file` | AI Changes: Accept Selected File | — | — |
| `ai_changes.apply_preview` | AI Changes: Apply Preview | — | — |
| `ai_changes.cancel_preview` | AI Changes: Cancel Preview | — | — |
| `ai_changes.reject_file` | AI Changes: Reject Selected File | — | — |

## `app`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `app.check_for_updates` | Check for Updates… | — | — |
| `app.hide` | Hide Tenmoku | <kbd>⌘H</kbd> | — |
| `app.hide_others` | Hide Others | <kbd>⌥⌘H</kbd> | — |
| `app.install_shell_command` | Install 'tenmoku' Command in PATH | — | — |
| `app.quit` | Quit | <kbd>⌘Q</kbd> | <kbd>Ctrl+Q</kbd> |
| `app.set_up` | Set Up Tenmoku… | — | — |
| `app.show_all` | Show All | — | — |
| `app.show_whats_new` | What's New | — | — |
| `app.uninstall_shell_command` | Uninstall 'tenmoku' Command from PATH | — | — |

## `buffers`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `buffers.show` | Show Buffers | — | — |

## `chat`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `chat.ask_about_selection` | Chat: Ask About Selection | — | — |
| `chat.clear_api_key` | Chat: Clear API Key | — | — |
| `chat.delete` | Delete Chat | — | — |
| `chat.new` | New Chat | — | — |
| `chat.open` | Chat: Open | <kbd>⌃⌘I</kbd> | <kbd>Ctrl+Alt+I</kbd> |
| `chat.retry` | Retry | — | — |
| `chat.review_latest_changes` | Review Latest Chat Changes | — | — |
| `chat.send` | Send Message | <kbd>↩</kbd> | <kbd>Enter</kbd> |
| `chat.set_api_key` | Chat: Set API Key… | — | — |
| `chat.stop` | Stop Reply | <kbd>⎋</kbd> | <kbd>Esc</kbd> |

## `command_palette`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `command_palette.toggle` | Show All Commands | <kbd>⇧⌘P</kbd> | <kbd>Ctrl+Shift+P</kbd> |

## `commit_diff`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `commit_diff.collapse_all` | Commit Diff: Collapse All | — | — |
| `commit_diff.expand_all` | Commit Diff: Expand All | — | — |
| `commit_diff.expand_hidden` | Commit Diff: Show Hidden Lines | — | — |
| `commit_diff.next_change` | Commit Diff: Next Change | — | — |
| `commit_diff.previous_change` | Commit Diff: Previous Change | — | — |
| `commit_diff.toggle_section` | Commit Diff: Toggle Section | — | — |

## `completion`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `completion.accept` | Accept Completion | <kbd>↩</kbd>, <kbd>⇥</kbd> | <kbd>Enter</kbd>, <kbd>Tab</kbd> |
| `completion.dismiss` | Dismiss Completion | <kbd>⎋</kbd> | <kbd>Esc</kbd> |
| `completion.select_next` | Select Next Completion | <kbd>↓</kbd> | <kbd>Down</kbd> |
| `completion.select_prev` | Select Previous Completion | <kbd>↑</kbd> | <kbd>Up</kbd> |

## `debug`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `debug.continue` | Debug: Continue | — | — |
| `debug.evaluate` | Debug: Evaluate Selection | — | — |
| `debug.frame_down` | Debug: Select Callee Frame | — | — |
| `debug.frame_up` | Debug: Select Caller Frame | — | — |
| `debug.hover` | Debug: Show Value at Cursor | — | — |
| `debug.pause` | Debug: Pause | <kbd>F6</kbd> | <kbd>F6</kbd> |
| `debug.remove_all_breakpoints` | Debug: Remove All Breakpoints | — | — |
| `debug.rerun_last` | Debug: Rerun Last Session | — | — |
| `debug.restart` | Debug: Restart | <kbd>⇧⌘F5</kbd> | <kbd>Ctrl+Shift+F5</kbd> |
| `debug.run_to_cursor` | Debug: Run to Cursor | <kbd>⌘F10</kbd> | <kbd>Ctrl+F10</kbd> |
| `debug.show_console` | Debug: Toggle Debug Console | — | — |
| `debug.show_log` | Debug: Show Adapter Log | — | — |
| `debug.show_panels` | Debug: Toggle Debug Tool Window | — | — |
| `debug.show_session` | Debug: Show Session… | — | — |
| `debug.start` | Debug: Start Debugging | <kbd>F5</kbd> | <kbd>F5</kbd> |
| `debug.start_with_args` | Debug: Start with Arguments… | — | — |
| `debug.step_into` | Debug: Step Into | <kbd>F11</kbd> | <kbd>F11</kbd> |
| `debug.step_out` | Debug: Step Out | <kbd>⇧F11</kbd> | <kbd>Shift+F11</kbd> |
| `debug.step_over` | Debug: Step Over | <kbd>F10</kbd> | <kbd>F10</kbd> |
| `debug.stop` | Debug: Stop | <kbd>⇧F5</kbd> | <kbd>Shift+F5</kbd> |
| `debug.test_at_cursor` | Debug: Debug Test at Cursor | — | — |
| `debug.toggle_breakpoint` | Debug: Toggle Breakpoint | <kbd>F9</kbd> | <kbd>F9</kbd> |
| `debug.toggle_breakpoint_enabled` | Debug: Enable or Disable Breakpoint | — | — |

## `diagnostics`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `diagnostics.next` | Go to Next Problem | <kbd>&#93; D</kbd>, <kbd>F8</kbd> | <kbd>&#93; D</kbd>, <kbd>F8</kbd> |
| `diagnostics.previous` | Go to Previous Problem | <kbd>&#91; D</kbd>, <kbd>⇧F8</kbd> | <kbd>&#91; D</kbd>, <kbd>Shift+F8</kbd> |
| `diagnostics.search_document` | Search Diagnostics in File | — | — |
| `diagnostics.search_workspace` | Search Diagnostics in Workspace | — | — |

## `diff`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `diff.compare_file` | Compare Active File With… | — | — |
| `diff.compare_saved` | Compare Active File with Saved | <kbd>⌘K D</kbd> | <kbd>Ctrl+K D</kbd> |
| `diff.next_change` | Go to Next Difference | <kbd>F7</kbd>, <kbd>⌥F5</kbd> | <kbd>F7</kbd>, <kbd>Alt+F5</kbd> |
| `diff.previous_change` | Go to Previous Difference | <kbd>⇧F7</kbd>, <kbd>⌥⇧F5</kbd> | <kbd>Shift+F7</kbd>, <kbd>Alt+Shift+F5</kbd> |
| `diff.toggle_folding` | Toggle Unchanged Regions | — | — |
| `diff.toggle_inline` | Toggle Inline Diff | — | — |

## `dock`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `dock.focus_down` | Focus Below Dock | <kbd>⌃W J</kbd>, <kbd>⌃W ⌃J</kbd>, <kbd>⌃J</kbd> | <kbd>Ctrl+W J</kbd>, <kbd>Ctrl+W Ctrl+J</kbd>, <kbd>Ctrl+J</kbd> |
| `dock.focus_editor` | Focus Active Editor | <kbd>⌃W P</kbd>, <kbd>⌃W W</kbd>, <kbd>⎋</kbd> | <kbd>Ctrl+W P</kbd>, <kbd>Ctrl+W W</kbd>, <kbd>Esc</kbd> |
| `dock.focus_left` | Focus Left of Dock | <kbd>⌃W H</kbd>, <kbd>⌃W ⌃H</kbd>, <kbd>⌃H</kbd> | <kbd>Ctrl+W H</kbd>, <kbd>Ctrl+W Ctrl+H</kbd>, <kbd>Ctrl+H</kbd> |
| `dock.focus_next_panel_group` | Focus Next Panel Group | — | — |
| `dock.focus_right` | Focus Right of Dock | <kbd>⌃W L</kbd>, <kbd>⌃W ⌃L</kbd>, <kbd>⌃L</kbd> | <kbd>Ctrl+W L</kbd>, <kbd>Ctrl+W Ctrl+L</kbd>, <kbd>Ctrl+L</kbd> |
| `dock.focus_up` | Focus Above Dock | <kbd>⌃W K</kbd>, <kbd>⌃W ⌃K</kbd>, <kbd>⌃K</kbd> | <kbd>Ctrl+W K</kbd>, <kbd>Ctrl+W Ctrl+K</kbd>, <kbd>Ctrl+K</kbd> |
| `dock.grow` | Grow Dock | — | — |
| `dock.join_panel_groups` | Join Panel Groups | — | — |
| `dock.move_panel_to_next_group` | Move Tool Window to Next Group | — | — |
| `dock.move_panel_to_previous_group` | Move Tool Window to Previous Group | — | — |
| `dock.move_sidebar_left` | Move Sidebar to Left | — | — |
| `dock.move_sidebar_right` | Move Sidebar to Right | — | — |
| `dock.move_tool_window_to_bottom_panel` | Move Tool Window to Bottom Panel | — | — |
| `dock.move_tool_window_to_sidebar` | Move Tool Window to Sidebar | — | — |
| `dock.show_project` | Show Project | <kbd>⇧⌘E</kbd> | <kbd>Ctrl+Shift+E</kbd> |
| `dock.shrink` | Shrink Dock | — | — |
| `dock.split_panel_right` | Split Panel Right | — | — |
| `dock.toggle` | Toggle Dock | <kbd>⌘B</kbd> | <kbd>Ctrl+B</kbd> |

## `editor`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `editor.add_caret_at_next_occurrence` | Add Caret at Next Occurrence | <kbd>G B</kbd>, <kbd>⌘D</kbd> | <kbd>G B</kbd> |
| `editor.add_carets_at_all_occurrences` | Add Carets at All Occurrences | <kbd>⇧⌘L</kbd> | — |
| `editor.add_cursor_above` | Add Cursor Above | <kbd>⌥⌘↑</kbd> | <kbd>Alt+Shift+Up</kbd> |
| `editor.add_cursor_below` | Add Cursor Below | <kbd>⌥⌘↓</kbd> | <kbd>Alt+Shift+Down</kbd> |
| `editor.announce_position` | Editor: Announce Position | — | — |
| `editor.apply_inlay_hint` | Apply Inlay Hint | — | — |
| `editor.backspace` | Delete Backward | <kbd>⌫</kbd>, <kbd>⇧⌫</kbd> | <kbd>Backspace</kbd>, <kbd>Shift+Backspace</kbd> |
| `editor.column_select_down` | Column Select Down | <kbd>⌥⇧⌘↓</kbd> | — |
| `editor.column_select_left` | Column Select Left | <kbd>⌥⇧⌘←</kbd> | — |
| `editor.column_select_right` | Column Select Right | <kbd>⌥⇧⌘→</kbd> | — |
| `editor.column_select_up` | Column Select Up | <kbd>⌥⇧⌘↑</kbd> | — |
| `editor.convert_indentation_to_spaces` | Convert Indentation to Spaces | — | — |
| `editor.convert_indentation_to_tabs` | Convert Indentation to Tabs | — | — |
| `editor.copy` | Copy | <kbd>⌘C</kbd> | <kbd>Ctrl+C</kbd> |
| `editor.copy_rows_up` | Copy Rows Up | <kbd>⌥⇧↑</kbd> | <kbd>Ctrl+Alt+Shift+Up</kbd> |
| `editor.cursor_redo` | Cursor Redo | — | — |
| `editor.cursor_undo` | Cursor Undo | <kbd>⌘U</kbd> | <kbd>Ctrl+U</kbd> |
| `editor.cut` | Cut | <kbd>⌘X</kbd> | <kbd>Ctrl+X</kbd> |
| `editor.delete` | Delete Forward | <kbd>⌦</kbd> | <kbd>Delete</kbd> |
| `editor.delete_all_left` | Delete All Left | <kbd>⌘⌫</kbd> | — |
| `editor.delete_all_right` | Delete All Right | <kbd>⌃K</kbd> | — |
| `editor.delete_duplicate_lines` | Delete Duplicate Lines | — | — |
| `editor.delete_rows` | Delete Rows | <kbd>⇧⌘K</kbd> | <kbd>Ctrl+Shift+K</kbd> |
| `editor.delete_subword_left` | Delete Subword Left | <kbd>⌃⌥⌫</kbd> | — |
| `editor.delete_subword_right` | Delete Subword Right | <kbd>⌃⌥⌦</kbd> | — |
| `editor.delete_word_left` | Delete Word Left | <kbd>⌥⌫</kbd> | <kbd>Ctrl+Backspace</kbd> |
| `editor.delete_word_right` | Delete Word Right | <kbd>⌥⌦</kbd> | <kbd>Ctrl+Delete</kbd> |
| `editor.detect_indentation` | Detect Indentation from Content | — | — |
| `editor.duplicate_rows` | Duplicate Rows | <kbd>⌥⇧↓</kbd> | <kbd>Ctrl+Alt+Shift+Down</kbd> |
| `editor.expand_selection` | Expand Selection | <kbd>⌃⇧⌘→</kbd> | <kbd>Alt+Shift+Right</kbd> |
| `editor.fold` | Fold | <kbd>⌥⌘&#91;</kbd> | <kbd>Ctrl+{</kbd> |
| `editor.fold_all` | Fold All | <kbd>⌘K ⌘0</kbd> | <kbd>Ctrl+K Ctrl+0</kbd> |
| `editor.fold_level_1` | Fold Level 1 | <kbd>⌘K ⌘1</kbd> | <kbd>Ctrl+K Ctrl+1</kbd> |
| `editor.fold_level_2` | Fold Level 2 | <kbd>⌘K ⌘2</kbd> | <kbd>Ctrl+K Ctrl+2</kbd> |
| `editor.fold_level_3` | Fold Level 3 | <kbd>⌘K ⌘3</kbd> | <kbd>Ctrl+K Ctrl+3</kbd> |
| `editor.fold_level_4` | Fold Level 4 | <kbd>⌘K ⌘4</kbd> | <kbd>Ctrl+K Ctrl+4</kbd> |
| `editor.fold_level_5` | Fold Level 5 | <kbd>⌘K ⌘5</kbd> | <kbd>Ctrl+K Ctrl+5</kbd> |
| `editor.fold_level_6` | Fold Level 6 | <kbd>⌘K ⌘6</kbd> | <kbd>Ctrl+K Ctrl+6</kbd> |
| `editor.fold_level_7` | Fold Level 7 | <kbd>⌘K ⌘7</kbd> | <kbd>Ctrl+K Ctrl+7</kbd> |
| `editor.fold_recursively` | Fold Recursively | <kbd>⌘K ⌘&#91;</kbd> | <kbd>Ctrl+K Ctrl+&#91;</kbd> |
| `editor.go_to_bracket` | Go to Bracket | <kbd>⇧⌘&#92;</kbd> | <kbd>Ctrl+Shift+&#92;</kbd> |
| `editor.go_to_last_edit_location` | Go to Last Edit Location | <kbd>⌘K ⌘Q</kbd> | <kbd>Ctrl+K Ctrl+Q</kbd> |
| `editor.indent` | Indent | <kbd>⌘&#93;</kbd> | <kbd>Ctrl+&#93;</kbd> |
| `editor.insert` | Insert Text | — | — |
| `editor.insert_line_above` | Insert Line Above | <kbd>⇧⌘↩</kbd> | <kbd>Ctrl+Shift+Enter</kbd> |
| `editor.insert_line_below` | Insert Line Below | <kbd>⌘↩</kbd> | <kbd>Ctrl+Enter</kbd> |
| `editor.join_lines` | Join Lines | <kbd>⌃J</kbd> | — |
| `editor.move` | Move | — | — |
| `editor.move_document_end` | Move to Document End | <kbd>⌘↓</kbd> | <kbd>Ctrl+End</kbd> |
| `editor.move_document_start` | Move to Document Start | <kbd>⌘↑</kbd> | <kbd>Ctrl+Home</kbd> |
| `editor.move_down` | Move Down | <kbd>↓</kbd> | <kbd>Down</kbd> |
| `editor.move_last_selection_to_next_find_match` | Move Last Selection to Next Find Match | <kbd>⌘K ⌘D</kbd> | <kbd>Ctrl+K Ctrl+D</kbd> |
| `editor.move_left` | Move Left | <kbd>←</kbd> | <kbd>Left</kbd> |
| `editor.move_line_end` | Move to Line End | <kbd>⌘→</kbd>, <kbd>↘</kbd> | <kbd>End</kbd> |
| `editor.move_line_start` | Move to Line Start | <kbd>⌘←</kbd>, <kbd>↖</kbd> | <kbd>Home</kbd> |
| `editor.move_right` | Move Right | <kbd>→</kbd> | <kbd>Right</kbd> |
| `editor.move_rows_down` | Move Rows Down | <kbd>⌥↓</kbd> | <kbd>Alt+Down</kbd> |
| `editor.move_rows_up` | Move Rows Up | <kbd>⌥↑</kbd> | <kbd>Alt+Up</kbd> |
| `editor.move_subword_left` | Move Subword Left | <kbd>⌃⌥←</kbd> | — |
| `editor.move_subword_right` | Move Subword Right | <kbd>⌃⌥→</kbd> | — |
| `editor.move_up` | Move Up | <kbd>↑</kbd> | <kbd>Up</kbd> |
| `editor.move_word_left` | Move Word Left | <kbd>⌥←</kbd> | <kbd>Ctrl+Left</kbd> |
| `editor.move_word_right` | Move Word Right | <kbd>⌥→</kbd> | <kbd>Ctrl+Right</kbd> |
| `editor.newline` | Insert Newline | <kbd>⇧↩</kbd>, <kbd>↩</kbd> | <kbd>Shift+Enter</kbd>, <kbd>Enter</kbd> |
| `editor.open_link` | Open Link | — | — |
| `editor.outdent` | Outdent | <kbd>⌘&#91;</kbd>, <kbd>⇧⇥</kbd> | <kbd>Ctrl+&#91;</kbd>, <kbd>Shift+Tab</kbd> |
| `editor.page_down` | Page Down | <kbd>⇟</kbd> | <kbd>PageDown</kbd> |
| `editor.page_up` | Page Up | <kbd>⇞</kbd> | <kbd>PageUp</kbd> |
| `editor.paste` | Paste | <kbd>⌘V</kbd> | <kbd>Ctrl+V</kbd> |
| `editor.peek_definition` | Peek Definition | <kbd>⌥F12</kbd> | <kbd>Alt+F12</kbd> |
| `editor.peek_references` | Peek References | <kbd>⇧F12</kbd> | <kbd>Shift+F12</kbd> |
| `editor.redo` | Redo | <kbd>⇧⌘Z</kbd> | <kbd>Ctrl+Shift+Z</kbd>, <kbd>Ctrl+Y</kbd> |
| `editor.save` | Save | <kbd>⌘S</kbd> | <kbd>Ctrl+S</kbd> |
| `editor.select_all` | Select All | <kbd>⌘A</kbd> | <kbd>Ctrl+A</kbd> |
| `editor.select_all_occurrences` | Select All Occurrences | <kbd>⇧⌘L</kbd> | <kbd>Ctrl+Shift+L</kbd> |
| `editor.select_document_end` | Select to Document End | <kbd>⇧⌘↓</kbd> | <kbd>Ctrl+Shift+End</kbd> |
| `editor.select_document_start` | Select to Document Start | <kbd>⇧⌘↑</kbd> | <kbd>Ctrl+Shift+Home</kbd> |
| `editor.select_down` | Select Down | <kbd>⇧↓</kbd> | <kbd>Shift+Down</kbd> |
| `editor.select_left` | Select Left | <kbd>⇧←</kbd> | <kbd>Shift+Left</kbd> |
| `editor.select_line_end` | Select to Line End | <kbd>⇧⌘→</kbd>, <kbd>⇧↘</kbd> | <kbd>Shift+End</kbd> |
| `editor.select_line_start` | Select to Line Start | <kbd>⇧⌘←</kbd>, <kbd>⇧↖</kbd> | <kbd>Shift+Home</kbd> |
| `editor.select_next_occurrence` | Select Next Occurrence | <kbd>⌘D</kbd> | <kbd>Ctrl+D</kbd> |
| `editor.select_page_down` | Select Page Down | <kbd>⇧⇟</kbd> | <kbd>Shift+PageDown</kbd> |
| `editor.select_page_up` | Select Page Up | <kbd>⇧⇞</kbd> | <kbd>Shift+PageUp</kbd> |
| `editor.select_right` | Select Right | <kbd>⇧→</kbd> | <kbd>Shift+Right</kbd> |
| `editor.select_subword_left` | Select Subword Left | <kbd>⌃⌥⇧←</kbd> | — |
| `editor.select_subword_right` | Select Subword Right | <kbd>⌃⌥⇧→</kbd> | — |
| `editor.select_up` | Select Up | <kbd>⇧↑</kbd> | <kbd>Shift+Up</kbd> |
| `editor.select_word_left` | Select Word Left | <kbd>⌥⇧←</kbd> | <kbd>Ctrl+Shift+Left</kbd> |
| `editor.select_word_right` | Select Word Right | <kbd>⌥⇧→</kbd> | <kbd>Ctrl+Shift+Right</kbd> |
| `editor.set_end_of_line_crlf` | Change End of Line Sequence to CRLF | — | — |
| `editor.set_end_of_line_lf` | Change End of Line Sequence to LF | — | — |
| `editor.show_hover` | Show Hover | <kbd>⌘K ⌘I</kbd>, <kbd>⇧K</kbd> | <kbd>Ctrl+K Ctrl+I</kbd>, <kbd>Shift+K</kbd> |
| `editor.show_signature_help` | Show Signature Help | <kbd>⇧⌘Space</kbd> | <kbd>Ctrl+Shift+Space</kbd> |
| `editor.shrink_selection` | Shrink Selection | <kbd>⌃⇧⌘←</kbd> | <kbd>Alt+Shift+Left</kbd> |
| `editor.single_selection` | Single Selection | <kbd>⎋</kbd> | <kbd>Esc</kbd> |
| `editor.sort_lines_ascending` | Sort Lines Ascending | — | — |
| `editor.sort_lines_descending` | Sort Lines Descending | — | — |
| `editor.split_selection_into_lines` | Split Selection into Lines | <kbd>⌥⇧I</kbd> | <kbd>Alt+Shift+I</kbd> |
| `editor.tab` | Insert Tab | <kbd>⇥</kbd> | <kbd>Tab</kbd> |
| `editor.toggle_block_comment` | Toggle Block Comment | <kbd>⌥⇧A</kbd> | <kbd>Alt+Shift+A</kbd> |
| `editor.toggle_comment` | Toggle Line Comment | <kbd>⌘/</kbd> | <kbd>Ctrl+/</kbd> |
| `editor.toggle_provenance` | Toggle Edit Provenance | — | — |
| `editor.toggle_soft_wrap` | Toggle Soft Wrap | <kbd>⌥Z</kbd> | <kbd>Alt+Z</kbd> |
| `editor.transform_lowercase` | Transform to Lowercase | — | — |
| `editor.transform_titlecase` | Transform to Title Case | — | — |
| `editor.transform_uppercase` | Transform to Uppercase | — | — |
| `editor.transpose_letters` | Transpose Letters | <kbd>⌃T</kbd> | — |
| `editor.trigger_completion` | Trigger Completion | <kbd>⌃Space</kbd> | <kbd>Ctrl+Space</kbd> |
| `editor.trim_trailing_whitespace` | Trim Trailing Whitespace | <kbd>⌘K ⌘X</kbd> | <kbd>Ctrl+K Ctrl+X</kbd> |
| `editor.undo` | Undo | <kbd>⌘Z</kbd> | <kbd>Ctrl+Z</kbd> |
| `editor.unfold` | Unfold | <kbd>⌥⌘&#93;</kbd> | <kbd>Ctrl+}</kbd> |
| `editor.unfold_all` | Unfold All | <kbd>⌘K ⌘J</kbd> | <kbd>Ctrl+K Ctrl+J</kbd> |
| `editor.unfold_recursively` | Unfold Recursively | <kbd>⌘K ⌘&#93;</kbd> | <kbd>Ctrl+K Ctrl+&#93;</kbd> |

## `find`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `find.close` | Close Find | <kbd>⎋</kbd> | <kbd>Esc</kbd> |
| `find.next` | Find Next | <kbd>⌘G</kbd>, <kbd>↩</kbd> | <kbd>F3</kbd>, <kbd>Enter</kbd> |
| `find.previous` | Find Previous | <kbd>⇧⌘G</kbd>, <kbd>⇧↩</kbd> | <kbd>Shift+F3</kbd>, <kbd>Shift+Enter</kbd> |
| `find.replace_all` | Replace All | <kbd>⌥⌘↩</kbd> | <kbd>Ctrl+Alt+Enter</kbd> |
| `find.replace_next` | Replace | <kbd>⌘&#33;</kbd>, <kbd>↩</kbd> | <kbd>Ctrl+&#33;</kbd>, <kbd>Enter</kbd> |
| `find.show` | Find | <kbd>⌘F</kbd> | <kbd>Ctrl+F</kbd> |
| `find.show_replace` | Replace… | <kbd>⌥⌘F</kbd> | <kbd>Ctrl+Alt+F</kbd> |
| `find.toggle_case` | Match Case | <kbd>⌥⌘C</kbd> | <kbd>Alt+C</kbd> |
| `find.toggle_in_selection` | Find in Selection | <kbd>⌥⌘L</kbd> | <kbd>Alt+L</kbd> |
| `find.toggle_preserve_case` | Toggle Preserve Case | <kbd>⌥⌘P</kbd> | <kbd>Alt+P</kbd> |
| `find.toggle_regex` | Use Regular Expression | <kbd>⌥⌘R</kbd> | <kbd>Alt+R</kbd> |
| `find.toggle_replace` | Toggle Replace | — | — |
| `find.toggle_whole_word` | Match Whole Word | <kbd>⌥⌘W</kbd> | <kbd>Alt+W</kbd> |

## `ghost_text`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `ghost_text.accept` | Accept Suggestion | <kbd>⇥</kbd> | <kbd>Tab</kbd> |
| `ghost_text.accept_line` | Accept Next Line of Suggestion | — | — |
| `ghost_text.accept_word` | Accept Next Word of Suggestion | <kbd>⌘→</kbd> | <kbd>Ctrl+Right</kbd> |
| `ghost_text.dismiss` | Dismiss Suggestion | — | — |

## `git`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `git.accept_all_both` | Git: Accept All Both | — | — |
| `git.accept_all_current` | Git: Accept All Current | — | — |
| `git.accept_all_incoming` | Git: Accept All Incoming | — | — |
| `git.accept_both` | Git: Accept Both Changes | — | — |
| `git.accept_current` | Git: Accept Current Change | — | — |
| `git.accept_incoming` | Git: Accept Incoming Change | — | — |
| `git.blame_line` | Git: Show Line Blame | — | — |
| `git.branches_menu` | Git: Branches… | — | — |
| `git.commit` | Git: Commit Staged | — | — |
| `git.commit_amend` | Git: Commit Staged (Amend) | — | — |
| `git.commit_box.commit` | Commit from the Message Box | <kbd>⌘↩</kbd> | <kbd>Ctrl+Enter</kbd> |
| `git.compare_conflict` | Git: Compare Conflict Changes | — | — |
| `git.create_branch` | Git: Create Branch… | — | — |
| `git.diff_head` | Git: Compare with HEAD | — | — |
| `git.diff_index` | Git: Compare with Index | — | — |
| `git.discard_file` | Git: Discard File Changes | — | — |
| `git.fetch` | Git: Fetch | — | — |
| `git.file_history` | Git: File History | — | — |
| `git.log` | Git: Log | — | — |
| `git.log_all` | Git: Log (All Branches) | — | — |
| `git.next_change` | Go to Next Change | <kbd>⌥F3</kbd> | <kbd>Alt+F3</kbd> |
| `git.next_conflict` | Git: Next Conflict | — | — |
| `git.open_line_commit` | Git: Open Line's Commit Change | — | — |
| `git.previous_change` | Go to Previous Change | <kbd>⌥⇧F3</kbd> | <kbd>Alt+Shift+F3</kbd> |
| `git.previous_conflict` | Git: Previous Conflict | — | — |
| `git.pull` | Git: Pull | — | — |
| `git.push` | Git: Push | — | — |
| `git.push_force_with_lease` | Git: Push (Force with Lease) | — | — |
| `git.refresh` | Refresh Changes | — | — |
| `git.reset_buffer` | Git: Revert Editor to Index | — | — |
| `git.revert_hunk` | Git: Revert Change | <kbd>⌥⌘Z</kbd> | <kbd>Ctrl+Alt+Z</kbd> |
| `git.show_changes` | Show Changes | <kbd>⌃⇧G</kbd> | <kbd>Ctrl+Shift+G</kbd> |
| `git.show_files` | Go to Tracked File | — | — |
| `git.stage_file` | Git: Stage File | — | — |
| `git.stage_hunk` | Git: Stage Change | <kbd>⌘Y</kbd> | <kbd>Ctrl+Alt+Y</kbd> |
| `git.stash` | Git: Stash | — | — |
| `git.stash_apply` | Git: Apply Stash… | — | — |
| `git.stash_include_untracked` | Git: Stash (Include Untracked) | — | — |
| `git.stash_pop` | Git: Pop Stash… | — | — |
| `git.switch_branch` | Git: Switch Branch… | — | — |
| `git.toggle_blame_annotations` | Git: Toggle Blame Annotations | <kbd>⌥⌘G B</kbd> | <kbd>Ctrl+Alt+G B</kbd> |
| `git.toggle_line_blame` | Git: Toggle Line Blame | — | — |
| `git.unstage_file` | Git: Unstage File | — | — |
| `git.unstage_hunk` | Git: Undo Stage Change | — | — |
| `git.update_project` | Git: Update Project | — | — |

## `git_log`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `git_log.compare_with_working_tree` | Git: Compare with Working Tree | — | — |
| `git_log.copy_commit_hash` | Git: Copy Commit Hash | — | — |
| `git_log.copy_subject` | Git: Copy Commit Subject | — | — |
| `git_log.focus_filter` | Git: Focus Log Filter | — | — |
| `git_log.open_changes` | Git: Open Changes | — | — |
| `git_log.open_commit_link` | Git: Open Commit on Remote | — | — |
| `git_log.open_file_at_revision` | Git: Open File at This Revision | — | — |

## `git_panel`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `git_panel.discard_row` | Git: Discard Selected Row | — | — |
| `git_panel.stage_row` | Git: Stage Selected Row | — | — |
| `git_panel.unstage_row` | Git: Unstage Selected Row | — | — |

## `go_to_line`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `go_to_line.toggle` | Go to Line/Column… | <kbd>⌃G</kbd> | <kbd>Ctrl+G</kbd> |

## `hex`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `hex.copy` | Copy Bytes | <kbd>⌘C</kbd> | <kbd>Ctrl+C</kbd> |
| `hex.copy_as_hex` | Copy as Hex | — | — |
| `hex.copy_as_text` | Copy as Text | — | — |
| `hex.find` | Find Bytes | <kbd>⌘F</kbd> | <kbd>Ctrl+F</kbd> |
| `hex.find_next` | Find Next Bytes | <kbd>⌘G</kbd> | <kbd>F3</kbd> |
| `hex.find_previous` | Find Previous Bytes | <kbd>⇧⌘G</kbd> | <kbd>Shift+F3</kbd> |
| `hex.go_to_offset` | Go to Offset… | <kbd>⌃G</kbd> | <kbd>Ctrl+G</kbd> |
| `hex.open_as_hex` | Open as Hex | — | — |
| `hex.open_as_text` | Open as Text | — | — |
| `hex.toggle_endian` | Toggle Inspector Byte Order | — | — |
| `hex.toggle_inspector` | Toggle Byte Inspector | — | — |

## `hierarchy`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `hierarchy.toggle_direction` | Toggle Incoming and Outgoing Calls | — | — |

## `history`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `history.restore_local` | Local History: Restore This Version | — | — |
| `history.show_local` | Local History: Show History | — | — |

## `image`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `image.open_as_image` | Open as Image | — | — |
| `image.open_as_text` | Open as Text | — | — |
| `image.zoom_actual` | Actual Size | <kbd>⌘1</kbd> | <kbd>Ctrl+1</kbd> |
| `image.zoom_fit` | Fit to Window | <kbd>⌘0</kbd> | <kbd>Ctrl+0</kbd> |
| `image.zoom_in` | Zoom In | <kbd>⌘=</kbd>, <kbd>⌘+</kbd> | <kbd>Ctrl+=</kbd>, <kbd>Ctrl++</kbd> |
| `image.zoom_out` | Zoom Out | <kbd>⌘-</kbd> | <kbd>Ctrl+-</kbd> |

## `inline_edit`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `inline_edit.cancel` | Cancel Inline Edit | <kbd>⎋</kbd> | <kbd>Esc</kbd> |
| `inline_edit.submit` | Send Inline Edit | <kbd>↩</kbd> | <kbd>Enter</kbd> |

## `keymaps`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `keymaps.search` | Search Keymaps | — | — |

## `language`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `language.select` | Change Language Mode… | <kbd>⌘K M</kbd> | <kbd>Ctrl+K M</kbd> |

## `location`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `location.next` | Location List: Next Item | — | — |
| `location.previous` | Location List: Previous Item | — | — |
| `location.show` | Location List: Show List | — | — |
| `location.toggle` | Location List: Toggle List | — | — |

## `lsp`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `lsp.code_actions` | Code Actions… | <kbd>⌘.</kbd>, <kbd>G R A</kbd> | <kbd>Ctrl+.</kbd>, <kbd>G R A</kbd> |
| `lsp.fix_all` | Fix All | — | — |
| `lsp.format_document` | Format Document | <kbd>⌥⇧F</kbd> | <kbd>Ctrl+Shift+I</kbd> |
| `lsp.format_selection` | Format Selection | <kbd>⌘K ⌘F</kbd> | <kbd>Ctrl+K Ctrl+F</kbd> |
| `lsp.organize_imports` | Organize Imports | <kbd>⌥⇧O</kbd> | <kbd>Alt+Shift+O</kbd> |
| `lsp.refactor` | Refactor… | <kbd>⌃⇧R</kbd> | <kbd>Ctrl+Shift+R</kbd> |
| `lsp.rename` | Rename Symbol… | <kbd>F2</kbd>, <kbd>G R N</kbd> | <kbd>F2</kbd>, <kbd>G R N</kbd> |
| `lsp.restart` | Restart Language Server | — | — |
| `lsp.run_lens` | Code Lens: Run… | — | — |
| `lsp.show_log` | Language Server Log | — | — |
| `lsp.source_action` | Source Action… | — | — |

## `markdown`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `markdown.show_preview` | Markdown: Open Preview | <kbd>⇧⌘V</kbd> | <kbd>Ctrl+Shift+V</kbd> |
| `markdown.show_preview_to_side` | Markdown: Open Preview to the Side | <kbd>⌘K V</kbd> | <kbd>Ctrl+K V</kbd> |
| `markdown.toggle_preview` | Markdown: Toggle Preview | — | — |

## `memory`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `memory.show` | Show Memory Usage | — | — |

## `menu_bar`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `menu_bar.open` | Open Menu Bar | — | <kbd>F10</kbd> |

## `nav`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `nav.back` | Go Back | <kbd>⌃-</kbd> | <kbd>Ctrl+Alt+-</kbd> |
| `nav.document_symbols` | Go to Symbol in File… | <kbd>⇧⌘O</kbd> | <kbd>Ctrl+Shift+O</kbd> |
| `nav.find_references` | Find All References | <kbd>G R R</kbd>, <kbd>⌥⇧F12</kbd> | <kbd>G R R</kbd>, <kbd>Alt+Shift+F12</kbd> |
| `nav.forward` | Go Forward | <kbd>⌃⇧-</kbd> | <kbd>Ctrl+&#95;</kbd> |
| `nav.goto_declaration` | Go to Declaration | <kbd>G ⇧D</kbd> | <kbd>G Shift+D</kbd> |
| `nav.goto_definition` | Go to Definition | <kbd>G D</kbd>, <kbd>⌃&#93;</kbd>, <kbd>⌘Click</kbd>, <kbd>F12</kbd> | <kbd>G D</kbd>, <kbd>Ctrl+&#93;</kbd>, <kbd>Ctrl+Click</kbd>, <kbd>F12</kbd> |
| `nav.goto_implementation` | Go to Implementations | <kbd>G R I</kbd>, <kbd>⌘F12</kbd> | <kbd>G R I</kbd>, <kbd>Ctrl+F12</kbd> |
| `nav.goto_type_definition` | Go to Type Definition | <kbd>G R T</kbd> | <kbd>G R T</kbd> |
| `nav.show_call_hierarchy` | Show Call Hierarchy | <kbd>⌥⇧H</kbd> | <kbd>Alt+Shift+H</kbd> |
| `nav.show_type_hierarchy` | Show Type Hierarchy | — | — |
| `nav.workspace_symbols` | Go to Symbol in Workspace… | <kbd>⌘T</kbd> | <kbd>Ctrl+T</kbd> |

## `notifications`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `notifications.show` | Show Notifications | — | — |
| `notifications.toggle_focus` | Toggle Notification Focus Mode | — | — |

## `outline`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `outline.open_selected` | Go to Selected Symbol | <kbd>↩</kbd>, <kbd>O</kbd> | <kbd>Enter</kbd>, <kbd>O</kbd> |
| `outline.select_first` | Select First Symbol | <kbd>G G</kbd> | <kbd>G G</kbd> |
| `outline.select_last` | Select Last Symbol | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> |
| `outline.select_next` | Select Next Symbol | <kbd>↓</kbd>, <kbd>J</kbd> | <kbd>Down</kbd>, <kbd>J</kbd> |
| `outline.select_previous` | Select Previous Symbol | <kbd>↑</kbd>, <kbd>K</kbd> | <kbd>Up</kbd>, <kbd>K</kbd> |
| `outline.show` | Show Outline | <kbd>G ⇧O</kbd>, <kbd>⌥⌘O</kbd> | <kbd>G Shift+O</kbd>, <kbd>Ctrl+Alt+O</kbd> |

## `pane`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `pane.close` | Close Pane | <kbd>⌃W C</kbd>, <kbd>⌃W Q</kbd>, <kbd>⌃W ⌃Q</kbd>, <kbd>⌘K ⌘W</kbd> | <kbd>Ctrl+W C</kbd>, <kbd>Ctrl+W Q</kbd>, <kbd>Ctrl+W Ctrl+Q</kbd>, <kbd>Ctrl+K Ctrl+W</kbd> |
| `pane.equalize` | Equalize Panes | <kbd>⌃W =</kbd> | <kbd>Ctrl+W =</kbd> |
| `pane.exchange` | Exchange with Next Pane | <kbd>⌃W X</kbd>, <kbd>⌃W ⌃X</kbd> | <kbd>Ctrl+W X</kbd>, <kbd>Ctrl+W Ctrl+X</kbd> |
| `pane.focus_down` | Focus Pane Below | <kbd>⌃W J</kbd>, <kbd>⌃W ⌃J</kbd>, <kbd>⌃W ↓</kbd>, <kbd>⌘K ⌘↓</kbd> | <kbd>Ctrl+W J</kbd>, <kbd>Ctrl+W Ctrl+J</kbd>, <kbd>Ctrl+W Down</kbd>, <kbd>Ctrl+K Ctrl+Down</kbd> |
| `pane.focus_group1` | Focus First Editor Group | <kbd>⌘1</kbd> | <kbd>Ctrl+1</kbd> |
| `pane.focus_group2` | Focus Second Editor Group | <kbd>⌘2</kbd> | <kbd>Ctrl+2</kbd> |
| `pane.focus_group3` | Focus Third Editor Group | <kbd>⌘3</kbd> | <kbd>Ctrl+3</kbd> |
| `pane.focus_group4` | Focus Fourth Editor Group | <kbd>⌘4</kbd> | <kbd>Ctrl+4</kbd> |
| `pane.focus_left` | Focus Left Pane | <kbd>⌃W H</kbd>, <kbd>⌃W ⌃H</kbd>, <kbd>⌃W ←</kbd>, <kbd>⌘K ⌘←</kbd> | <kbd>Ctrl+W H</kbd>, <kbd>Ctrl+W Ctrl+H</kbd>, <kbd>Ctrl+W Left</kbd>, <kbd>Ctrl+K Ctrl+Left</kbd> |
| `pane.focus_next` | Focus Next Pane | <kbd>⌃W W</kbd>, <kbd>⌃W ⌃W</kbd> | <kbd>Ctrl+W W</kbd>, <kbd>Ctrl+W Ctrl+W</kbd> |
| `pane.focus_previous` | Focus Previous Pane | <kbd>⌃W ⇧W</kbd> | <kbd>Ctrl+W Shift+W</kbd> |
| `pane.focus_right` | Focus Right Pane | <kbd>⌃W L</kbd>, <kbd>⌃W ⌃L</kbd>, <kbd>⌃W →</kbd>, <kbd>⌘K ⌘→</kbd> | <kbd>Ctrl+W L</kbd>, <kbd>Ctrl+W Ctrl+L</kbd>, <kbd>Ctrl+W Right</kbd>, <kbd>Ctrl+K Ctrl+Right</kbd> |
| `pane.focus_up` | Focus Pane Above | <kbd>⌃W K</kbd>, <kbd>⌃W ⌃K</kbd>, <kbd>⌃W ↑</kbd>, <kbd>⌘K ⌘↑</kbd> | <kbd>Ctrl+W K</kbd>, <kbd>Ctrl+W Ctrl+K</kbd>, <kbd>Ctrl+W Up</kbd>, <kbd>Ctrl+K Ctrl+Up</kbd> |
| `pane.grow_height` | Grow Pane Height | <kbd>⌃W +</kbd> | <kbd>Ctrl+W +</kbd> |
| `pane.grow_width` | Grow Pane Width | <kbd>⌃W &#62;</kbd> | <kbd>Ctrl+W &#62;</kbd> |
| `pane.move_tab_down` | Move Tab Down | <kbd>⌃W ⇧J</kbd>, <kbd>⌃⌘↓</kbd> | <kbd>Ctrl+W Shift+J</kbd>, <kbd>Ctrl+Alt+Down</kbd> |
| `pane.move_tab_left` | Move Tab Left | <kbd>⌃W ⇧H</kbd>, <kbd>⌃⌘←</kbd> | <kbd>Ctrl+W Shift+H</kbd>, <kbd>Ctrl+Alt+Left</kbd> |
| `pane.move_tab_right` | Move Tab Right | <kbd>⌃W ⇧L</kbd>, <kbd>⌃⌘→</kbd> | <kbd>Ctrl+W Shift+L</kbd>, <kbd>Ctrl+Alt+Right</kbd> |
| `pane.move_tab_up` | Move Tab Up | <kbd>⌃W ⇧K</kbd>, <kbd>⌃⌘↑</kbd> | <kbd>Ctrl+W Shift+K</kbd>, <kbd>Ctrl+Alt+Up</kbd> |
| `pane.move_to_new_window` | Move to New Window | — | — |
| `pane.reorder_tab_left` | Reorder Tab Left | — | — |
| `pane.reorder_tab_right` | Reorder Tab Right | — | — |
| `pane.shrink_height` | Shrink Pane Height | <kbd>⌃W -</kbd> | <kbd>Ctrl+W -</kbd> |
| `pane.shrink_width` | Shrink Pane Width | <kbd>⌃W &#60;</kbd> | <kbd>Ctrl+W &#60;</kbd> |
| `pane.split_down` | Split Down | <kbd>⌃W S</kbd>, <kbd>⌃W ⇧S</kbd>, <kbd>⌃W ⌃S</kbd>, <kbd>⌘K ⌘&#92;</kbd> | <kbd>Ctrl+W S</kbd>, <kbd>Ctrl+W Shift+S</kbd>, <kbd>Ctrl+W Ctrl+S</kbd>, <kbd>Ctrl+K Ctrl+&#92;</kbd> |
| `pane.split_right` | Split Right | <kbd>⌃W V</kbd>, <kbd>⌃W ⌃V</kbd>, <kbd>⌘&#92;</kbd> | <kbd>Ctrl+W V</kbd>, <kbd>Ctrl+W Ctrl+V</kbd>, <kbd>Ctrl+&#92;</kbd> |
| `pane.toggle_maximize` | Toggle Maximized Pane | <kbd>⌃W Z</kbd>, <kbd>⌘K ⌘M</kbd> | <kbd>Ctrl+W Z</kbd>, <kbd>Ctrl+K Ctrl+M</kbd> |

## `pdf`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `pdf.open_in_default_app` | Open in Default App | — | — |

## `peek`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `peek.close` | Close Peek | <kbd>⎋</kbd> | <kbd>Esc</kbd> |
| `peek.open` | Open Peeked Definition | <kbd>↩</kbd> | <kbd>Enter</kbd> |

## `picker`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `picker.confirm` | Confirm | <kbd>↩</kbd> | <kbd>Enter</kbd> |
| `picker.dismiss` | Dismiss | <kbd>⎋</kbd> | <kbd>Esc</kbd> |
| `picker.select_next` | Select Next | <kbd>↓</kbd>, <kbd>⌃N</kbd> | <kbd>Down</kbd> |
| `picker.select_prev` | Select Previous | <kbd>↑</kbd>, <kbd>⌃P</kbd> | <kbd>Up</kbd> |

## `problems`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `problems.apply_fix` | Apply Fix | <kbd>⌥↩</kbd> | <kbd>Alt+Enter</kbd> |
| `problems.ask_agent` | Ask an Agent About This Problem | — | — |
| `problems.explain` | Explain This Problem | — | — |
| `problems.open` | Open Problem | <kbd>↩</kbd>, <kbd>O</kbd> | <kbd>Enter</kbd>, <kbd>O</kbd> |
| `problems.select_first` | Select First Problem | <kbd>G G</kbd> | <kbd>G G</kbd> |
| `problems.select_last` | Select Last Problem | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> |
| `problems.select_next` | Select Next Problem | <kbd>↓</kbd>, <kbd>J</kbd> | <kbd>Down</kbd>, <kbd>J</kbd> |
| `problems.select_previous` | Select Previous Problem | <kbd>↑</kbd>, <kbd>K</kbd> | <kbd>Up</kbd>, <kbd>K</kbd> |
| `problems.show` | Show Problems | <kbd>⇧⌘M</kbd> | <kbd>Ctrl+Shift+M</kbd> |

## `project_panel`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `project_panel.clear_filter` | Clear Filter | <kbd>⇧F</kbd> | <kbd>Shift+F</kbd> |
| `project_panel.collapse_all` | Collapse All Folders | <kbd>⇧W</kbd> | <kbd>Shift+W</kbd> |
| `project_panel.collapse_or_parent` | Collapse or Select Parent | <kbd>←</kbd>, <kbd>H</kbd> | <kbd>Left</kbd>, <kbd>H</kbd> |
| `project_panel.copy` | Copy | <kbd>⌘C</kbd>, <kbd>C</kbd> | <kbd>Ctrl+C</kbd>, <kbd>C</kbd> |
| `project_panel.copy_path` | Copy Path | <kbd>G Y</kbd> | <kbd>G Y</kbd> |
| `project_panel.copy_relative_path` | Copy Relative Path | <kbd>⇧Y</kbd> | <kbd>Shift+Y</kbd> |
| `project_panel.cut` | Cut | <kbd>⌘X</kbd>, <kbd>X</kbd> | <kbd>Ctrl+X</kbd>, <kbd>X</kbd> |
| `project_panel.delete` | Move to Trash | <kbd>⌘⌫</kbd>, <kbd>D</kbd>, <kbd>⇧D</kbd> | <kbd>Ctrl+Backspace</kbd>, <kbd>D</kbd>, <kbd>Shift+D</kbd> |
| `project_panel.duplicate` | Duplicate | — | — |
| `project_panel.expand_all` | Expand All Folders | <kbd>⇧E</kbd> | <kbd>Shift+E</kbd> |
| `project_panel.expand_or_first_child` | Expand or Select First Child | <kbd>→</kbd> | <kbd>Right</kbd> |
| `project_panel.filter` | Filter by Name… | <kbd>/</kbd> | <kbd>/</kbd> |
| `project_panel.find_in_folder` | Find in Folder… | — | — |
| `project_panel.new_file` | New File… | <kbd>⌥⌘N</kbd>, <kbd>A</kbd>, <kbd>%</kbd> | <kbd>Ctrl+Alt+N</kbd>, <kbd>A</kbd>, <kbd>%</kbd> |
| `project_panel.new_folder` | New Folder… | <kbd>⌥⇧⌘N</kbd> | <kbd>Ctrl+Alt+Shift+N</kbd> |
| `project_panel.new_menu` | New… | <kbd>⌘N</kbd> | <kbd>Alt+N</kbd> |
| `project_panel.open` | Open | <kbd>↩</kbd>, <kbd>L</kbd> | <kbd>Enter</kbd>, <kbd>L</kbd> |
| `project_panel.open_in_terminal` | Open in Terminal | — | — |
| `project_panel.open_to_side` | Open to the Side | <kbd>⌘↩</kbd> | <kbd>Ctrl+Enter</kbd> |
| `project_panel.paste` | Paste | <kbd>⌘V</kbd>, <kbd>P</kbd> | <kbd>Ctrl+V</kbd>, <kbd>P</kbd> |
| `project_panel.redo` | Redo File Operation | <kbd>⇧⌘Z</kbd> | — |
| `project_panel.refresh` | Refresh | <kbd>G R</kbd> | <kbd>G R</kbd> |
| `project_panel.rename` | Rename… | <kbd>F2</kbd>, <kbd>R</kbd>, <kbd>⇧R</kbd> | <kbd>F2</kbd>, <kbd>R</kbd>, <kbd>Shift+R</kbd> |
| `project_panel.reveal_active` | Reveal Active File in Project | — | — |
| `project_panel.reveal_in_os` | Reveal in File Manager | — | — |
| `project_panel.select_first` | Select First | <kbd>G G</kbd> | <kbd>G G</kbd> |
| `project_panel.select_last` | Select Last | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> |
| `project_panel.select_next` | Select Next | <kbd>↓</kbd>, <kbd>J</kbd> | <kbd>Down</kbd>, <kbd>J</kbd> |
| `project_panel.select_prev` | Select Previous | <kbd>↑</kbd>, <kbd>K</kbd> | <kbd>Up</kbd>, <kbd>K</kbd> |
| `project_panel.toggle` | Toggle Folder or Open File | <kbd>O</kbd> | <kbd>O</kbd> |
| `project_panel.toggle_ignored` | Toggle Ignored Files | <kbd>⇧H</kbd> | <kbd>Shift+H</kbd> |
| `project_panel.undo` | Undo File Operation | <kbd>⌘Z</kbd> | — |

## `proposal`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `proposal.accept_all` | Accept All Proposed Changes | <kbd>⌘↩</kbd> | <kbd>Ctrl+Alt+Shift+Y</kbd> |
| `proposal.accept_hunk` | Accept Proposed Change | <kbd>⌘Y</kbd> | <kbd>Ctrl+Alt+Y</kbd> |
| `proposal.next_hunk` | Next Proposed Change | <kbd>⌥F5</kbd> | <kbd>Alt+F5</kbd> |
| `proposal.previous_hunk` | Previous Proposed Change | <kbd>⌥⇧F5</kbd> | <kbd>Alt+Shift+F5</kbd> |
| `proposal.reject_all` | Reject All Proposed Changes | <kbd>⌘⌫</kbd> | <kbd>Ctrl+Alt+Shift+N</kbd> |
| `proposal.reject_hunk` | Reject Proposed Change | <kbd>⌘N</kbd> | <kbd>Ctrl+Alt+N</kbd> |

## `quick_open`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `quick_open.toggle` | Go to File | <kbd>⌘P</kbd> | <kbd>Ctrl+P</kbd> |

## `quickfix`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `quickfix.first` | Quickfix: First Item | — | — |
| `quickfix.last` | Quickfix: Last Item | — | — |
| `quickfix.next` | Quickfix: Next Item | <kbd>&#93; Q</kbd> | <kbd>&#93; Q</kbd> |
| `quickfix.previous` | Quickfix: Previous Item | <kbd>&#91; Q</kbd> | <kbd>&#91; Q</kbd> |
| `quickfix.show` | Quickfix: Show List | — | — |
| `quickfix.toggle` | Quickfix: Toggle List | — | — |

## `recent`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `recent.open_in_new_window` | Open Recent in New Window | — | — |

## `references_panel`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `references_panel.open` | Open Reference | <kbd>↩</kbd>, <kbd>O</kbd> | <kbd>Enter</kbd>, <kbd>O</kbd> |
| `references_panel.select_first` | Select First Reference | <kbd>G G</kbd> | <kbd>G G</kbd> |
| `references_panel.select_last` | Select Last Reference | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> |
| `references_panel.select_next` | Select Next Reference | <kbd>↓</kbd>, <kbd>J</kbd> | <kbd>Down</kbd>, <kbd>J</kbd> |
| `references_panel.select_prev` | Select Previous Reference | <kbd>↑</kbd>, <kbd>K</kbd> | <kbd>Up</kbd>, <kbd>K</kbd> |

## `runs`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `runs.ask_agent_about_failure` | Runs: Ask an Agent About This Failure | — | — |
| `runs.build` | Runs: Build Project | — | — |
| `runs.rerun_failed` | Runs: Rerun Failed Tests | <kbd>Space T F</kbd> | <kbd>Space T F</kbd> |
| `runs.show` | Show Runs | <kbd>Space R</kbd> | <kbd>Space R</kbd> |
| `runs.test_file` | Runs: Test File | — | — |
| `runs.test_project` | Runs: Test Project | — | — |

## `search`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `search.buffer_lines` | Search Lines in Buffer | — | — |
| `search.clear_results` | Clear Search Results | — | — |
| `search.collapse_all` | Collapse All | — | — |
| `search.collapse_or_parent` | Collapse Result or Select Its File | <kbd>←</kbd> | <kbd>Left</kbd> |
| `search.dismiss_file` | Dismiss Selected File | — | — |
| `search.expand_all` | Expand All | — | — |
| `search.expand_or_first_child` | Expand Result or Select Its First Match | <kbd>→</kbd> | <kbd>Right</kbd> |
| `search.focus_results` | Focus Search Results | — | — |
| `search.open_selected` | Open Selected Result | <kbd>↩</kbd>, <kbd>O</kbd> | <kbd>Enter</kbd>, <kbd>O</kbd> |
| `search.refresh` | Refresh | — | — |
| `search.replace_all` | Replace All in Files | <kbd>⌥⌘↩</kbd> | <kbd>Ctrl+Alt+Enter</kbd> |
| `search.replace_in_file` | Replace in Selected File | — | — |
| `search.select_first` | Select First Result | <kbd>G G</kbd> | <kbd>G G</kbd> |
| `search.select_last` | Select Last Result | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> |
| `search.select_next` | Select Next Result | <kbd>↓</kbd>, <kbd>J</kbd> | <kbd>Down</kbd>, <kbd>J</kbd> |
| `search.select_previous` | Select Previous Result | <kbd>↑</kbd>, <kbd>K</kbd> | <kbd>Up</kbd>, <kbd>K</kbd> |
| `search.show` | Find in Files | <kbd>⇧⌘F</kbd> | <kbd>Ctrl+Shift+F</kbd> |
| `search.show_replace` | Replace in Files… | <kbd>⇧⌘H</kbd> | <kbd>Ctrl+Shift+H</kbd> |
| `search.toggle_case` | Toggle Match Case | <kbd>⌥⌘C</kbd> | <kbd>Alt+C</kbd> |
| `search.toggle_details` | Toggle Search Details | <kbd>⇧⌘J</kbd> | <kbd>Ctrl+Shift+J</kbd> |
| `search.toggle_preserve_case` | Toggle Preserve Case | <kbd>⌥⌘P</kbd> | <kbd>Alt+P</kbd> |
| `search.toggle_regex` | Toggle Regex | <kbd>⌥⌘R</kbd> | <kbd>Alt+R</kbd> |
| `search.toggle_replace` | Toggle Replace | — | — |
| `search.toggle_whole_word` | Toggle Whole Word | <kbd>⌥⌘W</kbd> | <kbd>Alt+W</kbd> |
| `search.view_as_list` | View as List | — | — |
| `search.view_as_tree` | View as Tree | — | — |

## `settings`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `settings.add_list_item` | Settings: Add List Item | — | — |
| `settings.remove_list_item` | Settings: Remove List Item | — | — |
| `settings.reset_to_default` | Settings: Reset to Default | — | — |
| `settings.switch_scope` | Settings: Switch Scope | — | — |

## `shortcuts`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `shortcuts.copy_command_id` | Keyboard Shortcuts: Copy Command ID | — | — |
| `shortcuts.reset_binding` | Keyboard Shortcuts: Reset Keybinding | — | — |
| `shortcuts.show_same_keybindings` | Keyboard Shortcuts: Show Same Keybindings | — | — |

## `spell`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `spell.add_word` | Spelling: Add Word to Dictionary | — | — |
| `spell.ignore_word` | Spelling: Ignore Word | — | — |
| `spell.next` | Spelling: Next Misspelling | — | — |
| `spell.previous` | Spelling: Previous Misspelling | — | — |
| `spell.suggest` | Spelling: Suggest Corrections… | <kbd>⌥⌘.</kbd> | <kbd>Ctrl+Alt+.</kbd> |
| `spell.toggle` | Spelling: Toggle Spell Check | — | — |

## `start_page`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `start_page.open_recent_file` | Open Recent File | — | — |
| `start_page.open_recent_folder` | Open Recent Folder | — | — |

## `tasks`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `tasks.rerun_last` | Tasks: Rerun Last Task | — | — |
| `tasks.run` | Tasks: Run Task… | — | — |
| `tasks.run_build` | Tasks: Run Build Task | <kbd>⇧⌘B</kbd> | <kbd>Ctrl+Shift+B</kbd> |

## `terminal`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `terminal.close` | Kill Terminal | — | — |
| `terminal.find` | Terminal: Find | <kbd>⌘F</kbd> | <kbd>Ctrl+Shift+F</kbd> |
| `terminal.move_to_bottom` | Move Terminal to Bottom | — | — |
| `terminal.move_to_left` | Move Terminal to Left | — | — |
| `terminal.move_to_right` | Move Terminal to Right | — | — |
| `terminal.new` | New Terminal | <kbd>⌃⇧&#96;</kbd> | <kbd>Ctrl+&#126;</kbd> |
| `terminal.next` | Next Terminal | <kbd>⇧⌘&#93;</kbd> | <kbd>Ctrl+PageDown</kbd> |
| `terminal.next_prompt` | Terminal: Next Prompt | — | — |
| `terminal.previous` | Previous Terminal | <kbd>⇧⌘&#91;</kbd> | <kbd>Ctrl+PageUp</kbd> |
| `terminal.previous_prompt` | Terminal: Previous Prompt | — | — |
| `terminal.rename` | Rename Terminal | — | — |
| `terminal.toggle` | Toggle Terminal | <kbd>⌃&#96;</kbd> | <kbd>Ctrl+&#96;</kbd> |
| `terminal.vi_mode` | Terminal: Vi Copy Mode | — | — |

## `tests`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `tests.rerun_last` | Rerun Last Test | — | — |
| `tests.run_at_cursor` | Run Test at Cursor | — | — |
| `tests.run_file` | Run Tests in File | — | — |
| `tests.run_row` | Run Test at Row: the lens above a test, which runs that test, as | — | — |

## `text_input`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `text_input.backspace` | Delete Backward | <kbd>⌫</kbd> | <kbd>Backspace</kbd> |
| `text_input.clear` | Clear | <kbd>⌘⌫</kbd> | <kbd>Ctrl+Shift+Backspace</kbd> |
| `text_input.delete_forward` | Delete Forward | <kbd>⌦</kbd> | <kbd>Delete</kbd> |
| `text_input.delete_word_left` | Delete Word Backward | <kbd>⌥⌫</kbd> | <kbd>Ctrl+Backspace</kbd> |
| `text_input.move_end` | Move to End | <kbd>↘</kbd>, <kbd>⌘→</kbd> | <kbd>End</kbd> |
| `text_input.move_home` | Move to Start | <kbd>↖</kbd>, <kbd>⌘←</kbd> | <kbd>Home</kbd> |
| `text_input.move_left` | Move Left | <kbd>←</kbd> | <kbd>Left</kbd> |
| `text_input.move_right` | Move Right | <kbd>→</kbd> | <kbd>Right</kbd> |
| `text_input.move_word_left` | Move Word Left | <kbd>⌥←</kbd> | <kbd>Ctrl+Left</kbd> |
| `text_input.move_word_right` | Move Word Right | <kbd>⌥→</kbd> | <kbd>Ctrl+Right</kbd> |
| `text_input.paste` | Paste | <kbd>⌘V</kbd> | <kbd>Ctrl+V</kbd> |
| `text_input.submit` | Submit | <kbd>↩</kbd> | <kbd>Enter</kbd> |

## `theme`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `theme.select` | Change Color Theme… | <kbd>⌘K ⌘T</kbd> | <kbd>Ctrl+K Ctrl+T</kbd> |

## `toast`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `toast.dismiss` | Dismiss Notification | <kbd>⎋</kbd> | <kbd>Esc</kbd> |
| `toast.dismiss_all` | Dismiss Notifications | <kbd>⌘K ⎋</kbd> | <kbd>Ctrl+K Esc</kbd> |
| `toast.focus` | Toast: Focus | — | — |
| `toast.next_button` | Next Notification Button | <kbd>⇥</kbd> | <kbd>Tab</kbd> |
| `toast.previous_button` | Previous Notification Button | <kbd>⇧⇥</kbd> | <kbd>Shift+Tab</kbd> |

## `todo`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `todo.list` | Search TODO Comments | — | — |
| `todo.next` | Next TODO Comment | — | — |
| `todo.previous` | Previous TODO Comment | — | — |

## `view`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `view.toggle_breadcrumbs` | Toggle Breadcrumbs | — | — |
| `view.toggle_status_bar` | Toggle Status Bar | — | — |

## `vim`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `vim.blank_lines` | Vim Blank Lines | — | — |
| `vim.change` | Vim Change | — | — |
| `vim.change_newer` | Vim Change Newer | — | — |
| `vim.change_older` | Vim Change Older | — | — |
| `vim.command_line` | Vim Command Line | — | — |
| `vim.command_line_cancel` | Vim Cancel Command Line | — | — |
| `vim.command_line_submit` | Vim Submit Command Line | — | — |
| `vim.diff_get` | Vim Diff Get | — | — |
| `vim.diff_put` | Vim Diff Put | — | — |
| `vim.earlier` | Vim Earlier Text State | — | — |
| `vim.ex_line` | Vim Run Ex Line | — | — |
| `vim.exchange_clear` | Vim Exchange Clear | — | — |
| `vim.expand_selection` | Vim Expand Selection | <kbd>⌃⇧⌘→</kbd> | <kbd>Alt+Shift+Right</kbd> |
| `vim.filter` | Vim Filter | — | — |
| `vim.fold` | Vim Fold | — | — |
| `vim.increment` | Vim Increment | — | — |
| `vim.insert` | Vim Insert | — | — |
| `vim.insert_last` | Vim Insert At Last Insert | — | — |
| `vim.join` | Vim Join Lines | — | — |
| `vim.join_raw` | Vim Join Lines Without Spaces | — | — |
| `vim.jump` | Vim Jump | — | — |
| `vim.jump_newer` | Vim Jump Newer | — | — |
| `vim.jump_older` | Vim Jump Older | — | — |
| `vim.key` | Vim Key | <kbd>↩</kbd>, <kbd>⇥</kbd>, <kbd>⌫</kbd>, <kbd>⇧⌫</kbd>, <kbd>⌦</kbd>, <kbd>⌥⌫</kbd> | <kbd>Ctrl+W</kbd>, <kbd>Ctrl+U</kbd>, <kbd>Ctrl+R</kbd>, <kbd>Ctrl+T</kbd>, <kbd>Ctrl+D</kbd>, <kbd>Ctrl+O</kbd>, <kbd>Ctrl+N</kbd>, <kbd>Ctrl+P</kbd>, <kbd>Ctrl+A</kbd>, <kbd>Enter</kbd>, <kbd>Tab</kbd>, <kbd>Backspace</kbd>, <kbd>Shift+Backspace</kbd>, <kbd>Delete</kbd>, <kbd>Ctrl+Backspace</kbd>, <kbd>Ctrl+V</kbd>, <kbd>Ctrl+Q</kbd>, <kbd>Ctrl+X</kbd>, <kbd>Ctrl+Y</kbd>, <kbd>Ctrl+B</kbd>, <kbd>Ctrl+F</kbd> |
| `vim.later` | Vim Later Text State | — | — |
| `vim.line_undo` | Vim Line Undo | — | — |
| `vim.move` | Vim Move | <kbd>↓</kbd>, <kbd>←</kbd>, <kbd>→</kbd>, <kbd>↑</kbd> | <kbd>Down</kbd>, <kbd>Left</kbd>, <kbd>Right</kbd>, <kbd>Up</kbd> |
| `vim.move_rows` | Vim Move Rows | — | — |
| `vim.normal_mode` | Vim Normal Mode | <kbd>⎋</kbd>, <kbd>⌃&#91;</kbd> | <kbd>Esc</kbd>, <kbd>Ctrl+&#91;</kbd> |
| `vim.normal_resume` | Vim Resume Normal | — | — |
| `vim.operate` | Vim Operate | — | — |
| `vim.play` | Vim Play Macro | — | — |
| `vim.put` | Vim Put | — | — |
| `vim.quit_discard` | Vim Quit Discard | — | — |
| `vim.record` | Vim Record Macro | — | — |
| `vim.record_stop` | Vim Stop Recording | — | — |
| `vim.redo` | Vim Redo | — | — |
| `vim.repeat` | Vim Repeat | — | — |
| `vim.repeat_substitute` | Vim Repeat Substitute | — | — |
| `vim.replace_char` | Vim Replace Character | — | — |
| `vim.replace_mode` | Vim Replace Mode | — | — |
| `vim.replace_normal_mode` | Vim Normal Mode from Replace | — | — |
| `vim.reselect` | Vim Reselect | — | — |
| `vim.resume_insert` | Vim Resume Insert | — | — |
| `vim.scroll` | Vim Scroll | — | — |
| `vim.select` | Vim Select Text Object | — | — |
| `vim.set_mark` | Vim Set Mark | — | — |
| `vim.shrink_selection` | Vim Shrink Selection | <kbd>⌃⇧⌘←</kbd> | <kbd>Alt+Shift+Left</kbd> |
| `vim.substitute_all_lines` | Vim Substitute All Lines | — | — |
| `vim.substitute_answer` | Vim Substitute Answer | — | — |
| `vim.surround` | Vim Surround | — | — |
| `vim.surround_add` | Vim Surround Add | — | — |
| `vim.toggle` | Toggle Vim Mode | — | — |
| `vim.toggle_case` | Vim Toggle Case | — | — |
| `vim.undo` | Vim Undo | — | — |
| `vim.virtual_replace_mode` | Vim Virtual Replace Mode | — | — |
| `vim.visual` | Vim Visual Mode | — | — |
| `vim.visual_exit` | Vim Leave Visual Mode | — | — |
| `vim.visual_put` | Vim Put Over Selection | — | — |
| `vim.visual_swap` | Vim Swap Selection Ends | — | — |
| `vim.write_quit` | Vim Write Quit | — | — |

## `vim_line`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `vim_line.cancel` | Cancel Command Line | <kbd>⎋</kbd> | <kbd>Esc</kbd> |

## `window`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `window.close` | Close Window | <kbd>⇧⌘W</kbd> | <kbd>Ctrl+Shift+W</kbd> |
| `window.minimize` | Minimize | <kbd>⌘M</kbd> | — |
| `window.new` | New Window | <kbd>⇧⌘N</kbd> | <kbd>Ctrl+Shift+N</kbd> |
| `window.toggle_fullscreen` | Toggle Full Screen | <kbd>⌃⌘F</kbd> | <kbd>F11</kbd> |
| `window.zoom` | Zoom | — | — |

## `workbench`

| ID | Title | macOS | Linux and Windows |
|---|---|---|---|
| `workbench.cancel_vim_filter` | Vim: Cancel Filter | — | — |
| `workbench.change_encoding` | Change File Encoding | — | — |
| `workbench.change_line_ending` | Change End of Line Sequence… | — | — |
| `workbench.clear_plugin_data` | Extensions: Clear Plugin Data | — | — |
| `workbench.clear_recent` | Clear Recently Opened | — | — |
| `workbench.close_all_editors` | Close All Editors | — | — |
| `workbench.close_folder` | Close Folder | <kbd>⌘K F</kbd> | <kbd>Ctrl+K F</kbd> |
| `workbench.close_group_editors` | Close Editors in Group | <kbd>⌘K W</kbd> | <kbd>Ctrl+K W</kbd> |
| `workbench.close_other_tabs` | Close Other Tabs | — | — |
| `workbench.close_saved` | Close Saved Editors | <kbd>⌘K U</kbd> | <kbd>Ctrl+K U</kbd> |
| `workbench.close_tab` | Close Tab | <kbd>⌘W</kbd> | <kbd>Ctrl+W</kbd> |
| `workbench.configure_snippets` | Configure User Snippets | — | — |
| `workbench.copy_active_file_path` | Copy Path of Active File | <kbd>⌘K P</kbd> | <kbd>Ctrl+K P</kbd> |
| `workbench.copy_active_file_relative_path` | Copy Relative Path of Active File | — | — |
| `workbench.copy_reference` | Copy Reference | — | — |
| `workbench.harpoon_add` | Harpoon: Add File | — | — |
| `workbench.harpoon_jump1` | Harpoon: Jump to File 1 | — | — |
| `workbench.harpoon_jump2` | Harpoon: Jump to File 2 | — | — |
| `workbench.harpoon_jump3` | Harpoon: Jump to File 3 | — | — |
| `workbench.harpoon_jump4` | Harpoon: Jump to File 4 | — | — |
| `workbench.harpoon_jump5` | Harpoon: Jump to File 5 | — | — |
| `workbench.harpoon_jump6` | Harpoon: Jump to File 6 | — | — |
| `workbench.harpoon_jump7` | Harpoon: Jump to File 7 | — | — |
| `workbench.harpoon_jump8` | Harpoon: Jump to File 8 | — | — |
| `workbench.harpoon_jump9` | Harpoon: Jump to File 9 | — | — |
| `workbench.harpoon_remove_current` | Harpoon: Remove File | — | — |
| `workbench.harpoon_toggle_list` | Harpoon: Show List | — | — |
| `workbench.insert_snippet` | Insert Snippet | — | — |
| `workbench.install_extension` | Extensions: Install from File or Folder… | — | — |
| `workbench.keep_editor` | Keep Editor | — | — |
| `workbench.last_editor_in_group` | Last Editor in Group | — | — |
| `workbench.link_extension_folder` | Extensions: Install Folder for Development… | — | — |
| `workbench.new_file` | New File | <kbd>⌘N</kbd> | <kbd>Ctrl+N</kbd> |
| `workbench.next_tab` | Next Tab | <kbd>&#93; B</kbd>, <kbd>⇧⌘&#93;</kbd> | <kbd>&#93; B</kbd>, <kbd>Ctrl+PageDown</kbd> |
| `workbench.open_editor_at_index1` | Open Editor at Index 1 | <kbd>⌃1</kbd> | <kbd>Alt+1</kbd> |
| `workbench.open_editor_at_index2` | Open Editor at Index 2 | <kbd>⌃2</kbd> | <kbd>Alt+2</kbd> |
| `workbench.open_editor_at_index3` | Open Editor at Index 3 | <kbd>⌃3</kbd> | <kbd>Alt+3</kbd> |
| `workbench.open_editor_at_index4` | Open Editor at Index 4 | <kbd>⌃4</kbd> | <kbd>Alt+4</kbd> |
| `workbench.open_editor_at_index5` | Open Editor at Index 5 | <kbd>⌃5</kbd> | <kbd>Alt+5</kbd> |
| `workbench.open_editor_at_index6` | Open Editor at Index 6 | <kbd>⌃6</kbd> | <kbd>Alt+6</kbd> |
| `workbench.open_editor_at_index7` | Open Editor at Index 7 | <kbd>⌃7</kbd> | <kbd>Alt+7</kbd> |
| `workbench.open_editor_at_index8` | Open Editor at Index 8 | <kbd>⌃8</kbd> | <kbd>Alt+8</kbd> |
| `workbench.open_editor_at_index9` | Open Editor at Index 9 | <kbd>⌃9</kbd> | <kbd>Alt+9</kbd> |
| `workbench.open_file` | Open File… | — | — |
| `workbench.open_folder` | Open Folder… | <kbd>⌘O</kbd> | <kbd>Ctrl+O</kbd> |
| `workbench.open_folder_in_new_window` | Open Folder in New Window… | — | — |
| `workbench.open_keyboard_shortcuts` | Open Keyboard Shortcuts | <kbd>⌘K ⌘S</kbd> | <kbd>Ctrl+K Ctrl+S</kbd> |
| `workbench.open_keyboard_shortcuts_json` | Open Keyboard Shortcuts (JSON) | — | — |
| `workbench.open_path` | Open Path | — | — |
| `workbench.open_recent` | Open Recent… | <kbd>⌃R</kbd> | <kbd>Ctrl+R</kbd> |
| `workbench.open_settings` | Open Settings (JSON) | — | — |
| `workbench.open_settings_ui` | Open Settings (UI) | <kbd>⌘,</kbd> | <kbd>Ctrl+,</kbd> |
| `workbench.pick_breadcrumb_path` | Breadcrumbs: Pick from Folder | — | — |
| `workbench.pick_breadcrumb_symbol` | Breadcrumbs: Pick Symbol | — | — |
| `workbench.pin_tab` | Pin Tab | — | — |
| `workbench.previous_tab` | Previous Tab | <kbd>&#91; B</kbd>, <kbd>⇧⌘&#91;</kbd> | <kbd>&#91; B</kbd>, <kbd>Ctrl+PageUp</kbd> |
| `workbench.reopen_closed_editor` | Reopen Closed Editor | <kbd>⇧⌘T</kbd> | <kbd>Ctrl+Shift+T</kbd> |
| `workbench.reopen_with_encoding` | Reopen Editor with Encoding… | — | — |
| `workbench.reveal_active_file_in_os` | Reveal Active File in File Manager | <kbd>⌘K R</kbd> | <kbd>Ctrl+K R</kbd> |
| `workbench.revoke_plugin_grants` | Extensions: Revoke Plugin Grants | — | — |
| `workbench.save_all` | Save All | <kbd>⌥⌘S</kbd> | — |
| `workbench.save_as` | Save As… | <kbd>⇧⌘S</kbd> | <kbd>Ctrl+Shift+S</kbd> |
| `workbench.save_with_encoding` | Save with Encoding… | — | — |
| `workbench.save_without_formatting` | Save without Formatting | <kbd>⌘K S</kbd> | — |
| `workbench.show_config_files` | Open Config File… | — | — |
| `workbench.show_config_problems` | Show Config Problems | — | — |
| `workbench.show_extensions` | Extensions: Show Installed | — | — |
| `workbench.show_plugin_log` | Extensions: Show Plugin Log | — | — |
| `workbench.tab_switcher_next` | Switch Tab | <kbd>⌃⇥</kbd> | <kbd>Ctrl+Tab</kbd> |
| `workbench.tab_switcher_previous` | Switch Tab Backward | <kbd>⌃⇧⇥</kbd> | <kbd>Ctrl+Shift+Tab</kbd> |
| `workbench.toggle_zen_mode` | Toggle Zen Mode | <kbd>⌘K Z</kbd>, <kbd>⎋ ⎋</kbd> | <kbd>Ctrl+K Z</kbd>, <kbd>Esc Esc</kbd> |
| `workbench.trust_folder` | Trust This Folder | — | — |
| `workbench.uninstall_plugin` | Extensions: Uninstall Plugin | — | — |
| `workbench.unpin_tab` | Unpin Tab | — | — |
| `workbench.untrust_folder` | Untrust This Folder | — | — |
