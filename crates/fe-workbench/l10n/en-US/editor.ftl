# fe-workbench's editor gutter text (i18n design, T1549): `workbench-editor-…`.

## Provenance (provenance_column.rs): who wrote each row, in the annotation
## column left of the line numbers. An agent's rows have no entry: they draw
## as the change bar's dotted agent lane.

# The entry on rows you wrote.
workbench-editor-provenance-you = You
# The entry on rows the language server's edits wrote.
workbench-editor-provenance-lsp = LSP
# The entry on rows the formatter wrote.
workbench-editor-provenance-format = Format
# Resting on an entry of yours.
workbench-editor-provenance-written-by-you = Written by you
# Resting on another author's entry; $author is the entry's text: a tool's or
# the host's name.
workbench-editor-provenance-written-by = Written by { $author }

## Lane marks (lane_marks.rs): a proposal's lane chip, in its blocks' header
## rows before Accept and Reject. $lane is the lane's number.

# Its hunks in the editor are all pending.
workbench-editor-lane-pending = lane { $lane } · pending
# Its hunks in the editor are all stale: none can be accepted.
workbench-editor-lane-stale = lane { $lane } · stale
# Some of its hunks in the editor are stale ($count), the rest pending.
workbench-editor-lane-conflicts =
    { $count ->
        [one] lane { $lane } · { $count } conflict
       *[other] lane { $lane } · { $count } conflicts
    }
