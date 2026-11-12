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
# A paste of a file an editor is busy with. $name is the entry's.
workbench-project-paste-busy-saving = “{ $name }” is being saved. Paste it once it’s saved.
workbench-project-paste-busy-opening = “{ $name }” is being opened. Paste it once it’s open.
workbench-project-paste-busy-working = “{ $name }” is being worked on. Paste it once that’s done.
# A paste that found its entry changed under it. $name is the entry's.
workbench-project-paste-raced = “{ $name }” was moved or replaced while it was being pasted. Nothing was moved.

## The tree's right-click menu (project_panel_menu.rs)

# What a screen reader calls the menu a right click on the tree opens. Its
# items are the tree's commands, titled as the command palette lists them.
workbench-project-menu = File actions

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
