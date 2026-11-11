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
