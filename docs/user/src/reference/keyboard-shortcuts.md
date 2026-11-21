# Keyboard shortcuts

> Generated; do not edit. `cargo run -p fastedit --example gen_user_docs` writes this page from the built-in keymap (`crates/fe-keymap/keymaps/fastedit.toml`).

The default key bindings, by command, with the keys on each platform and the context they apply in. Your `keymap.toml` overrides them.

## `ai`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `ai.changes_next_file` | Select Next AI Changed File | <kbd>⌥&#93;</kbd> | <kbd>Alt+&#93;</kbd> | `ai_changes` |
| `ai.changes_previous_file` | Select Previous AI Changed File | <kbd>⌥&#91;</kbd> | <kbd>Alt+&#91;</kbd> | `ai_changes` |
| `ai.inline_edit` | Inline Edit with AI | <kbd>⌘I</kbd> | <kbd>Ctrl+I</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |

## `app`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `app.hide` | Hide Tenmoku | <kbd>⌘H</kbd> | — | everywhere |
| `app.hide_others` | Hide Others | <kbd>⌥⌘H</kbd> | — | everywhere |
| `app.quit` | Quit | <kbd>⌘Q</kbd> | <kbd>Ctrl+Q</kbd> | everywhere |

## `chat`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `chat.open` | Chat: Open | <kbd>⌃⌘I</kbd> | <kbd>Ctrl+Alt+I</kbd> | `workbench` |
| `chat.send` | Send Message | <kbd>↩</kbd> | <kbd>Enter</kbd> | `chat_composer && editor` |
| `chat.stop` | Stop Reply | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `chat_composer && editor` |

## `command_palette`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `command_palette.toggle` | Show All Commands | <kbd>⇧⌘P</kbd> | <kbd>Ctrl+Shift+P</kbd> | `workbench` |

## `completion`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `completion.accept` | Accept Completion | <kbd>↩</kbd>, <kbd>⇥</kbd> | <kbd>Enter</kbd>, <kbd>Tab</kbd> | `completion` |
| `completion.dismiss` | Dismiss Completion | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `completion` |
| `completion.select_next` | Select Next Completion | <kbd>↓</kbd> | <kbd>Down</kbd> | `completion` |
| `completion.select_prev` | Select Previous Completion | <kbd>↑</kbd> | <kbd>Up</kbd> | `completion` |

## `debug`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `debug.pause` | Debug: Pause | <kbd>F6</kbd> | <kbd>F6</kbd> | `workbench && debug_running` |
| `debug.restart` | Debug: Restart | <kbd>⇧⌘F5</kbd> | <kbd>Ctrl+Shift+F5</kbd> | `workbench && debug_session` |
| `debug.run_to_cursor` | Debug: Run to Cursor | <kbd>⌘F10</kbd> | <kbd>Ctrl+F10</kbd> | `workbench && debug_stopped` |
| `debug.start` | Debug: Start Debugging | <kbd>F5</kbd> | <kbd>F5</kbd> | `workbench` |
| `debug.step_into` | Debug: Step Into | <kbd>F11</kbd> | <kbd>F11</kbd> | `workbench && debug_stopped` |
| `debug.step_out` | Debug: Step Out | <kbd>⇧F11</kbd> | <kbd>Shift+F11</kbd> | `workbench && debug_stopped` |
| `debug.step_over` | Debug: Step Over | <kbd>F10</kbd> | <kbd>F10</kbd> | `workbench && debug_stopped` |
| `debug.stop` | Debug: Stop | <kbd>⇧F5</kbd> | <kbd>Shift+F5</kbd> | `workbench && debug_session` |
| `debug.toggle_breakpoint` | Debug: Toggle Breakpoint | <kbd>F9</kbd> | <kbd>F9</kbd> | `editor` |

## `diagnostics`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `diagnostics.next` | Go to Next Problem | <kbd>&#93; D</kbd> | <kbd>&#93; D</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `diagnostics.next` | Go to Next Problem | <kbd>F8</kbd> | <kbd>F8</kbd> | `workbench` |
| `diagnostics.previous` | Go to Previous Problem | <kbd>&#91; D</kbd> | <kbd>&#91; D</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `diagnostics.previous` | Go to Previous Problem | <kbd>⇧F8</kbd> | <kbd>Shift+F8</kbd> | `workbench` |

## `diff`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `diff.compare_saved` | Compare Active File with Saved | <kbd>⌘K D</kbd> | <kbd>Ctrl+K D</kbd> | `editor` |
| `diff.next_change` | Go to Next Difference | <kbd>F7</kbd>, <kbd>⌥F5</kbd> | <kbd>F7</kbd>, <kbd>Alt+F5</kbd> | `CommitDiffView` |
| `diff.next_change` | Go to Next Difference | <kbd>F7</kbd>, <kbd>⌥F5</kbd> | <kbd>F7</kbd>, <kbd>Alt+F5</kbd> | `DiffView` |
| `diff.previous_change` | Go to Previous Difference | <kbd>⇧F7</kbd>, <kbd>⌥⇧F5</kbd> | <kbd>Shift+F7</kbd>, <kbd>Alt+Shift+F5</kbd> | `CommitDiffView` |
| `diff.previous_change` | Go to Previous Difference | <kbd>⇧F7</kbd>, <kbd>⌥⇧F5</kbd> | <kbd>Shift+F7</kbd>, <kbd>Alt+Shift+F5</kbd> | `DiffView` |

## `dock`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `dock.focus_down` | Focus Below Dock | <kbd>⌃W J</kbd>, <kbd>⌃W ⌃J</kbd> | <kbd>Ctrl+W J</kbd>, <kbd>Ctrl+W Ctrl+J</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel \|\| git_log_panel \|\| terminal_panel) && vim && !editor && !text_input` |
| `dock.focus_down` | Focus Below Dock | <kbd>⌃J</kbd> | <kbd>Ctrl+J</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel) && vim && !editor && !text_input` |
| `dock.focus_editor` | Focus Active Editor | <kbd>⌃W P</kbd>, <kbd>⌃W W</kbd> | <kbd>Ctrl+W P</kbd>, <kbd>Ctrl+W W</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel \|\| git_log_panel \|\| terminal_panel) && vim && !editor && !text_input` |
| `dock.focus_editor` | Focus Active Editor | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel) && vim && !editor && !text_input` |
| `dock.focus_left` | Focus Left of Dock | <kbd>⌃W H</kbd>, <kbd>⌃W ⌃H</kbd> | <kbd>Ctrl+W H</kbd>, <kbd>Ctrl+W Ctrl+H</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel \|\| git_log_panel \|\| terminal_panel) && vim && !editor && !text_input` |
| `dock.focus_left` | Focus Left of Dock | <kbd>⌃H</kbd> | <kbd>Ctrl+H</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel) && vim && !editor && !text_input` |
| `dock.focus_right` | Focus Right of Dock | <kbd>⌃W L</kbd>, <kbd>⌃W ⌃L</kbd> | <kbd>Ctrl+W L</kbd>, <kbd>Ctrl+W Ctrl+L</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel \|\| git_log_panel \|\| terminal_panel) && vim && !editor && !text_input` |
| `dock.focus_right` | Focus Right of Dock | <kbd>⌃L</kbd> | <kbd>Ctrl+L</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel) && vim && !editor && !text_input` |
| `dock.focus_up` | Focus Above Dock | <kbd>⌃W K</kbd>, <kbd>⌃W ⌃K</kbd> | <kbd>Ctrl+W K</kbd>, <kbd>Ctrl+W Ctrl+K</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel \|\| git_log_panel \|\| terminal_panel) && vim && !editor && !text_input` |
| `dock.focus_up` | Focus Above Dock | <kbd>⌃K</kbd> | <kbd>Ctrl+K</kbd> | `(project_panel \|\| outline_panel \|\| ReferencesPanel \|\| problems_panel \|\| runs_panel \|\| debug_panel \|\| quickfix_panel \|\| search_results \|\| git_panel) && vim && !editor && !text_input` |
| `dock.show_project` | Show Project | <kbd>⇧⌘E</kbd> | <kbd>Ctrl+Shift+E</kbd> | `workbench` |
| `dock.toggle` | Toggle Dock | <kbd>⌘B</kbd> | <kbd>Ctrl+B</kbd> | `workbench` |

