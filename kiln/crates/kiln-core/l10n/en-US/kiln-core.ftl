# kiln-core's messages.

## public
## Menu titles apps on kiln may use: kiln's bundled menus
## (kiln/src/setup/menus.toml) name them by `title_key`. `$app` is the app's
## name, which kiln gives every menu title (`menu::set_label_arg`).

kiln-core-menu-hide-app = Hide { $app }
kiln-core-menu-hide-others = Hide Others
kiln-core-menu-show-all = Show All
kiln-core-menu-quit-app = Quit { $app }
kiln-core-menu-view = View
kiln-core-menu-command-palette = Command Palette
kiln-core-menu-toggle-full-screen = Toggle Full Screen
kiln-core-menu-window = Window
kiln-core-menu-minimize = Minimize
kiln-core-menu-zoom = Zoom
kiln-core-menu-close-window = Close Window
# The submenu macOS fills with other apps' services, a Services item's
# title when its table gives none.
kiln-core-menu-services = Services

## Keys as shortcuts spell them out (`pc_label`, Linux and Windows): each
## modifier, then the key, joined by `+`, as in Ctrl+Shift+Left. macOS's
## shortcuts use symbols, except for Space and Click.

kiln-core-key-ctrl = Ctrl
kiln-core-key-alt = Alt
kiln-core-key-shift = Shift
kiln-core-key-super = Super
kiln-core-key-left = Left
kiln-core-key-right = Right
kiln-core-key-up = Up
kiln-core-key-down = Down
kiln-core-key-enter = Enter
kiln-core-key-escape = Esc
kiln-core-key-backspace = Backspace
kiln-core-key-delete = Delete
kiln-core-key-tab = Tab
kiln-core-key-space = Space
kiln-core-key-click = Click
kiln-core-key-page-up = PageUp
kiln-core-key-page-down = PageDown
kiln-core-key-home = Home
kiln-core-key-end = End

## Keystrokes as keymap files write them, such as `cmd-shift-left`: why one
## doesn't parse. `$name` and `$key` are quoted as written.

kiln-core-keystroke-expected = expected a keystroke
kiln-core-keystroke-modifier-expected = expected a modifier
kiln-core-keystroke-modifier-unknown = unknown modifier { $name }: expected cmd, ctrl, alt, shift, fn or secondary
kiln-core-keystroke-modifier-repeated = { $name } is repeated
kiln-core-keystroke-key-expected = expected a key
kiln-core-keystroke-key-unknown = unknown key { $key }

## Bindings that hide or delay each other. `$location` is a binding's
## file:line:column, `$keys` its keys, `$command` its command; `$other` and
## `$other_location` the other binding's; `$context` the frames, as
## [workbench, editor]; `$longer` the longer binding's keys.

kiln-core-keymap-overrides = { $location }: `{ $keys }` for { $command } overrides { $other } at { $other_location }
kiln-core-keymap-shadows = { $location }: `{ $keys }` for { $command } shadows { $other } at { $other_location } in { $context }
kiln-core-keymap-prefix-delay = { $location }: `{ $keys }` for { $command } waits for the chord timeout in { $context }, as `{ $longer }` for { $other } at { $other_location } starts with it

## The keychain (secrets). `$reason` is the platform's own error.

kiln-core-secrets-unavailable = no keychain is available: { $reason }
kiln-core-secrets-denied = access to the keychain was declined
kiln-core-secrets-other = keychain error: { $reason }
kiln-core-secrets-no-keychain = this platform has no keychain

## Config files.

# A `platform` filter naming no platform; `$name` is quoted as written.
kiln-core-platform-unknown = unknown platform { $name }: expected macos, linux or windows
# What a settings file was expected to be, after serde's "expected".
kiln-core-settings-expected-table = a settings file with a table `{ $name }`
# `$text` is quoted as written.
kiln-core-theme-colour-invalid = { $text } isn't a colour: expected "#rrggbb" or "#rrggbbaa"

## Settings and other TOML config files, checked against a schema.

# `$reason` is the system's error.
kiln-core-config-unreadable = cannot read the file: { $reason }
kiln-core-config-not-a-file = not a regular file
kiln-core-config-too-big = bigger than { $kib } KiB
kiln-core-config-unknown-key = unknown key, ignored
# `$expected` is one or more of the types below, joined by
# kiln-core-config-either; `$found` is TOML's name for the type written,
# such as `string` or `integer`.
kiln-core-config-type-mismatch = expected { $expected }, found { $found }
kiln-core-config-either = { $first } or { $second }
kiln-core-config-type-table = a table
kiln-core-config-type-array = an array
kiln-core-config-type-string = a string
kiln-core-config-type-integer = an integer
kiln-core-config-type-number = a number
kiln-core-config-type-boolean = a boolean
# `$value` is the number as written; `$min` and `$max` the bounds.
kiln-core-config-out-of-range = { $value } is out of range: must be from { $min } to { $max }
kiln-core-config-too-small = { $value } is too small: must be at least { $min }
kiln-core-config-too-large = { $value } is too large: must be at most { $max }
kiln-core-config-empty = must not be empty
kiln-core-config-too-short = must be at least { $min } { $min ->
        [one] character
       *[other] characters
    } long
