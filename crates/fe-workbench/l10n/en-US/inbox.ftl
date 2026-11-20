# fe-workbench's notification inbox and its bell (i18n design, T1583):
# `workbench-inbox-…`.

# The status bar's bell: its accessible name and tooltip. $count is how many
# notifications need the user.
workbench-inbox-bell =
    { $count ->
        [0] Notifications
        [one] Notifications, { $count } needs you
       *[other] Notifications, { $count } need you
    }
# The bell's accessible name and tooltip while Focus mode is on: non-urgent
# toasts are held. $count is how many notifications need the user.
workbench-inbox-bell-focus =
    { $count ->
        [0] Notifications, Focus mode on
        [one] Notifications, Focus mode on, { $count } needs you
       *[other] Notifications, Focus mode on, { $count } need you
    }
# The popup footer's toggle for Focus mode, which holds non-urgent toasts in
# the inbox until there has been no input for a while.
workbench-inbox-focus = Focus mode
# The inbox popup's accessible name.
workbench-inbox-title = Notifications
# The group of notifications waiting on the user, each with its action.
workbench-inbox-needs-you = Needs you
# The group of everything else.
workbench-inbox-earlier = Earlier
# Removes every notification in Earlier.
workbench-inbox-clear-earlier = Clear earlier
# Shown in the popup when there are no notifications.
workbench-inbox-empty = No notifications
# A row's dismiss button: its accessible name.
workbench-inbox-dismiss = Dismiss
# A row's age: under a minute.
workbench-inbox-age-now = now
# A row's age in whole minutes, hours or days, at least 1.
workbench-inbox-age-minutes =
    { $count ->
        [one] { $count } min
       *[other] { $count } min
    }
workbench-inbox-age-hours =
    { $count ->
        [one] { $count } h
       *[other] { $count } h
    }
workbench-inbox-age-days =
    { $count ->
        [one] { $count } d
       *[other] { $count } d
    }

command-notifications-show = Show Notifications
command-notifications-toggle_focus = Toggle Notification Focus Mode