## `editor`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `editor.add_caret_at_next_occurrence` | Add Caret at Next Occurrence | <kbd>G B</kbd>, <kbd>⌘D</kbd> | <kbd>G B</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `editor.add_carets_at_all_occurrences` | Add Carets at All Occurrences | <kbd>⇧⌘L</kbd> | — | `editor && vim.mode == normal && !vim.pending` |
| `editor.add_cursor_above` | Add Cursor Above | <kbd>⌥⌘↑</kbd> | <kbd>Alt+Shift+Up</kbd> | `editor && !vim.enabled` |
| `editor.add_cursor_above` | Add Cursor Above | <kbd>⌥⌘↑</kbd> | <kbd>Alt+Shift+Up</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `editor.add_cursor_below` | Add Cursor Below | <kbd>⌥⌘↓</kbd> | <kbd>Alt+Shift+Down</kbd> | `editor && !vim.enabled` |
| `editor.add_cursor_below` | Add Cursor Below | <kbd>⌥⌘↓</kbd> | <kbd>Alt+Shift+Down</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `editor.backspace` | Delete Backward | <kbd>⌫</kbd>, <kbd>⇧⌫</kbd> | <kbd>Backspace</kbd>, <kbd>Shift+Backspace</kbd> | `editor` |
| `editor.column_select_down` | Column Select Down | <kbd>⌥⇧⌘↓</kbd> | — | `editor && !vim.enabled` |
| `editor.column_select_left` | Column Select Left | <kbd>⌥⇧⌘←</kbd> | — | `editor && !vim.enabled` |
| `editor.column_select_right` | Column Select Right | <kbd>⌥⇧⌘→</kbd> | — | `editor && !vim.enabled` |
| `editor.column_select_up` | Column Select Up | <kbd>⌥⇧⌘↑</kbd> | — | `editor && !vim.enabled` |
| `editor.copy` | Copy | <kbd>⌘C</kbd> | <kbd>Ctrl+C</kbd> | `editor` |
| `editor.copy_rows_up` | Copy Rows Up | <kbd>⌥⇧↑</kbd> | <kbd>Ctrl+Alt+Shift+Up</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.cursor_undo` | Cursor Undo | <kbd>⌘U</kbd> | <kbd>Ctrl+U</kbd> | `editor && !vim.enabled` |
| `editor.cut` | Cut | <kbd>⌘X</kbd> | <kbd>Ctrl+X</kbd> | `editor` |
| `editor.delete` | Delete Forward | <kbd>⌦</kbd> | <kbd>Delete</kbd> | `editor` |
| `editor.delete_all_left` | Delete All Left | <kbd>⌘⌫</kbd> | — | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.delete_all_right` | Delete All Right | <kbd>⌃K</kbd> | — | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.delete_rows` | Delete Rows | <kbd>⇧⌘K</kbd> | <kbd>Ctrl+Shift+K</kbd> | `editor` |
| `editor.delete_subword_left` | Delete Subword Left | <kbd>⌃⌥⌫</kbd> | — | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.delete_subword_right` | Delete Subword Right | <kbd>⌃⌥⌦</kbd> | — | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.delete_word_left` | Delete Word Left | <kbd>⌥⌫</kbd> | <kbd>Ctrl+Backspace</kbd> | `editor` |
| `editor.delete_word_right` | Delete Word Right | <kbd>⌥⌦</kbd> | <kbd>Ctrl+Delete</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.duplicate_rows` | Duplicate Rows | <kbd>⌥⇧↓</kbd> | <kbd>Ctrl+Alt+Shift+Down</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.expand_selection` | Expand Selection | <kbd>⌃⇧⌘→</kbd> | <kbd>Alt+Shift+Right</kbd> | `editor && !vim.enabled` |
| `editor.fold` | Fold | <kbd>⌥⌘&#91;</kbd> | <kbd>Ctrl+{</kbd> | `editor` |
| `editor.fold_all` | Fold All | <kbd>⌘K ⌘0</kbd> | <kbd>Ctrl+K Ctrl+0</kbd> | `editor` |
| `editor.fold_level_1` | Fold Level 1 | <kbd>⌘K ⌘1</kbd> | <kbd>Ctrl+K Ctrl+1</kbd> | `editor` |
| `editor.fold_level_2` | Fold Level 2 | <kbd>⌘K ⌘2</kbd> | <kbd>Ctrl+K Ctrl+2</kbd> | `editor` |
| `editor.fold_level_3` | Fold Level 3 | <kbd>⌘K ⌘3</kbd> | <kbd>Ctrl+K Ctrl+3</kbd> | `editor` |
| `editor.fold_level_4` | Fold Level 4 | <kbd>⌘K ⌘4</kbd> | <kbd>Ctrl+K Ctrl+4</kbd> | `editor` |
| `editor.fold_level_5` | Fold Level 5 | <kbd>⌘K ⌘5</kbd> | <kbd>Ctrl+K Ctrl+5</kbd> | `editor` |
| `editor.fold_level_6` | Fold Level 6 | <kbd>⌘K ⌘6</kbd> | <kbd>Ctrl+K Ctrl+6</kbd> | `editor` |
| `editor.fold_level_7` | Fold Level 7 | <kbd>⌘K ⌘7</kbd> | <kbd>Ctrl+K Ctrl+7</kbd> | `editor` |
| `editor.fold_recursively` | Fold Recursively | <kbd>⌘K ⌘&#91;</kbd> | <kbd>Ctrl+K Ctrl+&#91;</kbd> | `editor` |
| `editor.go_to_bracket` | Go to Bracket | <kbd>⇧⌘&#92;</kbd> | <kbd>Ctrl+Shift+&#92;</kbd> | `editor` |
| `editor.go_to_last_edit_location` | Go to Last Edit Location | <kbd>⌘K ⌘Q</kbd> | <kbd>Ctrl+K Ctrl+Q</kbd> | `editor` |
| `editor.indent` | Indent | <kbd>⌘&#93;</kbd> | <kbd>Ctrl+&#93;</kbd> | `editor` |
| `editor.insert_line_above` | Insert Line Above | <kbd>⇧⌘↩</kbd> | <kbd>Ctrl+Shift+Enter</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.insert_line_below` | Insert Line Below | <kbd>⌘↩</kbd> | <kbd>Ctrl+Enter</kbd> | `editor && !git_panel && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.join_lines` | Join Lines | <kbd>⌃J</kbd> | — | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.move_document_end` | Move to Document End | <kbd>⌘↓</kbd> | <kbd>Ctrl+End</kbd> | `editor` |
| `editor.move_document_start` | Move to Document Start | <kbd>⌘↑</kbd> | <kbd>Ctrl+Home</kbd> | `editor` |
| `editor.move_down` | Move Down | <kbd>↓</kbd> | <kbd>Down</kbd> | `editor` |
| `editor.move_last_selection_to_next_find_match` | Move Last Selection to Next Find Match | <kbd>⌘K ⌘D</kbd> | <kbd>Ctrl+K Ctrl+D</kbd> | `editor && !vim.enabled` |
| `editor.move_left` | Move Left | <kbd>←</kbd> | <kbd>Left</kbd> | `editor` |
| `editor.move_line_end` | Move to Line End | <kbd>⌘→</kbd>, <kbd>↘</kbd> | <kbd>End</kbd> | `editor` |
| `editor.move_line_start` | Move to Line Start | <kbd>⌘←</kbd>, <kbd>↖</kbd> | <kbd>Home</kbd> | `editor` |
| `editor.move_right` | Move Right | <kbd>→</kbd> | <kbd>Right</kbd> | `editor` |
| `editor.move_rows_down` | Move Rows Down | <kbd>⌥↓</kbd> | <kbd>Alt+Down</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.move_rows_up` | Move Rows Up | <kbd>⌥↑</kbd> | <kbd>Alt+Up</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.move_subword_left` | Move Subword Left | <kbd>⌃⌥←</kbd> | — | `editor` |
| `editor.move_subword_right` | Move Subword Right | <kbd>⌃⌥→</kbd> | — | `editor` |
| `editor.move_up` | Move Up | <kbd>↑</kbd> | <kbd>Up</kbd> | `editor` |
| `editor.move_word_left` | Move Word Left | <kbd>⌥←</kbd> | <kbd>Ctrl+Left</kbd> | `editor` |
| `editor.move_word_right` | Move Word Right | <kbd>⌥→</kbd> | <kbd>Ctrl+Right</kbd> | `editor` |
| `editor.newline` | Insert Newline | <kbd>⇧↩</kbd> | <kbd>Shift+Enter</kbd> | `chat_composer && editor` |
| `editor.newline` | Insert Newline | <kbd>↩</kbd> | <kbd>Enter</kbd> | `editor && !chat_composer` |
| `editor.outdent` | Outdent | <kbd>⌘&#91;</kbd> | — | `editor` |
| `editor.outdent` | Outdent | — | <kbd>Ctrl+&#91;</kbd> | `editor && !vim.enabled` |
| `editor.outdent` | Outdent | <kbd>⇧⇥</kbd> | <kbd>Shift+Tab</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.page_down` | Page Down | <kbd>⇟</kbd> | <kbd>PageDown</kbd> | `editor` |
| `editor.page_up` | Page Up | <kbd>⇞</kbd> | <kbd>PageUp</kbd> | `editor` |
| `editor.paste` | Paste | <kbd>⌘V</kbd> | <kbd>Ctrl+V</kbd> | `editor` |
| `editor.peek_definition` | Peek Definition | <kbd>⌥F12</kbd> | <kbd>Alt+F12</kbd> | `editor` |
| `editor.peek_references` | Peek References | <kbd>⇧F12</kbd> | <kbd>Shift+F12</kbd> | `editor` |
| `editor.redo` | Redo | <kbd>⇧⌘Z</kbd> | <kbd>Ctrl+Shift+Z</kbd>, <kbd>Ctrl+Y</kbd> | `editor` |
| `editor.save` | Save | <kbd>⌘S</kbd> | <kbd>Ctrl+S</kbd> | `editor` |
| `editor.select_all` | Select All | <kbd>⌘A</kbd> | <kbd>Ctrl+A</kbd> | `editor` |
| `editor.select_all_occurrences` | Select All Occurrences | <kbd>⇧⌘L</kbd> | <kbd>Ctrl+Shift+L</kbd> | `editor && !vim.enabled` |
| `editor.select_document_end` | Select to Document End | <kbd>⇧⌘↓</kbd> | <kbd>Ctrl+Shift+End</kbd> | `editor` |
| `editor.select_document_start` | Select to Document Start | <kbd>⇧⌘↑</kbd> | <kbd>Ctrl+Shift+Home</kbd> | `editor` |
| `editor.select_down` | Select Down | <kbd>⇧↓</kbd> | <kbd>Shift+Down</kbd> | `editor` |
| `editor.select_left` | Select Left | <kbd>⇧←</kbd> | <kbd>Shift+Left</kbd> | `editor` |
| `editor.select_line_end` | Select to Line End | <kbd>⇧⌘→</kbd>, <kbd>⇧↘</kbd> | <kbd>Shift+End</kbd> | `editor` |
| `editor.select_line_start` | Select to Line Start | <kbd>⇧⌘←</kbd>, <kbd>⇧↖</kbd> | <kbd>Shift+Home</kbd> | `editor` |
| `editor.select_next_occurrence` | Select Next Occurrence | <kbd>⌘D</kbd> | <kbd>Ctrl+D</kbd> | `editor && !vim.enabled` |
| `editor.select_page_down` | Select Page Down | <kbd>⇧⇟</kbd> | <kbd>Shift+PageDown</kbd> | `editor` |
| `editor.select_page_up` | Select Page Up | <kbd>⇧⇞</kbd> | <kbd>Shift+PageUp</kbd> | `editor` |
| `editor.select_right` | Select Right | <kbd>⇧→</kbd> | <kbd>Shift+Right</kbd> | `editor` |
| `editor.select_subword_left` | Select Subword Left | <kbd>⌃⌥⇧←</kbd> | — | `editor` |
| `editor.select_subword_right` | Select Subword Right | <kbd>⌃⌥⇧→</kbd> | — | `editor` |
| `editor.select_up` | Select Up | <kbd>⇧↑</kbd> | <kbd>Shift+Up</kbd> | `editor` |
| `editor.select_word_left` | Select Word Left | <kbd>⌥⇧←</kbd> | <kbd>Ctrl+Shift+Left</kbd> | `editor` |
| `editor.select_word_right` | Select Word Right | <kbd>⌥⇧→</kbd> | <kbd>Ctrl+Shift+Right</kbd> | `editor` |
| `editor.show_hover` | Show Hover | <kbd>⌘K ⌘I</kbd> | <kbd>Ctrl+K Ctrl+I</kbd> | `editor` |
| `editor.show_hover` | Show Hover | <kbd>⇧K</kbd> | <kbd>Shift+K</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `editor.show_signature_help` | Show Signature Help | <kbd>⇧⌘Space</kbd> | <kbd>Ctrl+Shift+Space</kbd> | `editor` |
| `editor.shrink_selection` | Shrink Selection | <kbd>⌃⇧⌘←</kbd> | <kbd>Alt+Shift+Left</kbd> | `editor && !vim.enabled` |
| `editor.single_selection` | Single Selection | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `editor && !vim.enabled && !chat_composer` |
| `editor.split_selection_into_lines` | Split Selection into Lines | <kbd>⌥⇧I</kbd> | <kbd>Alt+Shift+I</kbd> | `editor && !vim.enabled` |
| `editor.tab` | Insert Tab | <kbd>⇥</kbd> | <kbd>Tab</kbd> | `editor` |
| `editor.toggle_block_comment` | Toggle Block Comment | <kbd>⌥⇧A</kbd> | <kbd>Alt+Shift+A</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `editor.toggle_comment` | Toggle Line Comment | <kbd>⌘/</kbd> | <kbd>Ctrl+/</kbd> | `editor` |
| `editor.toggle_soft_wrap` | Toggle Soft Wrap | <kbd>⌥Z</kbd> | <kbd>Alt+Z</kbd> | `editor` |
| `editor.transpose_letters` | Transpose Letters | <kbd>⌃T</kbd> | — | `editor && !vim.enabled` |
| `editor.trigger_completion` | Trigger Completion | <kbd>⌃Space</kbd> | <kbd>Ctrl+Space</kbd> | `editor` |
| `editor.trim_trailing_whitespace` | Trim Trailing Whitespace | <kbd>⌘K ⌘X</kbd> | <kbd>Ctrl+K Ctrl+X</kbd> | `editor` |
| `editor.undo` | Undo | <kbd>⌘Z</kbd> | <kbd>Ctrl+Z</kbd> | `editor` |
| `editor.unfold` | Unfold | <kbd>⌥⌘&#93;</kbd> | <kbd>Ctrl+}</kbd> | `editor` |
| `editor.unfold_all` | Unfold All | <kbd>⌘K ⌘J</kbd> | <kbd>Ctrl+K Ctrl+J</kbd> | `editor` |
| `editor.unfold_recursively` | Unfold Recursively | <kbd>⌘K ⌘&#93;</kbd> | <kbd>Ctrl+K Ctrl+&#93;</kbd> | `editor` |

