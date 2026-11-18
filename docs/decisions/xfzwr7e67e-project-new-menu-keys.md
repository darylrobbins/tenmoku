# Project tree New menu keys (T681)

`project_panel.new_menu` opens the New menu under the dock header's `+`, from
the keyboard or the palette, wherever focus is: it shows the Project dock
first (a typed workbench command, like `project_panel.reveal_active`).

1. **`alt-n` off macOS, not JetBrains' `alt-insert`.** The keymap parser
   (`kiln-core` `keystroke.rs`) has no `insert` key, and adding one is outside
   this card. `alt-n` is free in the tree's context; `ctrl-n` stays New File
   in the editor. On macOS it is `cmd-n` in the tree only (a new file elsewhere).
2. **New File and New Folder get `cmd-alt-n` / `cmd-alt-shift-n`** (`ctrl-alt-n` /
   `ctrl-alt-shift-n` elsewhere), in the tree only, so the New menu shows keys
   for File and Folder without vim. `ctrl-alt-n` is also the proposal's reject
   key, but in the editor's context, not the tree's.
3. **The anchor is the `+` as last drawn**, recorded by the dock
   (`kiln_shell::dock::drawn_action_bounds`). With the dock hidden the menu
   opens two frames after it is shown, once the header has drawn.
