# fe-workbench's editor panes (i18n design, T1542): their tab strip's
# controls, their breadcrumbs, the toolbar picker and what a pane says while
# it opens a file: `workbench-pane-…`.

## What a pane shows in its tab's place

# The window's title while a file is read. $name is the file's name.
workbench-pane-title-loading = { $name } — loading
# $name is the file's name.
workbench-pane-loading = Loading { $name }…
workbench-pane-no-open-files = No open files
# A file that failed to open. $path is its path, $error why.
workbench-pane-cannot-open = Cannot open { $path }: { $error }

## Toasts

# $name is the file's name; the toast's detail says why.
workbench-pane-cant-open = Can't open { $name }
workbench-pane-save-before-image = Save the file before opening it as an image
workbench-pane-save-before-hex = Save the file before opening it as hex
# A hex viewer's selection too large to copy. $size is its size, $limit the
# most a copy takes, both as "12.3 MB".
workbench-pane-hex-copy-too-large = Can't copy { $size }: a copy takes at most { $limit }

## A file too large to edit, in a read-only viewer

# The viewer's banner. $size is the file's size, $limit the largest file
# that opens for editing, both as "12.3 MB".
workbench-pane-read-only-large = Read-only: { $size } is over { $limit }.
# Asked before editing it. $name is the file's name.
workbench-pane-edit-question = Edit { $name }?
# $size is the file's size, as "12.3 MB".
workbench-pane-edit-detail = It loads all { $size } into memory, which takes several seconds and about 1.5 times its size.
workbench-pane-edit = Edit
workbench-pane-cancel = Cancel
# The banner while it's read. $name is the file's name.
workbench-pane-loading-for-editing = Loading { $name } for editing…
# $name is the file's name, $error why it couldn't be read.
workbench-pane-cannot-read = Cannot read { $name }: { $error }

## Move to New Window

# The command's title: the palette shows it as "Pane: Move to New Window".
command-pane-move_to_new_window = Move to New Window
# Asked before a tab with unsaved changes moves. $name is the file's name.
workbench-pane-move-question = Save { $name } before moving it?
workbench-pane-move-detail = The new window opens the file as it is saved, so it moves only once its changes are saved.
workbench-pane-save-and-move = Save and Move
# A toast: the new window couldn't load the file, so the tab stays. $name
# is the file's name.
workbench-pane-cant-move = Can't move { $name } to a new window

## The buttons after the tabs: their tooltips and accessible names

# Lists the pane's tabs.
workbench-pane-tab-list = Tab List
workbench-pane-split-right = Split Right
# A Markdown file's.
workbench-pane-open-preview = Open Preview
workbench-pane-show-source = Show Source
workbench-pane-open-preview-to-side = Open Preview to the Side
# Lists the pane's actions.
workbench-pane-more-actions = More Actions

## The pane's actions, which More Actions lists

workbench-pane-action-split-right = Split Right
workbench-pane-action-split-down = Split Down
workbench-pane-action-maximize = Maximize
workbench-pane-action-restore = Restore
workbench-pane-action-move-to-new-window = Move to New Window
workbench-pane-action-exchange = Move to the Other Split
workbench-pane-action-pin-tab = Pin Tab
workbench-pane-action-unpin-tab = Unpin Tab
workbench-pane-action-close-other-tabs = Close Other Tabs
workbench-pane-action-reopen-closed-tab = Reopen Closed Tab
workbench-pane-action-close-tab = Close Tab
workbench-pane-action-close-pane = Close Pane
workbench-pane-action-move-tab-left = Move Tab Left
workbench-pane-action-move-tab-right = Move Tab Right
workbench-pane-action-move-tab-up = Move Tab Up
workbench-pane-action-move-tab-down = Move Tab Down
workbench-pane-action-toggle-breadcrumbs = Toggle Breadcrumbs

## The toolbar picker

# A directory a path crumb listed that couldn't be read. $dir is its path,
# $error why.
workbench-pane-cannot-read-dir = Cannot read { $dir }: { $error }
