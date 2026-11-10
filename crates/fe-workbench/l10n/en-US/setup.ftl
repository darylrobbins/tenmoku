# fe-workbench's setup wizard, start page, keyboard shortcuts editor and small
# pickers (i18n design, section 9, card W4). Every key here starts
# `workbench-setup-`.

## The setup wizard (setup_wizard.rs, setup_wizard_model.rs)

# The pages' headings and the line under each.
workbench-setup-appearance = Appearance
workbench-setup-appearance-subtitle = Choose the colors Tenmoku shows.
workbench-setup-font = Font
workbench-setup-font-subtitle = Choose how text looks in the editor.
workbench-setup-keyboard = Keyboard
workbench-setup-keyboard-subtitle = Choose the keys you edit with.
workbench-setup-editing = Editing
workbench-setup-editing-subtitle = Choose what the editor does for you.
workbench-setup-ready = Ready
workbench-setup-ready-subtitle = That's everything Tenmoku needs to start.

# Each control's title and its one-line description. The titles are also
# what VoiceOver calls the controls.
workbench-setup-mode = Mode
workbench-setup-mode-description = Stay dark or light, or follow the system's appearance.
workbench-setup-dark-theme = Dark theme
workbench-setup-dark-theme-description = The colors to use when the appearance is dark.
workbench-setup-light-theme = Light theme
workbench-setup-light-theme-description = The colors to use when the appearance is light.
workbench-setup-font-family = Font family
workbench-setup-font-family-description = The font text is shown in, in the editor and the terminal.
workbench-setup-font-size = Font size
workbench-setup-font-size-description = The size of text, in points.
workbench-setup-ligatures = Ligatures
workbench-setup-ligatures-description = Join character sequences such as -> into one shape, if the font has them.
workbench-setup-keys = Keys
workbench-setup-keys-description = Tenmoku's standard VS Code-style keys, or Vim's modes and motions.
workbench-setup-vim-keys = Vim keys
workbench-setup-vim-keys-description = Which maps Vim starts with, on top of your own.
workbench-setup-line-numbers = Line numbers
workbench-setup-line-numbers-description = Number rows absolutely, or by their distance from the caret.
workbench-setup-format-on-save = Format on save
workbench-setup-format-on-save-description = Have the language server format a file as it's saved.
workbench-setup-auto-save = Auto save
workbench-setup-auto-save-description = Save changed files without being asked.
workbench-setup-inline-diagnostics = Inline diagnostics
workbench-setup-inline-diagnostics-description = Which problems show their message at the end of their row.
workbench-setup-minimap = Minimap
workbench-setup-minimap-description = Draw the whole file small beside the text.
workbench-setup-inlay-hints = Inlay hints
workbench-setup-inlay-hints-description = Show the types and parameter names a language server infers.
workbench-setup-soft-wrap = Soft wrap
workbench-setup-soft-wrap-description = Wrap long rows at the edge of the editor rather than scrolling.

# The controls' options.
workbench-setup-mode-dark = Dark
workbench-setup-mode-light = Light
workbench-setup-mode-system = Match System
# $family is the font an unset font family stands for, such as Spline Sans Mono.
workbench-setup-font-default = Default ({ $family })
workbench-setup-keys-standard = Standard
workbench-setup-keys-vim = Vim
workbench-setup-vim-keys-plain = Plain vim
workbench-setup-vim-keys-lazyvim = LazyVim, Space as leader
workbench-setup-line-numbers-on = On
workbench-setup-line-numbers-relative = Relative
workbench-setup-line-numbers-hybrid = Hybrid
workbench-setup-auto-save-after-delay = After a pause
workbench-setup-auto-save-on-focus-change = When focus leaves
workbench-setup-inline-diagnostics-errors = Errors
workbench-setup-inline-diagnostics-warnings = Errors and warnings
# A switch's two options.
workbench-setup-off = Off
workbench-setup-on = On
# What a value set by hand, which the wizard doesn't offer, shows as.
workbench-setup-custom = Custom

# What VoiceOver calls the font size's − and + buttons.
workbench-setup-font-size-decrease = Decrease font size
workbench-setup-font-size-increase = Increase font size
# The same, for a stepper of another setting.
workbench-setup-decrease = Decrease
workbench-setup-increase = Increase

# What VoiceOver calls the code sample beside the choices.
workbench-setup-code-sample = Code sample

# What VoiceOver calls the sheet. $page and $count are numbers, $title is the
# page's heading.
workbench-setup-dialog = Set Up Tenmoku, step { $page } of { $count }, { $title }

# The footer's buttons.
workbench-setup-skip = Skip setup
workbench-setup-skip-tooltip = Keeps what you've chosen so far
workbench-setup-back = Back
workbench-setup-continue = Continue
workbench-setup-finish = Start Editing

