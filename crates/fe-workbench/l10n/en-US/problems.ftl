# fe-workbench's Problems panel (i18n design, T1547): `workbench-problems-…`.

## The panel (problems_panel.rs)

# The panel's name: the dock's header and its rail button's tooltip.
workbench-problems-title = Problems

## The header's scope switch

# The switch's accessible name.
workbench-problems-scope = Problems scope
# Lists the active editor's file alone.
workbench-problems-scope-this-file = This file
# Lists every file.
workbench-problems-scope-project = Project

## The header's severity filters: their accessible names and tooltips. Each
## shows its count beside its glyph and hides its severity while off.

# $count is how many errors the scope has.
workbench-problems-filter-errors = { $count ->
    [one] { $count } error
   *[other] { $count } errors
}
# $count is how many warnings the scope has.
workbench-problems-filter-warnings = { $count ->
    [one] { $count } warning
   *[other] { $count } warnings
}
# $count is how many information and hint diagnostics the scope has.
workbench-problems-filter-infos = { $count ->
    [one] { $count } info
   *[other] { $count } infos
}

## The empty state, centred in the panel: a heading and one hint

workbench-problems-empty = No problems
# Nothing's listed in any open file.
workbench-problems-empty-hint = Errors and warnings in the open files show here
# This file is chosen and its file has none, or no file is active.
workbench-problems-empty-this-file-hint = None in this file; Project lists every open file
# Problems are there, and the filters hide every one.
workbench-problems-hidden = No problems shown
workbench-problems-hidden-hint = Turn a severity filter back on to list its problems

## The status bar's items (the errors' and the warnings')

# Both items' tooltip. $errors and $warnings are the window's counts.
workbench-problems-status-tooltip = { $errors ->
    [one] { $errors } error
   *[other] { $errors } errors
}, { $warnings ->
    [one] { $warnings } warning
   *[other] { $warnings } warnings
}
