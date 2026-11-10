# The diff view's hunk headers (docs/design/design-system.md, Review diff).

## A hunk's header row: the symbols around it, outermost first and joined by
## ›, then how many rows it removes and adds.

gpui-diff-hunk-header =
    { $count ->
        [one] { $symbol } · { $count } change
       *[other] { $symbol } · { $count } changes
    }
# With no symbol around it.
gpui-diff-hunk-changes =
    { $count ->
        [one] { $count } change
       *[other] { $count } changes
    }
