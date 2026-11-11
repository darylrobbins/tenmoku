# fe-workbench's status bar (i18n design, T1543): what the bar itself adds
# to the active item's items, and vim's command line in its place:
# `workbench-status-…`.

## The bar (status_bar.rs)

# Beside the vim mode while a macro records. $register is the register it
# records into, a single letter such as "a"; "@a" is how vim names it.
workbench-status-recording = recording @{ $register }
# The encoding item's tooltip; a click opens the encoding picker.
workbench-status-select-encoding = Select Encoding

## Vim's command line (vim_line.rs)

# The mode pill before the line's `:`, `/` or `?`, in capitals as vim's
# modes are.
workbench-status-command-line-mode = COMMAND
