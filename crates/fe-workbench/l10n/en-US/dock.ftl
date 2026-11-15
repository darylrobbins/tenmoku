# fe-workbench's sidebar and its activity rail (i18n design, T1540):
# `workbench-dock-…`.

## The Explorer's and Source Control's views (dock.rs)

# The project tree's view, which the dock's header names: no header of its own.
workbench-dock-folders = Folders
# The outline's view, collapsed at the foot of Project (T1541).
workbench-dock-structure = Structure

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

## The panels' headers (dock.rs)

workbench-dock-references-title = References
workbench-dock-search-title = Search
workbench-dock-chat-title = Chat
workbench-dock-ai-changes-title = AI Changes
workbench-dock-agents-title = Agents

## The Agents panel (agents_panel.rs): the window's lanes, one row each

# Agents' rail badge. $count is how many lanes are working or have hunks to review.
workbench-dock-agents-badge = { $count ->
    [one] { $count } agent lane working or to review
   *[other] { $count } agent lanes working or to review
}
# A row's lane number, in the agent's colour. $number is the lane's number.
workbench-dock-agents-lane = lane { $number }
# A lane's state.
workbench-dock-agents-working = Working
workbench-dock-agents-pending-review = To review
workbench-dock-agents-accepted = Accepted
workbench-dock-agents-rejected = Rejected
workbench-dock-agents-abandoned = Abandoned
# A row's second line. $files is how many files the lane proposed to,
# $pending its undecided hunks, $conflicts its stale hunks not yet rejected.
workbench-dock-agents-counts = { $files ->
    [one] { $files } file
   *[other] { $files } files
} · { $pending } pending · { $conflicts ->
    [one] { $conflicts } conflict
   *[other] { $conflicts } conflicts
}
# A row's name to a screen reader: its lines. $lane is "lane 2", $name the
# lane's name, $state its state and $counts its second line.
workbench-dock-agents-row = { $lane }, { $name }, { $state }, { $counts }
# A row's buttons.
workbench-dock-agents-review = Review
workbench-dock-agents-stop = Stop
workbench-dock-agents-clear = Clear
# Clearing an Abandoned lane with hunks still undecided asks first. $lane
# is its number, $name its name and $count its undecided hunks.
workbench-dock-agents-clear-title = Clear lane { $lane }?
workbench-dock-agents-clear-detail = { $count ->
    [one] Lane { $lane } ({ $name }) ended with { $count } hunk undecided. Clearing it rejects it.
   *[other] Lane { $lane } ({ $name }) ended with { $count } hunks undecided. Clearing it rejects them.
}
workbench-dock-agents-clear-reject = Reject and Clear
workbench-dock-agents-clear-cancel = Cancel

## Commands

command-agent-show_panel = Show Agents
