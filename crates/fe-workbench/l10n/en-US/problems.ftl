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
# Lists the files a lane wrote alone: those with a lane chip.
workbench-problems-scope-from-agents = From agents

## Lane chips (docs/designs/agent-lanes.md § Problems and tests). $lane is
## the lane's number.

# On the row of a file the lane wrote: hunks of it the user accepted.
workbench-problems-lane-written = written by lane { $lane }
# On a problem on a row the lane wrote.
workbench-problems-lane-new = new, lane { $lane }

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
# From agents is chosen and no file a lane wrote has any.
workbench-problems-empty-from-agents-hint = None in the files a lane wrote; Project lists every open file
# Problems are there, and the filters hide every one.
workbench-problems-hidden = No problems shown
workbench-problems-hidden-hint = Turn a severity filter back on to list its problems

## The detail pane (problems_detail.rs), below the list for the selected
## problem: code and source, the message, a snippet and the suggested fix

# The button applying the language server's preferred quick fix for the
# problem; its key (⌥↵) shows beside it.
workbench-problems-detail-apply-fix = Apply fix

# The command the button and its key run.
command-problems-apply_fix = Apply Fix

# The buttons asking the chat agent about the problem: Ask an agent attaches
# it and leaves the prompt to send, Explain sends at once.
workbench-problems-detail-ask-agent = Ask an agent
workbench-problems-detail-explain = Explain
# Their tooltips while the agent can't be asked.
workbench-problems-ask-untrusted = This folder isn't trusted, so the chat can't be asked.
workbench-problems-ask-no-key = Set an API key in the chat to ask an agent.
# `code · source`, in the prompts below.
workbench-problems-origin = { $code } · { $source }
# The prompts. $message is the problem's message; $origin is `code · source`.
workbench-problems-ask-prompt = How do I fix this problem? { $message } ({ $origin })
workbench-problems-ask-prompt-bare = How do I fix this problem? { $message }
workbench-problems-explain-prompt = What does this problem mean, and why does it happen? { $message } ({ $origin })
workbench-problems-explain-prompt-bare = What does this problem mean, and why does it happen? { $message }

# The commands asking the chat agent about the selected problem.
command-problems-ask_agent = Ask an Agent About This Problem
command-problems-explain = Explain This Problem

## The status bar's items (the errors' and the warnings')

# Both items' tooltip. $errors and $warnings are the window's counts.
workbench-problems-status-tooltip = { $errors ->
    [one] { $errors } error
   *[other] { $errors } errors
}, { $warnings ->
    [one] { $warnings } warning
   *[other] { $warnings } warnings
}
