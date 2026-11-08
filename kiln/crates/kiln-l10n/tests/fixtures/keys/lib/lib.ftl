## public

lib-public = { $count ->
    [one] One
   *[other] { $count }
}

## Private to the library.

lib-private = Private
