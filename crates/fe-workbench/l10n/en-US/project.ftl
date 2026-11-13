# fe-workbench's project tree and Structure, the outline under it (i18n
# design, T1541): `workbench-project-…`. Quotes are typographic; “…” around
# a name is part of the message.

## The project tree (project_panel.rs)

# The sidebar panel's name: the dock's header, its rail button's tooltip.
workbench-project-title = Project
# A toast when the project's folder can't be opened. $path is the folder.
workbench-project-cant-open-folder = Can't open folder { $path }
# In place of the tree. $path is the folder, $error why it failed.
workbench-project-cannot-open = Cannot open { $path }: { $error }
workbench-project-no-folder = No folder open
# What a screen reader calls the tree of files and folders.
workbench-project-tree = Files
# A row's git status, said after its name, as its letter (or a folder's
# dot, for the changes beneath it) shows it.
workbench-project-status-added = added
workbench-project-status-modified = modified
workbench-project-status-deleted = deleted
workbench-project-status-untracked = untracked
workbench-project-status-conflicted = conflicted
# Said to a screen reader as shift-click or shift-arrows select more rows,
# or cmd-click (ctrl-click) one more or one fewer. $count is how many rows
# are selected.
workbench-project-selected-count = { $count ->
    [one] { $count } item selected
   *[other] { $count } items selected
}

## Filtering the tree by name (project_panel_filter.rs)

# The row in place of the tree when no listed name matches.
workbench-project-filter-no-matches = No matches in listed folders
workbench-project-filter-placeholder = Filter by name
# Above the rows while the filter is set. $query is what it matches.
workbench-project-filter-set = Filter: { $query }
# The same, when the selected entry it hides is $name.
workbench-project-filter-set-hiding = Filter: { $query } (hides “{ $name }”)

## Expanding every folder (project_panel_nav.rs)

# Under the rows when Expand All stopped at its cap. $count is the cap.
workbench-project-expand-all-capped = { $count ->
    [one] Expanded the first { $count } folder: Expand All stops there
   *[other] Expanded the first { $count } folders: Expand All stops there
}

## New, rename, move to the Trash and duplicate (project_panel_ops.rs)

# The question before a move to the Trash: its buttons.
workbench-project-cancel = Cancel
workbench-project-move-to-trash = Move to Trash
# $name is the file or folder's name.
workbench-project-trash-question = Move “{ $name }” to the Trash?
# A move to the Trash's detail when the entry changed while it was asked.
workbench-project-trash-moved-since = It was moved or replaced since you were asked.
workbench-project-trash-failed = “{ $name }” couldn’t be moved to the Trash.
workbench-project-duplicate-failed = “{ $name }” couldn’t be duplicated.
# The question before moving several selected entries to the Trash. $count
# is how many.
workbench-project-trash-question-many = { $count ->
    [one] Move { $count } item to the Trash?
   *[other] Move { $count } items to the Trash?
}
# A toast when several of the selected entries failed: $count is how many.
# Its detail names each, a line apiece.
workbench-project-trash-failed-many = { $count ->
    [one] { $count } item couldn’t be moved to the Trash.
   *[other] { $count } items couldn’t be moved to the Trash.
}
workbench-project-duplicate-failed-many = { $count ->
    [one] { $count } item couldn’t be duplicated.
   *[other] { $count } items couldn’t be duplicated.
}
# A line of such a toast's detail: $message says which entry failed, as
# for one alone, and $error why.
workbench-project-failed-why = { $message } { $error }
# Under the name field while a rename waits for language servers.
workbench-project-renaming = Renaming…
# The name field's placeholders.
workbench-project-file-name = File name
workbench-project-folder-name = Folder name
workbench-project-new-name = New name
# A rename that found its entry changed under it. $name is the entry's.
workbench-project-rename-raced = “{ $name }” was moved or replaced while it was being renamed. Nothing was renamed.
# A rename of a file an editor is busy with. $name is the entry's.
workbench-project-rename-busy-saving = “{ $name }” is being saved. Rename it once it’s saved.
workbench-project-rename-busy-opening = “{ $name }” is being opened. Rename it once it’s open.
workbench-project-rename-busy-working = “{ $name }” is being worked on. Rename it once that’s done.
# Under the name field when $name is taken.
workbench-project-name-taken = A file or folder “{ $name }” already exists here. Choose another name.
# The button of a message about a failed operation.
workbench-project-ok = OK

## Cut, copy and paste (project_panel_clipboard.rs)

# A paste of $name into itself or a folder inside it.
workbench-project-paste-into-itself-cut = “{ $name }” can’t be moved into itself or a folder inside it.
workbench-project-paste-into-itself-copy = “{ $name }” can’t be copied into itself or a folder inside it.
workbench-project-paste-failed = “{ $name }” couldn’t be pasted.
workbench-project-move-failed = “{ $name }” couldn’t be moved.
# A toast when several of the entries pasted failed: $count is how many.
# Its detail names each, a line apiece.
workbench-project-paste-failed-many = { $count ->
    [one] { $count } item couldn’t be pasted.
   *[other] { $count } items couldn’t be pasted.
}
# A paste of a file an editor is busy with. $name is the entry's.
workbench-project-paste-busy-saving = “{ $name }” is being saved. Paste it once it’s saved.
workbench-project-paste-busy-opening = “{ $name }” is being opened. Paste it once it’s open.
workbench-project-paste-busy-working = “{ $name }” is being worked on. Paste it once that’s done.
# A paste that found its entry changed under it. $name is the entry's.
workbench-project-paste-raced = “{ $name }” was moved or replaced while it was being pasted. Nothing was moved.