# The Ready page. $count is how many settings the run changed.
workbench-setup-changed = { $count ->
    [0] Tenmoku's defaults are set
    [one] You changed { $count } setting
   *[other] You changed { $count } settings
}
workbench-setup-change-in-settings = Change any of them in Settings
# $keys is the shortcut that opens Settings, such as ⌘,.
workbench-setup-change-in-settings-keys = Change any of them in Settings ({ $keys })
# The Ready page's hints: commands worth knowing.
workbench-setup-hint-command-palette = Command Palette
workbench-setup-hint-go-to-file = Go to File
workbench-setup-hint-open-folder = Open Folder
# A hint with its shortcut. $name is the hint, $keys its shortcut.
workbench-setup-hint-keys = { $name } ({ $keys })
workbench-setup-shell-command = Open files from Terminal with `tenmoku`
workbench-setup-install = Install

# A choice that couldn't be written. $key is the setting, as the docs spell
# it: `theme.mode`.
workbench-setup-change-failed = Can't change { $key }
workbench-setup-no-settings-file = there's no settings file to write to

## The Start page (start_page_model.rs)

# The sections' headings.
workbench-setup-start-start = Start
workbench-setup-start-recent-folders = Recent Folders
workbench-setup-start-recent-files = Recent Files
workbench-setup-start-settings = Settings
# The rows. With vim on, a row's dashboard key is underlined where its letter
# first shows in the row, whatever the case: n New File, f Open File, p Open
# Folder, g Find Text, r Recent Files, s Open Settings, q Quit. A translation
# without the letter underlines nothing; the key still works.
workbench-setup-start-new-file = New File
workbench-setup-start-open-file = Open File…
workbench-setup-start-open-folder = Open Folder…
workbench-setup-start-clone = Clone Repository…
workbench-setup-start-find-text = Find Text
workbench-setup-start-recent = Recent Files…
workbench-setup-start-open-settings = Open Settings
workbench-setup-start-shortcuts = Keyboard Shortcuts
workbench-setup-start-quit = Quit

## The Keyboard Shortcuts editor (shortcuts_editor.rs, shortcuts_recorder.rs)

workbench-setup-shortcuts-title = Keyboard Shortcuts
workbench-setup-shortcuts-search = Search keybindings by command or keys
workbench-setup-shortcuts-nothing-found = No keybindings found
# The table's column headings. The When heading is also the placeholder of a
# new binding's When box.
workbench-setup-shortcuts-column-command = Command
workbench-setup-shortcuts-column-keybinding = Keybinding
workbench-setup-shortcuts-column-when = When
workbench-setup-shortcuts-column-source = Source
# Where a binding comes from: a keymap the app ships, or the user's keymap.toml.
workbench-setup-shortcuts-source-default = Default
workbench-setup-shortcuts-source-user = User
# A line's menu.
workbench-setup-shortcuts-copy-command-id = Copy Command ID
workbench-setup-shortcuts-add = Add Keybinding
workbench-setup-shortcuts-remove = Remove Keybinding
workbench-setup-shortcuts-reset = Reset Keybinding
workbench-setup-shortcuts-show-same = Show Same Keybindings
# Beside Reset Keybinding, disabled, when the tab can't tell what the user's
# keymap.toml does to the default on another platform.
workbench-setup-shortcuts-reset-refused-hint = Edit keymap.toml
# The same, as a toast's detail, when keymap.toml changed after the menu
# offered Reset.
workbench-setup-shortcuts-reset-refused = Reset can't tell what the entries for other platforms in keymap.toml do to this default; edit keymap.toml

# A dimmed binding's tooltip: a later entry with the same keys and context
# replaces it. $location is that entry's file and line, `keymap.toml:12`.
workbench-setup-shortcuts-hidden-by = Hidden by { $location }
workbench-setup-shortcuts-hidden-by-unbind = Hidden by an unbind entry
# A clashing binding's warning. $commands is a list of command IDs, joined by
# commas; $also and $starts are two such lists.
workbench-setup-shortcuts-also-bound = Also bound to { $commands }
workbench-setup-shortcuts-starts-keys = Starts the keys of { $commands }
workbench-setup-shortcuts-also-bound-and-starts-keys = Also bound to { $also }; Starts the keys of { $starts }
# A command whose longer keys a binding starts, in that list: its ID and keys.
workbench-setup-shortcuts-command-keys = { $command } ({ $keys })

# A failed change to keymap.toml: a toast, with the reason as its detail.
workbench-setup-shortcuts-change-failed = Can't change the keybinding
workbench-setup-shortcuts-no-config-dir = There's no config directory
workbench-setup-shortcuts-write-dropped = the write was dropped

# The recorder's box, before a key is pressed.
workbench-setup-shortcuts-recorder-prompt = Press desired key combination, then Enter

## Config problems (config_problems.rs)