## `find`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `find.close` | Close Find | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `find_bar` |
| `find.next` | Find Next | <kbd>⌘G</kbd> | <kbd>F3</kbd> | `editor \|\| find_bar` |
| `find.next` | Find Next | <kbd>↩</kbd> | <kbd>Enter</kbd> | `find_bar && text_input` |
| `find.previous` | Find Previous | <kbd>⇧⌘G</kbd> | <kbd>Shift+F3</kbd> | `editor \|\| find_bar` |
| `find.previous` | Find Previous | <kbd>⇧↩</kbd> | <kbd>Shift+Enter</kbd> | `find_bar && text_input` |
| `find.replace_all` | Replace All | <kbd>⌥⌘↩</kbd> | <kbd>Ctrl+Alt+Enter</kbd> | `find_bar` |
| `find.replace_next` | Replace | <kbd>⌘&#33;</kbd> | <kbd>Ctrl+&#33;</kbd> | `find_bar` |
| `find.replace_next` | Replace | <kbd>↩</kbd> | <kbd>Enter</kbd> | `find_bar && replace_input && text_input` |
| `find.show` | Find | <kbd>⌘F</kbd> | <kbd>Ctrl+F</kbd> | `editor` |
| `find.show_replace` | Replace… | <kbd>⌥⌘F</kbd> | — | `workbench` |
| `find.show_replace` | Replace… | — | <kbd>Ctrl+Alt+F</kbd> | `workbench && !(editor && vim.enabled)` |
| `find.toggle_case` | Match Case | <kbd>⌥⌘C</kbd> | <kbd>Alt+C</kbd> | `find_bar` |
| `find.toggle_in_selection` | Find in Selection | <kbd>⌥⌘L</kbd> | <kbd>Alt+L</kbd> | `find_bar` |
| `find.toggle_preserve_case` | Toggle Preserve Case | <kbd>⌥⌘P</kbd> | <kbd>Alt+P</kbd> | `find_bar` |
| `find.toggle_regex` | Use Regular Expression | <kbd>⌥⌘R</kbd> | <kbd>Alt+R</kbd> | `find_bar` |
| `find.toggle_whole_word` | Match Whole Word | <kbd>⌥⌘W</kbd> | <kbd>Alt+W</kbd> | `find_bar` |

## `ghost_text`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `ghost_text.accept` | Accept Suggestion | <kbd>⇥</kbd> | <kbd>Tab</kbd> | `ghost_text` |
| `ghost_text.accept_word` | Accept Next Word of Suggestion | <kbd>⌘→</kbd> | <kbd>Ctrl+Right</kbd> | `ghost_text` |

## `git`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `git.commit_box.commit` | Commit from the Message Box | <kbd>⌘↩</kbd> | — | `git_panel` |
| `git.commit_box.commit` | Commit from the Message Box | — | <kbd>Ctrl+Enter</kbd> | `git_panel && editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `git.next_change` | Go to Next Change | <kbd>⌥F3</kbd> | <kbd>Alt+F3</kbd> | `editor` |
| `git.previous_change` | Go to Previous Change | <kbd>⌥⇧F3</kbd> | <kbd>Alt+Shift+F3</kbd> | `editor` |
| `git.revert_hunk` | Git: Revert Change | <kbd>⌥⌘Z</kbd> | — | `editor` |
| `git.revert_hunk` | Git: Revert Change | — | <kbd>Ctrl+Alt+Z</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `git.show_changes` | Show Changes | <kbd>⌃⇧G</kbd> | <kbd>Ctrl+Shift+G</kbd> | everywhere |
| `git.stage_hunk` | Git: Stage Change | <kbd>⌘Y</kbd> | — | `editor && !proposal` |
| `git.stage_hunk` | Git: Stage Change | — | <kbd>Ctrl+Alt+Y</kbd> | `editor && !proposal && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `git.toggle_blame_annotations` | Git: Toggle Blame Annotations | <kbd>⌥⌘G B</kbd> | — | `editor` |
| `git.toggle_blame_annotations` | Git: Toggle Blame Annotations | — | <kbd>Ctrl+Alt+G B</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |

## `go_to_line`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `go_to_line.toggle` | Go to Line/Column… | <kbd>⌃G</kbd> | <kbd>Ctrl+G</kbd> | `workbench` |

## `hex`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `hex.copy` | Copy Bytes | <kbd>⌘C</kbd> | <kbd>Ctrl+C</kbd> | `HexViewer` |
| `hex.find` | Find Bytes | <kbd>⌘F</kbd> | <kbd>Ctrl+F</kbd> | `HexViewer` |
| `hex.find_next` | Find Next Bytes | <kbd>⌘G</kbd> | <kbd>F3</kbd> | `HexViewer` |
| `hex.find_previous` | Find Previous Bytes | <kbd>⇧⌘G</kbd> | <kbd>Shift+F3</kbd> | `HexViewer` |
| `hex.go_to_offset` | Go to Offset… | <kbd>⌃G</kbd> | <kbd>Ctrl+G</kbd> | `HexViewer` |

## `image`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `image.zoom_actual` | Actual Size | <kbd>⌘1</kbd> | <kbd>Ctrl+1</kbd> | `ImageViewer` |
| `image.zoom_fit` | Fit to Window | <kbd>⌘0</kbd> | <kbd>Ctrl+0</kbd> | `ImageViewer` |
| `image.zoom_in` | Zoom In | <kbd>⌘=</kbd>, <kbd>⌘+</kbd> | <kbd>Ctrl+=</kbd>, <kbd>Ctrl++</kbd> | `ImageViewer` |
| `image.zoom_out` | Zoom Out | <kbd>⌘-</kbd> | <kbd>Ctrl+-</kbd> | `ImageViewer` |

## `inline_edit`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `inline_edit.cancel` | Cancel Inline Edit | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `inline_edit` |
| `inline_edit.submit` | Send Inline Edit | <kbd>↩</kbd> | <kbd>Enter</kbd> | `inline_edit && text_input` |

## `language`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `language.select` | Change Language Mode… | <kbd>⌘K M</kbd> | <kbd>Ctrl+K M</kbd> | `workbench` |

## `lsp`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `lsp.code_actions` | Code Actions… | <kbd>⌘.</kbd> | <kbd>Ctrl+.</kbd> | `editor` |
| `lsp.code_actions` | Code Actions… | <kbd>G R A</kbd> | <kbd>G R A</kbd> | `editor && vim.mode == normal && !vim.pending && !vim.replace_with_register` |
| `lsp.code_actions` | Code Actions… | <kbd>G R A</kbd> | <kbd>G R A</kbd> | `editor && vim.visual && !vim.replace_with_register` |
| `lsp.format_document` | Format Document | <kbd>⌥⇧F</kbd> | <kbd>Ctrl+Shift+I</kbd> | `editor && (!vim.enabled \|\| vim.mode == insert \|\| vim.mode == replace)` |
| `lsp.format_selection` | Format Selection | <kbd>⌘K ⌘F</kbd> | <kbd>Ctrl+K Ctrl+F</kbd> | `editor` |
| `lsp.organize_imports` | Organize Imports | <kbd>⌥⇧O</kbd> | <kbd>Alt+Shift+O</kbd> | `editor` |
| `lsp.refactor` | Refactor… | <kbd>⌃⇧R</kbd> | <kbd>Ctrl+Shift+R</kbd> | `editor` |
| `lsp.rename` | Rename Symbol… | <kbd>F2</kbd> | <kbd>F2</kbd> | `editor` |
| `lsp.rename` | Rename Symbol… | <kbd>G R N</kbd> | <kbd>G R N</kbd> | `editor && vim.mode == normal && !vim.pending && !vim.replace_with_register` |

## `markdown`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `markdown.show_preview` | Markdown: Open Preview | <kbd>⇧⌘V</kbd> | <kbd>Ctrl+Shift+V</kbd> | `editor` |
| `markdown.show_preview_to_side` | Markdown: Open Preview to the Side | <kbd>⌘K V</kbd> | <kbd>Ctrl+K V</kbd> | `editor` |

## `menu_bar`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `menu_bar.open` | Open Menu Bar | — | <kbd>F10</kbd> | `workbench` |

## `nav`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `nav.back` | Go Back | <kbd>⌃-</kbd> | <kbd>Ctrl+Alt+-</kbd> | `workbench && !terminal` |
| `nav.document_symbols` | Go to Symbol in File… | <kbd>⇧⌘O</kbd> | <kbd>Ctrl+Shift+O</kbd> | `workbench && !terminal` |
| `nav.find_references` | Find All References | <kbd>G R R</kbd> | <kbd>G R R</kbd> | `editor && vim.mode == normal && !vim.pending && !vim.replace_with_register` |
| `nav.find_references` | Find All References | <kbd>⌥⇧F12</kbd> | <kbd>Alt+Shift+F12</kbd> | `workbench && !terminal` |
| `nav.forward` | Go Forward | <kbd>⌃⇧-</kbd> | <kbd>Ctrl+&#95;</kbd> | `workbench && !terminal` |
| `nav.goto_declaration` | Go to Declaration | <kbd>G ⇧D</kbd> | <kbd>G Shift+D</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `nav.goto_definition` | Go to Definition | <kbd>G D</kbd>, <kbd>⌃&#93;</kbd> | <kbd>G D</kbd>, <kbd>Ctrl+&#93;</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `nav.goto_definition` | Go to Definition | <kbd>⌘Click</kbd> | <kbd>Ctrl+Click</kbd> | `workbench` |
| `nav.goto_definition` | Go to Definition | <kbd>F12</kbd> | <kbd>F12</kbd> | `workbench && !terminal` |
| `nav.goto_implementation` | Go to Implementations | <kbd>G R I</kbd> | <kbd>G R I</kbd> | `editor && vim.mode == normal && !vim.pending && !vim.replace_with_register` |
| `nav.goto_implementation` | Go to Implementations | <kbd>⌘F12</kbd> | <kbd>Ctrl+F12</kbd> | `workbench && !terminal` |
| `nav.goto_type_definition` | Go to Type Definition | <kbd>G R T</kbd> | <kbd>G R T</kbd> | `editor && vim.mode == normal && !vim.pending && !vim.replace_with_register` |
| `nav.show_call_hierarchy` | Show Call Hierarchy | <kbd>⌥⇧H</kbd> | <kbd>Alt+Shift+H</kbd> | `workbench && !terminal` |
| `nav.workspace_symbols` | Go to Symbol in Workspace… | <kbd>⌘T</kbd> | <kbd>Ctrl+T</kbd> | `workbench` |

## `outline`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `outline.open_selected` | Go to Selected Symbol | <kbd>↩</kbd> | <kbd>Enter</kbd> | `outline_panel` |
| `outline.open_selected` | Go to Selected Symbol | <kbd>O</kbd> | <kbd>O</kbd> | `outline_panel && vim` |
| `outline.select_first` | Select First Symbol | <kbd>G G</kbd> | <kbd>G G</kbd> | `outline_panel && vim` |
| `outline.select_last` | Select Last Symbol | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> | `outline_panel && vim` |
| `outline.select_next` | Select Next Symbol | <kbd>↓</kbd> | <kbd>Down</kbd> | `outline_panel` |
| `outline.select_next` | Select Next Symbol | <kbd>J</kbd> | <kbd>J</kbd> | `outline_panel && vim` |
| `outline.select_previous` | Select Previous Symbol | <kbd>↑</kbd> | <kbd>Up</kbd> | `outline_panel` |
| `outline.select_previous` | Select Previous Symbol | <kbd>K</kbd> | <kbd>K</kbd> | `outline_panel && vim` |
| `outline.show` | Show Outline | <kbd>G ⇧O</kbd> | <kbd>G Shift+O</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `outline.show` | Show Outline | <kbd>⌥⌘O</kbd> | <kbd>Ctrl+Alt+O</kbd> | `workbench` |

