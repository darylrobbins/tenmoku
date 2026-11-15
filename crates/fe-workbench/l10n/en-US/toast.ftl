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

# An agent's toast when its lane is done and waits for review. $lane is the
# lane's number, $name what the agent calls itself, $files how many files
# have changes to review.
workbench-toast-lane-done =
    lane { $lane } · { $name } is done: { $files ->
        [one] { $files } file
       *[other] { $files } files
    } to review
# That toast's action: opens the lane's changes for review.
workbench-toast-review-changes = Review changes
# That toast's other button: leaves the changes for now.
workbench-toast-later = Later