# The status bar's item, while the settings, keymap or theme files have
# problems.
workbench-setup-config-problems-status = { $count ->
    [one] { $count } config problem
   *[other] { $count } config problems
}
# How many problems the list shows.
workbench-setup-config-problems-count = { $count ->
    [one] { $count } problem
   *[other] { $count } problems
}
# A row's second line. $place is the file name, line and column, as a
# compiler writes them: `settings.toml:3:7`.
workbench-setup-config-problem-detail = { $place } · { $severity ->
    [error] error
   *[warning] warning
}
# A file that can't be opened at its problem: a dialog, with the reason as
# its detail, and its one button. $file is the file's name.
workbench-setup-config-open-failed = Can't open { $file }
workbench-setup-config-ok = OK
workbench-setup-config-not-a-file = not a regular file
# $kib is the most a config file may hold, in KiB: 1024.
workbench-setup-config-too-big = { $kib ->
    [one] bigger than { $kib } KiB
   *[other] bigger than { $kib } KiB
}

## The theme picker (theme_picker.rs)

# A theme picked that couldn't be written to settings.toml: a toast, with the
# reason as its detail. $theme is the theme's title.
workbench-setup-theme-keep-failed = Can't keep the theme { $theme }
# $path is settings.toml's path, $error the system's reason.
workbench-setup-theme-write-error = { $path }: { $error }
workbench-setup-theme-no-config-dir = there's no config directory

## The encoding picker (encoding_picker.rs)

# The status bar's encoding item lists two actions, then the encodings for one.
workbench-setup-encoding-actions-placeholder = Select Action
workbench-setup-encoding-reopen = Reopen with Encoding
workbench-setup-encoding-save = Save with Encoding
workbench-setup-encoding-reopen-placeholder = Select File Encoding to Reopen File
workbench-setup-encoding-save-placeholder = Select File Encoding to Save with
# The encodings offered.
workbench-setup-encoding-utf-8 = UTF-8
workbench-setup-encoding-utf-8-bom = UTF-8 with BOM
workbench-setup-encoding-utf-16le = UTF-16 LE
workbench-setup-encoding-utf-16be = UTF-16 BE
workbench-setup-encoding-windows-1252 = Western (Windows 1252)
workbench-setup-encoding-iso-8859-15 = Western (ISO 8859-15)
workbench-setup-encoding-windows-1250 = Central European (Windows 1250)
workbench-setup-encoding-iso-8859-2 = Central European (ISO 8859-2)
workbench-setup-encoding-windows-1251 = Cyrillic (Windows 1251)
workbench-setup-encoding-koi8-r = Cyrillic (KOI8-R)
workbench-setup-encoding-windows-1253 = Greek (Windows 1253)
workbench-setup-encoding-iso-8859-7 = Greek (ISO 8859-7)
workbench-setup-encoding-windows-1254 = Turkish (Windows 1254)
workbench-setup-encoding-windows-1255 = Hebrew (Windows 1255)
workbench-setup-encoding-windows-1256 = Arabic (Windows 1256)
workbench-setup-encoding-shift-jis = Japanese (Shift JIS)
workbench-setup-encoding-euc-jp = Japanese (EUC-JP)
workbench-setup-encoding-gbk = Simplified Chinese (GBK)
workbench-setup-encoding-gb18030 = Simplified Chinese (GB18030)
workbench-setup-encoding-big5 = Traditional Chinese (Big5)
workbench-setup-encoding-euc-kr = Korean (EUC-KR)

# What an encoding operation's toasts say. $name is the file's name; $encoding
# is the encoding's name, such as UTF-16LE or windows-1252.
workbench-setup-encoding-saving = “{ $name }” is being saved
workbench-setup-encoding-saving-detail = Try again once the save is done.
workbench-setup-encoding-running = “{ $name }” is already changing its encoding
workbench-setup-encoding-running-detail = Try again once that's done.
workbench-setup-encoding-reopen-no-file = Only a file on disk can be reopened with an encoding
workbench-setup-encoding-reopen-dirty = “{ $name }” has unsaved changes
workbench-setup-encoding-reopen-dirty-detail = Save or undo them before reopening it with { $encoding }.
workbench-setup-encoding-reopen-failed = Couldn't reopen “{ $name }”
workbench-setup-encoding-reopen-changed = “{ $name }” changed while it was read
workbench-setup-encoding-reopen-changed-detail = It wasn't reopened with { $encoding }.
workbench-setup-encoding-save-failed = “{ $name }” wasn't saved in { $encoding }
workbench-setup-encoding-save-changed = “{ $name }” changed while it was checked
workbench-setup-encoding-save-changed-detail = It wasn't set to { $encoding }.

## The small pickers (encoding_picker.rs, line_ending_picker.rs, language_picker.rs)

# Beside the row of what the editor has now.
workbench-setup-current = current
workbench-setup-line-ending-placeholder = Select End of Line Sequence