## Undo and redo of file operations (project_panel_undo.rs)

# The tree's own commands, beside the editor's Undo and Redo.
command-project_panel-undo = Undo File Operation
command-project_panel-redo = Redo File Operation
# An undo or redo refused: $name is the entry's.
workbench-project-undo-changed = “{ $name }” was moved, replaced or deleted since, so it was left as it is.
workbench-project-undo-taken = “{ $name }” can’t be put back: something by that name is there now.
workbench-project-undo-not-in-trash = “{ $name }” is no longer in the Trash.
# An entry that changed, or couldn't be reached, as it was being undone or
# redone: it stays, to be asked about the next time.
workbench-project-undo-changed-kept = “{ $name }” changed meanwhile, so it was left as it is. Try again to go ahead.
workbench-project-undo-unreachable = “{ $name }” can’t be reached now. Try again once it can.
workbench-project-undo-failed = “{ $name }” couldn’t be put back.
# The question before undoing or redoing an operation on an entry that was
# replaced since (as another program's save replaces a file), and the
# button that goes ahead. $name is the entry's.
# $count is how many were replaced, $name the first's; with several, the
# question's detail names each, a line apiece.
workbench-project-undo-changed-question = { $count ->
    [one] “{ $name }” has changed since. Undo anyway?
   *[other] { $count } items have changed since. Undo anyway?
}
workbench-project-undo-anyway = Undo
workbench-project-redo-changed-question = { $count ->
    [one] “{ $name }” has changed since. Redo anyway?
   *[other] { $count } items have changed since. Redo anyway?
}
workbench-project-redo-anyway = Redo
workbench-project-undo-busy-saving = “{ $name }” is being saved. Undo once it’s saved.
workbench-project-undo-busy-opening = “{ $name }” is being opened. Undo once it’s open.
workbench-project-undo-busy-working = “{ $name }” is being worked on. Undo once that’s done.
# A toast when several entries of an undo or redo failed: $count is how
# many. Its detail names each, a line apiece.
workbench-project-undo-failed-many = { $count ->
    [one] { $count } item couldn’t be put back.
   *[other] { $count } items couldn’t be put back.
}

## Dragging rows onto a folder (project_panel_drag.rs)

# Under the pointer while several selected rows are dragged. $count is how
# many.
workbench-project-drag-count = { $count ->
    [one] { $count } item
   *[other] { $count } items
}
# A row dropped on a folder that has an entry of its name already: it isn't
# moved. $name is the entry's name, $folder the folder's.
workbench-project-drop-name-taken = “{ $name }” wasn’t moved: “{ $folder }” has a file or folder of that name already.

## The tree's right-click menu (project_panel_menu.rs)

# What a screen reader calls the menu a right click on the tree opens. Its
# items are the tree's commands, titled as the command palette lists them.
workbench-project-menu = File actions

## The header's New menu (project_panel_new_menu.rs, project_new_templates.rs)

# The + at the left of the dock's header: its tooltip and accessible name.
workbench-project-new = New…
# What a screen reader calls the menu the + opens.
workbench-project-new-menu = New
# The menu's first entries: a new file of any name, and a new folder.
workbench-project-new-file = File
workbench-project-new-folder = Folder
# Its typed entries: a new file of that type, named in the tree's name field
# and made with a small template.
workbench-project-new-markdown = Markdown File
workbench-project-new-rust = Rust File
workbench-project-new-typescript = TypeScript File
workbench-project-new-javascript = JavaScript File
workbench-project-new-python = Python File
workbench-project-new-go = Go File
workbench-project-new-java = Java File
workbench-project-new-html = HTML File
workbench-project-new-css = CSS File
workbench-project-new-json = JSON File
workbench-project-new-shell = Shell Script
# The name a typed entry's field starts with, before the extension
# (Untitled.py), its stem selected to type over. A file name: no spaces.
workbench-project-new-untitled = Untitled

## Structure, the outline (outline_panel.rs)

workbench-project-structure-no-file = No file open
workbench-project-structure-plain-text = No outline for plain text
# $language is the file's language, which has no outline query.
workbench-project-structure-unsupported = No outline for { $language }
workbench-project-structure-too-large = File too large for an outline
workbench-project-structure-parsing = Parsing…
workbench-project-structure-no-symbols = No symbols
# Under a capped outline. $count is the cap, $shown the cap written with
# commas between groups of three digits.
workbench-project-structure-capped = { $count ->
    [one] Showing the first { $shown } symbol
   *[other] Showing the first { $shown } symbols
}