## `pane`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `pane.close` | Close Pane | <kbd>⌃W C</kbd>, <kbd>⌃W Q</kbd>, <kbd>⌃W ⌃Q</kbd> | <kbd>Ctrl+W C</kbd>, <kbd>Ctrl+W Q</kbd>, <kbd>Ctrl+W Ctrl+Q</kbd> | `editor && vim.mode == normal` |
| `pane.close` | Close Pane | <kbd>⌘K ⌘W</kbd> | <kbd>Ctrl+K Ctrl+W</kbd> | `workbench` |
| `pane.equalize` | Equalize Panes | <kbd>⌃W =</kbd> | <kbd>Ctrl+W =</kbd> | `editor && vim.mode == normal` |
| `pane.exchange` | Exchange with Next Pane | <kbd>⌃W X</kbd>, <kbd>⌃W ⌃X</kbd> | <kbd>Ctrl+W X</kbd>, <kbd>Ctrl+W Ctrl+X</kbd> | `editor && vim.mode == normal` |
| `pane.focus_down` | Focus Pane Below | <kbd>⌃W J</kbd>, <kbd>⌃W ⌃J</kbd>, <kbd>⌃W ↓</kbd> | <kbd>Ctrl+W J</kbd>, <kbd>Ctrl+W Ctrl+J</kbd>, <kbd>Ctrl+W Down</kbd> | `editor && vim.mode == normal` |
| `pane.focus_down` | Focus Pane Below | <kbd>⌘K ⌘↓</kbd> | <kbd>Ctrl+K Ctrl+Down</kbd> | `workbench` |
| `pane.focus_group1` | Focus First Editor Group | <kbd>⌘1</kbd> | <kbd>Ctrl+1</kbd> | `workbench` |
| `pane.focus_group2` | Focus Second Editor Group | <kbd>⌘2</kbd> | <kbd>Ctrl+2</kbd> | `workbench` |
| `pane.focus_group3` | Focus Third Editor Group | <kbd>⌘3</kbd> | <kbd>Ctrl+3</kbd> | `workbench` |
| `pane.focus_group4` | Focus Fourth Editor Group | <kbd>⌘4</kbd> | <kbd>Ctrl+4</kbd> | `workbench` |
| `pane.focus_left` | Focus Left Pane | <kbd>⌃W H</kbd>, <kbd>⌃W ⌃H</kbd>, <kbd>⌃W ←</kbd> | <kbd>Ctrl+W H</kbd>, <kbd>Ctrl+W Ctrl+H</kbd>, <kbd>Ctrl+W Left</kbd> | `editor && vim.mode == normal` |
| `pane.focus_left` | Focus Left Pane | <kbd>⌘K ⌘←</kbd> | <kbd>Ctrl+K Ctrl+Left</kbd> | `workbench` |
| `pane.focus_next` | Focus Next Pane | <kbd>⌃W W</kbd>, <kbd>⌃W ⌃W</kbd> | <kbd>Ctrl+W W</kbd>, <kbd>Ctrl+W Ctrl+W</kbd> | `editor && vim.mode == normal` |
| `pane.focus_previous` | Focus Previous Pane | <kbd>⌃W ⇧W</kbd> | <kbd>Ctrl+W Shift+W</kbd> | `editor && vim.mode == normal` |
| `pane.focus_right` | Focus Right Pane | <kbd>⌃W L</kbd>, <kbd>⌃W ⌃L</kbd>, <kbd>⌃W →</kbd> | <kbd>Ctrl+W L</kbd>, <kbd>Ctrl+W Ctrl+L</kbd>, <kbd>Ctrl+W Right</kbd> | `editor && vim.mode == normal` |
| `pane.focus_right` | Focus Right Pane | <kbd>⌘K ⌘→</kbd> | <kbd>Ctrl+K Ctrl+Right</kbd> | `workbench` |
| `pane.focus_up` | Focus Pane Above | <kbd>⌃W K</kbd>, <kbd>⌃W ⌃K</kbd>, <kbd>⌃W ↑</kbd> | <kbd>Ctrl+W K</kbd>, <kbd>Ctrl+W Ctrl+K</kbd>, <kbd>Ctrl+W Up</kbd> | `editor && vim.mode == normal` |
| `pane.focus_up` | Focus Pane Above | <kbd>⌘K ⌘↑</kbd> | <kbd>Ctrl+K Ctrl+Up</kbd> | `workbench` |
| `pane.grow_height` | Grow Pane Height | <kbd>⌃W +</kbd> | <kbd>Ctrl+W +</kbd> | `editor && vim.mode == normal` |
| `pane.grow_width` | Grow Pane Width | <kbd>⌃W &#62;</kbd> | <kbd>Ctrl+W &#62;</kbd> | `editor && vim.mode == normal` |
| `pane.move_tab_down` | Move Tab Down | <kbd>⌃W ⇧J</kbd> | <kbd>Ctrl+W Shift+J</kbd> | `editor && vim.mode == normal` |
| `pane.move_tab_down` | Move Tab Down | <kbd>⌃⌘↓</kbd> | <kbd>Ctrl+Alt+Down</kbd> | `workbench` |
| `pane.move_tab_left` | Move Tab Left | <kbd>⌃W ⇧H</kbd> | <kbd>Ctrl+W Shift+H</kbd> | `editor && vim.mode == normal` |
| `pane.move_tab_left` | Move Tab Left | <kbd>⌃⌘←</kbd> | <kbd>Ctrl+Alt+Left</kbd> | `workbench` |
| `pane.move_tab_right` | Move Tab Right | <kbd>⌃W ⇧L</kbd> | <kbd>Ctrl+W Shift+L</kbd> | `editor && vim.mode == normal` |
| `pane.move_tab_right` | Move Tab Right | <kbd>⌃⌘→</kbd> | <kbd>Ctrl+Alt+Right</kbd> | `workbench` |
| `pane.move_tab_up` | Move Tab Up | <kbd>⌃W ⇧K</kbd> | <kbd>Ctrl+W Shift+K</kbd> | `editor && vim.mode == normal` |
| `pane.move_tab_up` | Move Tab Up | <kbd>⌃⌘↑</kbd> | <kbd>Ctrl+Alt+Up</kbd> | `workbench` |
| `pane.shrink_height` | Shrink Pane Height | <kbd>⌃W -</kbd> | <kbd>Ctrl+W -</kbd> | `editor && vim.mode == normal` |
| `pane.shrink_width` | Shrink Pane Width | <kbd>⌃W &#60;</kbd> | <kbd>Ctrl+W &#60;</kbd> | `editor && vim.mode == normal` |
| `pane.split_down` | Split Down | <kbd>⌃W S</kbd>, <kbd>⌃W ⇧S</kbd>, <kbd>⌃W ⌃S</kbd> | <kbd>Ctrl+W S</kbd>, <kbd>Ctrl+W Shift+S</kbd>, <kbd>Ctrl+W Ctrl+S</kbd> | `editor && vim.mode == normal` |
| `pane.split_down` | Split Down | <kbd>⌘K ⌘&#92;</kbd> | <kbd>Ctrl+K Ctrl+&#92;</kbd> | `workbench` |
| `pane.split_right` | Split Right | <kbd>⌃W V</kbd>, <kbd>⌃W ⌃V</kbd> | <kbd>Ctrl+W V</kbd>, <kbd>Ctrl+W Ctrl+V</kbd> | `editor && vim.mode == normal` |
| `pane.split_right` | Split Right | <kbd>⌘&#92;</kbd> | <kbd>Ctrl+&#92;</kbd> | `workbench` |
| `pane.toggle_maximize` | Toggle Maximized Pane | <kbd>⌃W Z</kbd> | <kbd>Ctrl+W Z</kbd> | `editor && vim.mode == normal` |
| `pane.toggle_maximize` | Toggle Maximized Pane | <kbd>⌘K ⌘M</kbd> | <kbd>Ctrl+K Ctrl+M</kbd> | `workbench` |

## `peek`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `peek.close` | Close Peek | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `editor && peek_shown && !completion && vim.mode != insert && vim.mode != replace` |
| `peek.close` | Close Peek | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `peek` |
| `peek.close` | Close Peek | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `peek && editor` |
| `peek.open` | Open Peeked Definition | <kbd>↩</kbd> | <kbd>Enter</kbd> | `peek && editor` |

## `picker`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `picker.confirm` | Confirm | <kbd>↩</kbd> | <kbd>Enter</kbd> | `picker` |
| `picker.dismiss` | Dismiss | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `picker` |
| `picker.select_next` | Select Next | <kbd>↓</kbd>, <kbd>⌃N</kbd> | <kbd>Down</kbd> | `picker` |
| `picker.select_prev` | Select Previous | <kbd>↑</kbd>, <kbd>⌃P</kbd> | <kbd>Up</kbd> | `picker` |

## `problems`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `problems.apply_fix` | Apply Fix | <kbd>⌥↩</kbd> | <kbd>Alt+Enter</kbd> | `problems_panel` |
| `problems.open` | Open Problem | <kbd>↩</kbd> | <kbd>Enter</kbd> | `problems_panel` |
| `problems.open` | Open Problem | <kbd>O</kbd> | <kbd>O</kbd> | `problems_panel && vim` |
| `problems.select_first` | Select First Problem | <kbd>G G</kbd> | <kbd>G G</kbd> | `problems_panel && vim` |
| `problems.select_last` | Select Last Problem | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> | `problems_panel && vim` |
| `problems.select_next` | Select Next Problem | <kbd>↓</kbd> | <kbd>Down</kbd> | `problems_panel` |
| `problems.select_next` | Select Next Problem | <kbd>J</kbd> | <kbd>J</kbd> | `problems_panel && vim` |
| `problems.select_previous` | Select Previous Problem | <kbd>↑</kbd> | <kbd>Up</kbd> | `problems_panel` |
| `problems.select_previous` | Select Previous Problem | <kbd>K</kbd> | <kbd>K</kbd> | `problems_panel && vim` |
| `problems.show` | Show Problems | <kbd>⇧⌘M</kbd> | — | `workbench` |
| `problems.show` | Show Problems | — | <kbd>Ctrl+Shift+M</kbd> | `workbench && !(editor && vim.enabled)` |

