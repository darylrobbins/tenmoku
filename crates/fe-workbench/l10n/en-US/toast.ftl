# fe-workbench's toasts (i18n design, T1556): `workbench-toast-…`.

# A toast's close button: its accessible name.
workbench-toast-dismiss = Dismiss
# A toast's button holding the choices that didn't fit beside it.
workbench-toast-more = More
# Beside a toast's message when an identical one came again. $count is how
# many times it has come, at least 2.
workbench-toast-count =
    { $count ->
        [one] ×{ $count }
       *[other] ×{ $count }
    }
