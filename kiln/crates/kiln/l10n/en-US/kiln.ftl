# kiln's messages.

## Command titles: `command-<id>`, the id's `.` as `-`. kiln's typed app and
## window commands (kiln::app, kiln::window). `$app` is the app's name, which
## kiln gives every command title once it starts.

command-app-quit = Quit
command-app-hide = Hide { $app }
command-app-hide_others = Hide Others
command-app-show_all = Show All
command-window-minimize = Minimize
command-window-zoom = Zoom
command-window-toggle_fullscreen = Toggle Full Screen
command-window-close = Close Window

## Theme selections. `$id` is a theme's ID; `$appearance` is the theme's
## appearance and `$slot` the one it was chosen for, `dark` or `light`.

kiln-theme-wrong-appearance = theme { $id } is { $appearance ->
        [light] light
       *[dark] dark
    }, so it can't be the { $slot ->
        [light] light
       *[dark] dark
    } theme
kiln-theme-not-installed = theme { $id } isn't installed

## Contributed keys and menu items. `$command` and `$other` are command
## IDs, `$keys` a binding's keys; `$menu` and `$id` are quoted.

kiln-keys-bound-twice = { $keys } is bound to both { $other } and { $command }
# Why a command that takes no args rejects them.
kiln-command-takes-no-args = { $command } takes no args
kiln-menu-item-unknown-command = unknown command { $command }; the item is left out
# `$reason` is why the command rejects the item's args.
kiln-menu-item-bad-args = bad args for { $command }: { $reason }; the item is left out
kiln-menu-repeated = menu { $menu } is repeated
kiln-menu-id-repeated = id { $id } is repeated in menu { $menu }

## Extensions. `$extension` is an extension's ID.

# A theme an extension contributes whose ID another of its themes has.
kiln-extension-theme-id-taken = `{ $id }` is the ID of another of { $extension }'s themes, which loads instead
# Names the app in a licence notice when it has no name of its own.
kiln-extension-notice-unnamed-app = The app
# Why a workspace's extensions aren't loaded: `$root` is the workspace.
kiln-extension-workspace-untrusted = { $root } isn't trusted
# Why a file an extension contributes isn't read.
kiln-extension-file-missing = no such file
kiln-extension-file-unreadable = can't be read: { $reason }
# `$target` is where a link in the file's path leads.
kiln-extension-file-leads-outside = leads to { $target }, outside { $extension }'s directory, so it isn't read
kiln-extension-cant-unbind = an extension can't unbind keys: `command = ""` for { $keys } is ignored
kiln-extension-cant-remove-item = an extension can't remove menu items: the removal of { $id } from { $menu } is ignored
kiln-extension-cant-replace-item = { $menu } already has an item { $id }; an extension can't replace it, so this one is ignored