## `project_panel`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `project_panel.clear_filter` | Clear Filter | <kbd>⇧F</kbd> | <kbd>Shift+F</kbd> | `project_panel && vim` |
| `project_panel.collapse_all` | Collapse All Folders | <kbd>⇧W</kbd> | <kbd>Shift+W</kbd> | `project_panel && vim` |
| `project_panel.collapse_or_parent` | Collapse or Select Parent | <kbd>←</kbd> | <kbd>Left</kbd> | `project_panel` |
| `project_panel.collapse_or_parent` | Collapse or Select Parent | <kbd>H</kbd> | <kbd>H</kbd> | `project_panel && vim` |
| `project_panel.copy` | Copy | <kbd>⌘C</kbd> | <kbd>Ctrl+C</kbd> | `project_panel` |
| `project_panel.copy` | Copy | <kbd>C</kbd> | <kbd>C</kbd> | `project_panel && vim` |
| `project_panel.copy_path` | Copy Path | <kbd>G Y</kbd> | <kbd>G Y</kbd> | `project_panel && vim` |
| `project_panel.copy_relative_path` | Copy Relative Path | <kbd>⇧Y</kbd> | <kbd>Shift+Y</kbd> | `project_panel && vim` |
| `project_panel.cut` | Cut | <kbd>⌘X</kbd> | <kbd>Ctrl+X</kbd> | `project_panel` |
| `project_panel.cut` | Cut | <kbd>X</kbd> | <kbd>X</kbd> | `project_panel && vim` |
| `project_panel.delete` | Move to Trash | <kbd>⌘⌫</kbd> | <kbd>Ctrl+Backspace</kbd> | `project_panel` |
| `project_panel.delete` | Move to Trash | <kbd>D</kbd>, <kbd>⇧D</kbd> | <kbd>D</kbd>, <kbd>Shift+D</kbd> | `project_panel && vim` |
| `project_panel.expand_all` | Expand All Folders | <kbd>⇧E</kbd> | <kbd>Shift+E</kbd> | `project_panel && vim` |
| `project_panel.expand_or_first_child` | Expand or Select First Child | <kbd>→</kbd> | <kbd>Right</kbd> | `project_panel` |
| `project_panel.filter` | Filter by Name… | <kbd>/</kbd> | <kbd>/</kbd> | `project_panel && vim` |
| `project_panel.new_file` | New File… | <kbd>⌥⌘N</kbd> | <kbd>Ctrl+Alt+N</kbd> | `project_panel` |
| `project_panel.new_file` | New File… | <kbd>A</kbd>, <kbd>%</kbd> | <kbd>A</kbd>, <kbd>%</kbd> | `project_panel && vim` |
| `project_panel.new_folder` | New Folder… | <kbd>⌥⇧⌘N</kbd> | <kbd>Ctrl+Alt+Shift+N</kbd> | `project_panel` |
| `project_panel.new_menu` | New… | <kbd>⌘N</kbd> | <kbd>Alt+N</kbd> | `project_panel` |
| `project_panel.open` | Open | <kbd>↩</kbd> | <kbd>Enter</kbd> | `project_panel` |
| `project_panel.open` | Open | <kbd>L</kbd> | <kbd>L</kbd> | `project_panel && vim` |
| `project_panel.open_to_side` | Open to the Side | <kbd>⌘↩</kbd> | <kbd>Ctrl+Enter</kbd> | `project_panel` |
| `project_panel.paste` | Paste | <kbd>⌘V</kbd> | <kbd>Ctrl+V</kbd> | `project_panel` |
| `project_panel.paste` | Paste | <kbd>P</kbd> | <kbd>P</kbd> | `project_panel && vim` |
| `project_panel.redo` | Redo File Operation | <kbd>⇧⌘Z</kbd> | — | `project_panel` |
| `project_panel.refresh` | Refresh | <kbd>G R</kbd> | <kbd>G R</kbd> | `project_panel && vim` |
| `project_panel.rename` | Rename… | <kbd>F2</kbd> | <kbd>F2</kbd> | `project_panel` |
| `project_panel.rename` | Rename… | <kbd>R</kbd>, <kbd>⇧R</kbd> | <kbd>R</kbd>, <kbd>Shift+R</kbd> | `project_panel && vim` |
| `project_panel.select_first` | Select First | <kbd>G G</kbd> | <kbd>G G</kbd> | `project_panel && vim` |
| `project_panel.select_last` | Select Last | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> | `project_panel && vim` |
| `project_panel.select_next` | Select Next | <kbd>↓</kbd> | <kbd>Down</kbd> | `project_panel` |
| `project_panel.select_next` | Select Next | <kbd>J</kbd> | <kbd>J</kbd> | `project_panel && vim` |
| `project_panel.select_prev` | Select Previous | <kbd>↑</kbd> | <kbd>Up</kbd> | `project_panel` |
| `project_panel.select_prev` | Select Previous | <kbd>K</kbd> | <kbd>K</kbd> | `project_panel && vim` |
| `project_panel.toggle` | Toggle Folder or Open File | <kbd>O</kbd> | <kbd>O</kbd> | `project_panel && vim` |
| `project_panel.toggle_ignored` | Toggle Ignored Files | <kbd>⇧H</kbd> | <kbd>Shift+H</kbd> | `project_panel && vim` |
| `project_panel.undo` | Undo File Operation | <kbd>⌘Z</kbd> | — | `project_panel` |

## `proposal`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `proposal.accept_all` | Accept All Proposed Changes | <kbd>⌘↩</kbd> | <kbd>Ctrl+Alt+Shift+Y</kbd> | `editor && proposal` |
| `proposal.accept_hunk` | Accept Proposed Change | <kbd>⌘Y</kbd> | <kbd>Ctrl+Alt+Y</kbd> | `editor && proposal` |
| `proposal.next_hunk` | Next Proposed Change | <kbd>⌥F5</kbd> | <kbd>Alt+F5</kbd> | `editor && proposal` |
| `proposal.previous_hunk` | Previous Proposed Change | <kbd>⌥⇧F5</kbd> | <kbd>Alt+Shift+F5</kbd> | `editor && proposal` |
| `proposal.reject_all` | Reject All Proposed Changes | <kbd>⌘⌫</kbd> | <kbd>Ctrl+Alt+Shift+N</kbd> | `editor && proposal` |
| `proposal.reject_hunk` | Reject Proposed Change | <kbd>⌘N</kbd> | <kbd>Ctrl+Alt+N</kbd> | `editor && proposal` |

## `quick_open`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `quick_open.toggle` | Go to File | <kbd>⌘P</kbd> | <kbd>Ctrl+P</kbd> | `workbench` |

## `quickfix`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `quickfix.next` | Quickfix: Next Item | <kbd>&#93; Q</kbd> | <kbd>&#93; Q</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `quickfix.previous` | Quickfix: Previous Item | <kbd>&#91; Q</kbd> | <kbd>&#91; Q</kbd> | `editor && vim.mode == normal && !vim.pending` |

## `references_panel`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `references_panel.open` | Open Reference | <kbd>↩</kbd> | <kbd>Enter</kbd> | `ReferencesPanel` |
| `references_panel.open` | Open Reference | <kbd>O</kbd> | <kbd>O</kbd> | `ReferencesPanel && vim` |
| `references_panel.select_first` | Select First Reference | <kbd>G G</kbd> | <kbd>G G</kbd> | `ReferencesPanel && vim` |
| `references_panel.select_last` | Select Last Reference | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> | `ReferencesPanel && vim` |
| `references_panel.select_next` | Select Next Reference | <kbd>↓</kbd> | <kbd>Down</kbd> | `ReferencesPanel` |
| `references_panel.select_next` | Select Next Reference | <kbd>J</kbd> | <kbd>J</kbd> | `ReferencesPanel && vim` |
| `references_panel.select_prev` | Select Previous Reference | <kbd>↑</kbd> | <kbd>Up</kbd> | `ReferencesPanel` |
| `references_panel.select_prev` | Select Previous Reference | <kbd>K</kbd> | <kbd>K</kbd> | `ReferencesPanel && vim` |

## `runs`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `runs.rerun_failed` | Runs: Rerun Failed Tests | <kbd>Space T F</kbd> | <kbd>Space T F</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `runs.show` | Show Runs | <kbd>Space R</kbd> | <kbd>Space R</kbd> | `editor && vim.mode == normal && !vim.pending` |

