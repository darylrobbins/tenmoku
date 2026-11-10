# fe-workbench's sidebar and its activity rail (i18n design, T1540):
# `workbench-dock-…`.

## The Explorer's and Source Control's views (dock.rs)

workbench-dock-folders = Folders
workbench-dock-outline = Outline

## The rail's actions, at its foot: their tooltips and accessible names

workbench-dock-git-log = Git Log
workbench-dock-terminal = Terminal

## The rail's badges' accessible names

# Changes' count. $count is how many files the Changes view lists.
workbench-dock-changed-files = { $count ->
    [one] { $count } changed file
   *[other] { $count } changed files
}
# Problems' dot. $count is how many errors the Problems panel lists.
workbench-dock-errors = { $count ->
    [one] { $count } error
   *[other] { $count } errors
}
