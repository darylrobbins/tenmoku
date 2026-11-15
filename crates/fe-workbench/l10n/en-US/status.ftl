# fe-workbench's status bar (i18n design, T1543): what the bar itself adds
# to the active item's items, and vim's command line in its place:
# `workbench-status-…`.

## The bar (status_bar.rs)

# Beside the vim mode while a macro records. $register is the register it
# records into, a single letter such as "a"; "@a" is how vim names it.
workbench-status-recording = recording @{ $register }
# The encoding item's tooltip; a click opens the encoding picker.
workbench-status-select-encoding = Select Encoding

# The agents item, where the platform draws the title bar: the same count as
# the title bar's chip. $count is how many lanes are Working.
workbench-status-agents-working = { $count ->
    [one] { $count } agent working
   *[other] { $count } agents working
}

## Vim's command line (vim_line.rs)

# The mode pill before the line's `:`, `/` or `?`, in capitals as vim's
# modes are.
workbench-status-command-line-mode = COMMAND

## The operation in progress (operations_status.rs)

# The count in the item, from its message: $done of $total, such as 3/12.
workbench-status-operation-count = { $done }/{ $total }
# The others running, beside the primary operation. $count is how many.
workbench-status-operation-more = +{ $count }
# The item's accessible name: $label is the primary operation's, $count how
# many others run.
workbench-status-operation-name =
    { $count ->
        [0] { $label }
        [one] { $label }, 1 more
       *[other] { $label }, { $count } more
    }
# The list's accessible name, opened from the item.
workbench-status-operations-title = Background operations
# What a list row says an operation comes from, besides a language server's
# own name: a task run, or other background work.
workbench-status-operation-source-run = Task
workbench-status-operation-source-other = Background
