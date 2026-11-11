# kiln-ui's messages.

## public
## Messages apps on kiln may use.

# A prompt's button that answers no and closes it. Escape presses a prompt
# button with this title (`Prompt::cancel_button`), as it does one titled
# "Cancel".
kiln-ui-cancel = Cancel

## Tooltips.

# A pinned tab's tooltip: its label, then its description, such as the
# folder that tells two files of one name apart.
kiln-ui-tab-pinned-tooltip = { $label } — { $description }
# A tooltip's label, then the shortcut that does the same, such as
# `Search ⇧⌘F`.
kiln-ui-tooltip-with-shortcut = { $label } { $shortcut }

## Tabs, as a screen reader names them.

# The strip of tabs above a pane's contents.
kiln-ui-tab-bar = Tabs
# A tab's name: its label, then its description, such as the folder that
# tells two files of one name apart.
kiln-ui-tab-label-described = { $label }, { $description }
# A tab with unsaved changes: its name, then that it's modified.
kiln-ui-tab-label-modified = { $label }, modified
# A tab's states, listed after it with commas between: it has unsaved
# changes, it's pinned to the strip's start, it's a preview the next file
# opened replaces.
kiln-ui-tab-state-modified = modified
kiln-ui-tab-state-pinned = pinned
kiln-ui-tab-state-preview = preview
# A tab's close button.
kiln-ui-tab-close = Close { $label }

## Landmarks: the names a screen reader gives the window's regions.

# The activity rail, the strip of buttons that show the panels.
kiln-ui-activity-bar = Activity bar
# The status bar along the window's bottom edge.
kiln-ui-status-bar = Status bar
