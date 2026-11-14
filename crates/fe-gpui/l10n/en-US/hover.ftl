# The hover card's footer (docs/design/design-system.md, Editor).

## How many callers what the hover is about has, from the call hierarchy:
## `$shown` is the count, "99+" at the cap; `$count` picks the plural.
gpui-hover-callers =
    { $count ->
        [one] { $shown } caller
       *[other] { $shown } callers
    }