## `search`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `search.collapse_or_parent` | Collapse Result or Select Its File | <kbd>←</kbd> | <kbd>Left</kbd> | `search_results` |
| `search.expand_or_first_child` | Expand Result or Select Its First Match | <kbd>→</kbd> | <kbd>Right</kbd> | `search_results` |
| `search.open_selected` | Open Selected Result | <kbd>↩</kbd> | <kbd>Enter</kbd> | `search_results` |
| `search.open_selected` | Open Selected Result | <kbd>O</kbd> | <kbd>O</kbd> | `search_results && vim` |
| `search.replace_all` | Replace All in Files | <kbd>⌥⌘↩</kbd> | <kbd>Ctrl+Alt+Enter</kbd> | `search_panel` |
| `search.select_first` | Select First Result | <kbd>G G</kbd> | <kbd>G G</kbd> | `search_results && vim` |
| `search.select_last` | Select Last Result | <kbd>⇧G</kbd> | <kbd>Shift+G</kbd> | `search_results && vim` |
| `search.select_next` | Select Next Result | <kbd>↓</kbd> | <kbd>Down</kbd> | `search_results` |
| `search.select_next` | Select Next Result | <kbd>J</kbd> | <kbd>J</kbd> | `search_results && vim` |
| `search.select_previous` | Select Previous Result | <kbd>↑</kbd> | <kbd>Up</kbd> | `search_results` |
| `search.select_previous` | Select Previous Result | <kbd>K</kbd> | <kbd>K</kbd> | `search_results && vim` |
| `search.show` | Find in Files | <kbd>⇧⌘F</kbd> | <kbd>Ctrl+Shift+F</kbd> | `workbench` |
| `search.show_replace` | Replace in Files… | <kbd>⇧⌘H</kbd> | — | `workbench` |
| `search.show_replace` | Replace in Files… | — | <kbd>Ctrl+Shift+H</kbd> | `workbench && !(editor && vim.enabled)` |
| `search.toggle_case` | Toggle Match Case | <kbd>⌥⌘C</kbd> | <kbd>Alt+C</kbd> | `search_panel` |
| `search.toggle_details` | Toggle Search Details | <kbd>⇧⌘J</kbd> | <kbd>Ctrl+Shift+J</kbd> | `search_panel` |
| `search.toggle_preserve_case` | Toggle Preserve Case | <kbd>⌥⌘P</kbd> | <kbd>Alt+P</kbd> | `search_panel` |
| `search.toggle_regex` | Toggle Regex | <kbd>⌥⌘R</kbd> | <kbd>Alt+R</kbd> | `search_panel` |
| `search.toggle_whole_word` | Toggle Whole Word | <kbd>⌥⌘W</kbd> | <kbd>Alt+W</kbd> | `search_panel` |

## `spell`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `spell.suggest` | Spelling: Suggest Corrections… | <kbd>⌥⌘.</kbd> | <kbd>Ctrl+Alt+.</kbd> | `editor` |

## `tasks`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `tasks.run_build` | Tasks: Run Build Task | <kbd>⇧⌘B</kbd> | <kbd>Ctrl+Shift+B</kbd> | `workbench` |

## `terminal`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `terminal.find` | Terminal: Find | <kbd>⌘F</kbd> | <kbd>Ctrl+Shift+F</kbd> | `terminal` |
| `terminal.new` | New Terminal | <kbd>⌃⇧&#96;</kbd> | <kbd>Ctrl+&#126;</kbd> | `workbench` |
| `terminal.next` | Next Terminal | <kbd>⇧⌘&#93;</kbd> | <kbd>Ctrl+PageDown</kbd> | `terminal` |
| `terminal.previous` | Previous Terminal | <kbd>⇧⌘&#91;</kbd> | <kbd>Ctrl+PageUp</kbd> | `terminal` |
| `terminal.toggle` | Toggle Terminal | <kbd>⌃&#96;</kbd> | <kbd>Ctrl+&#96;</kbd> | `workbench` |

## `text_input`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `text_input.backspace` | Delete Backward | <kbd>⌫</kbd> | <kbd>Backspace</kbd> | `text_input` |
| `text_input.clear` | Clear | <kbd>⌘⌫</kbd> | <kbd>Ctrl+Shift+Backspace</kbd> | `text_input` |
| `text_input.delete_forward` | Delete Forward | <kbd>⌦</kbd> | <kbd>Delete</kbd> | `(settings_editor \|\| find_bar \|\| search_panel \|\| shortcuts_editor \|\| inline_edit \|\| vim_line \|\| git_log_panel \|\| debug_panel) && text_input` |
| `text_input.delete_word_left` | Delete Word Backward | <kbd>⌥⌫</kbd> | <kbd>Ctrl+Backspace</kbd> | `text_input` |
| `text_input.move_end` | Move to End | <kbd>↘</kbd>, <kbd>⌘→</kbd> | <kbd>End</kbd> | `(settings_editor \|\| find_bar \|\| search_panel \|\| shortcuts_editor \|\| inline_edit \|\| vim_line \|\| git_log_panel \|\| debug_panel) && text_input` |
| `text_input.move_home` | Move to Start | <kbd>↖</kbd>, <kbd>⌘←</kbd> | <kbd>Home</kbd> | `(settings_editor \|\| find_bar \|\| search_panel \|\| shortcuts_editor \|\| inline_edit \|\| vim_line \|\| git_log_panel \|\| debug_panel) && text_input` |
| `text_input.move_left` | Move Left | <kbd>←</kbd> | <kbd>Left</kbd> | `(settings_editor \|\| find_bar \|\| search_panel \|\| shortcuts_editor \|\| inline_edit \|\| vim_line \|\| git_log_panel \|\| debug_panel) && text_input` |
| `text_input.move_right` | Move Right | <kbd>→</kbd> | <kbd>Right</kbd> | `(settings_editor \|\| find_bar \|\| search_panel \|\| shortcuts_editor \|\| inline_edit \|\| vim_line \|\| git_log_panel \|\| debug_panel) && text_input` |
| `text_input.move_word_left` | Move Word Left | <kbd>⌥←</kbd> | <kbd>Ctrl+Left</kbd> | `(settings_editor \|\| find_bar \|\| search_panel \|\| shortcuts_editor \|\| inline_edit \|\| vim_line \|\| git_log_panel \|\| debug_panel) && text_input` |
| `text_input.move_word_right` | Move Word Right | <kbd>⌥→</kbd> | <kbd>Ctrl+Right</kbd> | `(settings_editor \|\| find_bar \|\| search_panel \|\| shortcuts_editor \|\| inline_edit \|\| vim_line \|\| git_log_panel \|\| debug_panel) && text_input` |
| `text_input.paste` | Paste | <kbd>⌘V</kbd> | <kbd>Ctrl+V</kbd> | `text_input` |
| `text_input.submit` | Submit | <kbd>↩</kbd> | <kbd>Enter</kbd> | `search_panel && text_input` |
| `text_input.submit` | Submit | <kbd>↩</kbd> | <kbd>Enter</kbd> | `vim_line && text_input` |

## `theme`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `theme.select` | Change Color Theme… | <kbd>⌘K ⌘T</kbd> | <kbd>Ctrl+K Ctrl+T</kbd> | `workbench` |

## `toast`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `toast.dismiss` | Dismiss Notification | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `toast` |
| `toast.dismiss_all` | Dismiss Notifications | <kbd>⌘K ⎋</kbd> | — | `workbench` |
| `toast.dismiss_all` | Dismiss Notifications | — | <kbd>Ctrl+K Esc</kbd> | `workbench && !(editor && vim.enabled)` |
| `toast.next_button` | Next Notification Button | <kbd>⇥</kbd> | <kbd>Tab</kbd> | `toast` |
| `toast.previous_button` | Previous Notification Button | <kbd>⇧⇥</kbd> | <kbd>Shift+Tab</kbd> | `toast` |