# A value of the wrong type in a keymap or menu file. `$found` is TOML's
# name for the type written, or kiln-core-config-found-nothing.
kiln-core-config-expected-tables = expected an array of tables, found { $found }
kiln-core-config-expected-table = expected a table, found { $found }
kiln-core-config-expected-string = expected a string, found { $found }
kiln-core-config-expected-boolean = expected a boolean, found { $found }
kiln-core-config-found-nothing = nothing
# A required field left out; `$name` is the field.
kiln-core-config-missing = missing `{ $name }`
kiln-core-config-unknown-field = unknown field

## Keymap files. `$command` is a command ID, `$keys` a binding's keys;
## `$reason` is why the command rejects the binding's args.

kiln-core-keymap-click-alone = `{ $keys }`: a click binds only alone, with a modifier, as in `cmd-click`; the binding is ignored
kiln-core-keymap-unknown-command = unknown command { $command }; the binding is ignored
kiln-core-keymap-bad-args = bad args for { $command }: { $reason }
# `$name` is quoted as written.
kiln-core-keymap-platform-unknown = unknown platform { $name }: expected macos, linux, windows or default
kiln-core-keymap-expected-keys = expected a string or a table of keys by platform, found { $found }

## Menu files. `$menu`, `$id` and `$name` are quoted as written;
## `$command` is a command ID, and `$reason` why it rejects the item's args.

kiln-core-menu-repeated = menu { $menu } is repeated
kiln-core-menu-id-repeated = id { $id } is repeated in menu { $menu }
kiln-core-menu-before-and-after = set `before` or `after`, not both
kiln-core-menu-item-kind = expected one of `command`, `separator = true`, `system` or `remove = true`
kiln-core-menu-remove-needs-id = `remove` needs the `id` to remove
kiln-core-menu-system-unknown = unknown system menu { $name }: expected services
kiln-core-menu-os-action-unknown = unknown os_action { $name }: expected cut, copy, paste, select_all, undo or redo
kiln-core-menu-expected-frames = expected an array of frame names
kiln-core-menu-unknown-command = unknown command { $command }; the item is ignored
kiln-core-menu-bad-args = bad args for { $command }: { $reason }; the item is ignored
# `$name` is the literal field, `title` or `name`.
kiln-core-menu-title-and-key = set `{ $name }` or `title_key`, not both
kiln-core-menu-title-key-unknown = no en-US message { $key }; the title is the key

## Theme files.

kiln-core-theme-missing = no such file
kiln-core-theme-expected-name = expected a name: a string that isn't empty
kiln-core-theme-expected-appearance = expected "dark" or "light"
# A colour token's type, in kiln-core-config-type-mismatch's `$expected`.
kiln-core-theme-type-colour = a colour string
# `$version` is the file's `kiln_theme`; `$known` the version this reads.
kiln-core-theme-newer-version = written for version { $version }, but this reads version { $known }: keys it doesn't know are ignored
kiln-core-theme-expected-version = expected { $known }
# A value left out of a theme; `$problem` is one of the messages above, or
# a colour's or a type's.
kiln-core-theme-value-ignored = { $problem }, ignored

## The trust file, which records the folders whose code the user trusts.

# Comment lines at the top of the file, each starting `# `, written above
# its `[folders]` table. The commands it names are the app's.
kiln-core-trust-file-header =
    # This file records the folders you trusted (true) or didn't (false) to
    # run their code. Subfolders follow the nearest folder listed. The app's
    # Trust This Folder and Untrust This Folder commands change one.
kiln-core-trust-not-utf8 = { $folder } isn't UTF-8, so it can't be saved
kiln-core-trust-bad-file = { $path } isn't a trust file, so it's left as it is

## Writing one setting into a TOML file, keeping its comments. `$key` is
## the setting's dotted path, `$path` a file's, `$reason` the error.

kiln-core-write-failed = { $path }: { $reason }
kiln-core-write-number-unrepresentable = { $key }: { $value } isn't a number TOML can hold
kiln-core-write-keeps-changing = { $path } keeps changing
kiln-core-write-symlink-loop = too many levels of symbolic links at { $path }
kiln-core-write-invalid-toml = isn't valid TOML: { $reason }
kiln-core-write-not-a-table = { $key } isn't a table
kiln-core-write-is-a-table = { $key } is a table
kiln-core-write-drops-comment = removing { $key } would drop a comment beside it; edit the file by hand
# Where a new setting would go can't be found: the file is laid out in a
# way the writer doesn't expect.
kiln-core-write-dotted-outside-table = dotted keys outside of any table
kiln-core-write-table-unplaced = a table without a place
kiln-core-write-dotted-without-keys = a dotted table without keys
kiln-core-write-header-unplaced = a header without a place
