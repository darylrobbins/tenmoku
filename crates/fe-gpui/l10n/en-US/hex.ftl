# The hex viewer (hex_viewer.rs): a read-only tab of a file's bytes, as rows
# of offset, hex and ASCII, under a header strip.

command-hex-go_to_offset = Go to Offset…
command-hex-find = Find Bytes
command-hex-find_next = Find Next Bytes
command-hex-find_previous = Find Previous Bytes
command-hex-copy = Copy Bytes
command-hex-copy_as_hex = Copy as Hex
command-hex-copy_as_text = Copy as Text

## The rows of an empty file.
gpui-hex-empty = Empty file
## The header, while nothing else shows there: the file's length.
gpui-hex-size = { $bytes ->
    [one] 1 byte
   *[other] { $bytes } bytes
}
## The header's button that opens the file in an editor (or, too large to
## edit, in the text viewer).
gpui-hex-open-as-text = Text
gpui-hex-changed = { $name } changed on disk.
## `$error` is the system's reason, in its own words.
gpui-hex-cannot-read = Cannot read { $name }: { $error }
gpui-hex-reload = Reload

## The status bar: the selected byte's offset, as `0x1A2F`; with more than
## one byte selected, the first's and the last's, and how many.
gpui-hex-status-offset = Offset { $offset }
gpui-hex-status-range = Offset { $start }–{ $end }, { $count ->
    [one] 1 byte
   *[other] { $count } bytes
}

## Go to Offset, in the header: the input's name and placeholder, and why
## what's typed isn't an offset. `$last` is the last byte's, as `0x1A2F`.
gpui-hex-go-to-offset = Go to offset
gpui-hex-go-to-placeholder = Offset, decimal or 0x hex
gpui-hex-go-to-invalid = Not an offset: { $text }
gpui-hex-go-to-past-end = { $offset } is past the end: the last byte is { $last }

## Find, in the header: the input's name and placeholder, how its text reads
## (hex pairs, or text), Match Case, why a hex query isn't bytes, and what
## the last search said.
gpui-hex-find = Find bytes
gpui-hex-find-placeholder = Find
gpui-hex-find-hex = Hex
gpui-hex-find-text = Text
gpui-hex-match-case = Match Case
gpui-hex-find-odd = An odd number of hex digits
gpui-hex-find-not-hex = Not a hex digit: { $digit }
gpui-hex-find-wrapped = Wrapped
gpui-hex-find-not-found = Not found