## `vim`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `vim.expand_selection` | Vim Expand Selection | <kbd>⌃⇧⌘→</kbd> | <kbd>Alt+Shift+Right</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `vim.expand_selection` | Vim Expand Selection | <kbd>⌃⇧⌘→</kbd> | <kbd>Alt+Shift+Right</kbd> | `editor && vim.visual && !vim.replace_with_register` |
| `vim.key` | Vim Key | — | <kbd>Ctrl+W</kbd>, <kbd>Ctrl+U</kbd>, <kbd>Ctrl+R</kbd>, <kbd>Ctrl+T</kbd>, <kbd>Ctrl+D</kbd>, <kbd>Ctrl+O</kbd>, <kbd>Ctrl+N</kbd>, <kbd>Ctrl+P</kbd>, <kbd>Ctrl+A</kbd> | `editor && (vim.mode == insert \|\| vim.mode == replace)` |
| `vim.key` | Vim Key | <kbd>↩</kbd>, <kbd>⇥</kbd>, <kbd>⌫</kbd>, <kbd>⇧⌫</kbd>, <kbd>⌦</kbd>, <kbd>⌥⌫</kbd> | <kbd>Enter</kbd>, <kbd>Tab</kbd>, <kbd>Backspace</kbd>, <kbd>Shift+Backspace</kbd>, <kbd>Delete</kbd>, <kbd>Ctrl+Backspace</kbd>, <kbd>Ctrl+V</kbd>, <kbd>Ctrl+Q</kbd>, <kbd>Ctrl+A</kbd>, <kbd>Ctrl+X</kbd>, <kbd>Ctrl+Y</kbd>, <kbd>Ctrl+B</kbd>, <kbd>Ctrl+F</kbd> | `editor && vim.mode == normal` |
| `vim.key` | Vim Key | <kbd>↩</kbd>, <kbd>⇥</kbd>, <kbd>⌫</kbd>, <kbd>⇧⌫</kbd>, <kbd>⌦</kbd>, <kbd>⌥⌫</kbd> | <kbd>Enter</kbd>, <kbd>Tab</kbd>, <kbd>Backspace</kbd>, <kbd>Shift+Backspace</kbd>, <kbd>Delete</kbd>, <kbd>Ctrl+Backspace</kbd>, <kbd>Ctrl+V</kbd>, <kbd>Ctrl+Q</kbd>, <kbd>Ctrl+A</kbd>, <kbd>Ctrl+X</kbd>, <kbd>Ctrl+Y</kbd>, <kbd>Ctrl+B</kbd>, <kbd>Ctrl+F</kbd> | `editor && vim.visual` |
| `vim.move` `{"motion":"down"}` | Vim Move | <kbd>↓</kbd> | <kbd>Down</kbd> | `editor && vim.mode == normal` |
| `vim.move` `{"motion":"down"}` | Vim Move | <kbd>↓</kbd> | <kbd>Down</kbd> | `editor && vim.visual` |
| `vim.move` `{"motion":"left"}` | Vim Move | <kbd>←</kbd> | <kbd>Left</kbd> | `editor && vim.mode == normal` |
| `vim.move` `{"motion":"left"}` | Vim Move | <kbd>←</kbd> | <kbd>Left</kbd> | `editor && vim.visual` |
| `vim.move` `{"motion":"right"}` | Vim Move | <kbd>→</kbd> | <kbd>Right</kbd> | `editor && vim.mode == normal` |
| `vim.move` `{"motion":"right"}` | Vim Move | <kbd>→</kbd> | <kbd>Right</kbd> | `editor && vim.visual` |
| `vim.move` `{"motion":"up"}` | Vim Move | <kbd>↑</kbd> | <kbd>Up</kbd> | `editor && vim.mode == normal` |
| `vim.move` `{"motion":"up"}` | Vim Move | <kbd>↑</kbd> | <kbd>Up</kbd> | `editor && vim.visual` |
| `vim.normal_mode` | Vim Normal Mode | <kbd>⎋</kbd>, <kbd>⌃&#91;</kbd> | <kbd>Esc</kbd>, <kbd>Ctrl+&#91;</kbd> | `editor && vim.mode == insert` |
| `vim.normal_mode` | Vim Normal Mode | <kbd>⎋</kbd>, <kbd>⌃&#91;</kbd> | <kbd>Esc</kbd>, <kbd>Ctrl+&#91;</kbd> | `editor && vim.mode == replace` |
| `vim.shrink_selection` | Vim Shrink Selection | <kbd>⌃⇧⌘←</kbd> | <kbd>Alt+Shift+Left</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `vim.shrink_selection` | Vim Shrink Selection | <kbd>⌃⇧⌘←</kbd> | <kbd>Alt+Shift+Left</kbd> | `editor && vim.visual && !vim.replace_with_register` |

## `vim_line`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `vim_line.cancel` | Cancel Command Line | <kbd>⎋</kbd> | <kbd>Esc</kbd> | `vim_line` |

## `window`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `window.close` | Close Window | <kbd>⇧⌘W</kbd> | <kbd>Ctrl+Shift+W</kbd> | everywhere |
| `window.minimize` | Minimize | <kbd>⌘M</kbd> | — | everywhere |
| `window.new` | New Window | <kbd>⇧⌘N</kbd> | <kbd>Ctrl+Shift+N</kbd> | `workbench` |
| `window.toggle_fullscreen` | Toggle Full Screen | <kbd>⌃⌘F</kbd> | <kbd>F11</kbd> | everywhere |

## `workbench`

| Command | Title | macOS | Linux and Windows | Context |
|---|---|---|---|---|
| `workbench.close_folder` | Close Folder | <kbd>⌘K F</kbd> | <kbd>Ctrl+K F</kbd> | `workbench` |
| `workbench.close_group_editors` | Close Editors in Group | <kbd>⌘K W</kbd> | <kbd>Ctrl+K W</kbd> | `workbench` |
| `workbench.close_saved` | Close Saved Editors | <kbd>⌘K U</kbd> | <kbd>Ctrl+K U</kbd> | `workbench` |
| `workbench.close_tab` | Close Tab | <kbd>⌘W</kbd> | <kbd>Ctrl+W</kbd> | `workbench` |
| `workbench.copy_active_file_path` | Copy Path of Active File | <kbd>⌘K P</kbd> | <kbd>Ctrl+K P</kbd> | `workbench` |
| `workbench.new_file` | New File | <kbd>⌘N</kbd> | <kbd>Ctrl+N</kbd> | `workbench` |
| `workbench.next_tab` | Next Tab | <kbd>&#93; B</kbd> | <kbd>&#93; B</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `workbench.next_tab` | Next Tab | <kbd>⇧⌘&#93;</kbd> | <kbd>Ctrl+PageDown</kbd> | `workbench` |
| `workbench.open_editor_at_index1` | Open Editor at Index 1 | <kbd>⌃1</kbd> | <kbd>Alt+1</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index2` | Open Editor at Index 2 | <kbd>⌃2</kbd> | <kbd>Alt+2</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index3` | Open Editor at Index 3 | <kbd>⌃3</kbd> | <kbd>Alt+3</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index4` | Open Editor at Index 4 | <kbd>⌃4</kbd> | <kbd>Alt+4</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index5` | Open Editor at Index 5 | <kbd>⌃5</kbd> | <kbd>Alt+5</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index6` | Open Editor at Index 6 | <kbd>⌃6</kbd> | <kbd>Alt+6</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index7` | Open Editor at Index 7 | <kbd>⌃7</kbd> | <kbd>Alt+7</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index8` | Open Editor at Index 8 | <kbd>⌃8</kbd> | <kbd>Alt+8</kbd> | `workbench && !terminal` |
| `workbench.open_editor_at_index9` | Open Editor at Index 9 | <kbd>⌃9</kbd> | <kbd>Alt+9</kbd> | `workbench && !terminal` |
| `workbench.open_folder` | Open Folder… | <kbd>⌘O</kbd> | <kbd>Ctrl+O</kbd> | `workbench` |
| `workbench.open_keyboard_shortcuts` | Open Keyboard Shortcuts | <kbd>⌘K ⌘S</kbd> | <kbd>Ctrl+K Ctrl+S</kbd> | `workbench` |
| `workbench.open_recent` | Open Recent… | <kbd>⌃R</kbd> | <kbd>Ctrl+R</kbd> | `workbench && !terminal && !vim_line && !(editor && vim.enabled)` |
| `workbench.open_settings_ui` | Open Settings (UI) | <kbd>⌘,</kbd> | <kbd>Ctrl+,</kbd> | `workbench` |
| `workbench.previous_tab` | Previous Tab | <kbd>&#91; B</kbd> | <kbd>&#91; B</kbd> | `editor && vim.mode == normal && !vim.pending` |
| `workbench.previous_tab` | Previous Tab | <kbd>⇧⌘&#91;</kbd> | <kbd>Ctrl+PageUp</kbd> | `workbench` |
| `workbench.reopen_closed_editor` | Reopen Closed Editor | <kbd>⇧⌘T</kbd> | <kbd>Ctrl+Shift+T</kbd> | `workbench && !terminal` |
| `workbench.reveal_active_file_in_os` | Reveal Active File in File Manager | <kbd>⌘K R</kbd> | <kbd>Ctrl+K R</kbd> | `workbench` |
| `workbench.save_all` | Save All | <kbd>⌥⌘S</kbd> | — | `workbench` |
| `workbench.save_as` | Save As… | <kbd>⇧⌘S</kbd> | <kbd>Ctrl+Shift+S</kbd> | `workbench` |
| `workbench.save_without_formatting` | Save without Formatting | <kbd>⌘K S</kbd> | — | `workbench` |
| `workbench.tab_switcher_next` | Switch Tab | <kbd>⌃⇥</kbd> | <kbd>Ctrl+Tab</kbd> | `workbench && !terminal` |
| `workbench.tab_switcher_previous` | Switch Tab Backward | <kbd>⌃⇧⇥</kbd> | <kbd>Ctrl+Shift+Tab</kbd> | `workbench && !terminal` |
| `workbench.toggle_zen_mode` | Toggle Zen Mode | <kbd>⌘K Z</kbd> | <kbd>Ctrl+K Z</kbd> | `workbench` |
| `workbench.toggle_zen_mode` | Toggle Zen Mode | <kbd>⎋ ⎋</kbd> | <kbd>Esc Esc</kbd> | `zen_mode` |
