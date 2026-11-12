# fe-workbench's chrome and the rest of its text (i18n W8, T1563):
# `workbench-workbench-…`. The workbench's pickers, saves and reloads, the
# vim commands the workbench runs, formatters, workspace edits and the TODO
# comments.

## The workbench's pickers: their search fields' placeholders.

workbench-workbench-language-placeholder = Select a language…
workbench-workbench-config-problems-placeholder = Filter config problems…
workbench-workbench-go-to-line-placeholder = Line or line:column…
workbench-workbench-toolbar-placeholder = Filter…
workbench-workbench-recent-placeholder = Open recent folder or file…
workbench-workbench-theme-placeholder = Select a color theme…
workbench-workbench-extensions-placeholder = Filter installed extensions…
workbench-workbench-quick-open-placeholder = Go to file…

## Saving, auto save and files that changed on disk.

# A toast: auto save didn't save a file that changed on disk since it was
# read. $name is the file's name.
workbench-workbench-auto-save-changed = Auto save skipped: { $name } changed on disk
# A toast: auto save failed; its detail says why. $name is the file's name.
workbench-workbench-auto-save-failed = Can't auto save { $name }
# A toast: a save failed; its detail says why. $name is the file's name.
workbench-workbench-save-failed = Can't save { $name }
# The prompt over a file with unsaved changes that changed on disk. $name is
# the file's name.
workbench-workbench-disk-changed = “{ $name }” changed on disk.
workbench-workbench-disk-changed-detail = Reload it and lose your unsaved changes?
# The prompt's buttons.
workbench-workbench-disk-reload = Reload
workbench-workbench-disk-keep-mine = Keep Mine
# The button of the system's open dialog.
workbench-workbench-open-button = Open
# Creating the settings file failed, and removing its temporary copy failed
# too. $error is why creating failed, $path the temporary copy, $cleanup why
# removing it failed.
workbench-workbench-settings-cleanup-failed = { $error } (and removing { $path } failed: { $cleanup })

## Compare Active File with Saved.

workbench-workbench-compare-no-file = E32: No file name
# $path is the file's path.
workbench-workbench-compare-missing = File missing: { $path }
workbench-workbench-compare-too-large = Too large to compare: { $path }
# $error is why.
workbench-workbench-compare-cant-open = Can't open { $path }: { $error }

## Markdown preview: its tab's title.

# $name is the source file's name.
workbench-workbench-preview-title = Preview { $name }
workbench-workbench-preview-untitled = Preview Untitled

## Vim commands the workbench runs: vim's own messages, in nvim's English,
## and Tenmoku's.

workbench-workbench-vim-e13 = E13: File exists (add ! to override)
workbench-workbench-vim-e32 = E32: No file name
workbench-workbench-vim-e37 = E37: No write since last change (add ! to override)
workbench-workbench-vim-e99 = E99: Current buffer is not in diff mode
# A shell command in an editor that doesn't run them, such as a diff view's.
workbench-workbench-vim-no-shell = Not supported yet: shell commands
# `:e` without a file name.
workbench-workbench-vim-no-reload = Not supported yet: reloading a file from disk
workbench-workbench-vim-no-search = No search to replace
# `:w` to a file another tab is saving. $name is its name.
workbench-workbench-vim-saved-by-other = “{ $name }” is being saved by another tab
# $title is the dirty tab's title.
workbench-workbench-vim-e162 = E162: No write since last change for buffer "{ $title }"
# $title is the untitled tab's title.
workbench-workbench-vim-e141 = E141: No file name for buffer "{ $title }"
# $file is the file as typed.
workbench-workbench-vim-e484 = E484: Can't open file { $file }
# What `:r {file}` reports: $file as typed, its line and byte counts.
workbench-workbench-vim-read-report = "{ $file }" { $lines }L, { $bytes }B
# The same, for a file without a final line break.
workbench-workbench-vim-read-report-noeol = "{ $file }" [noeol] { $lines }L, { $bytes }B
# A path after `:e` that can't be opened. $path is as typed, $why the reason.
workbench-workbench-vim-cant-open = Can't open { $path }: { $why }
workbench-workbench-vim-no-folder = no folder to resolve it against
# $path is a folder on the way.
workbench-workbench-vim-not-a-directory = { $path } is not a directory
workbench-workbench-vim-e16 = E16: Invalid range
workbench-workbench-vim-e493 = E493: Backwards range given
workbench-workbench-vim-e23 = E23: No alternate file
workbench-workbench-vim-e85 = E85: There is no listed buffer
workbench-workbench-vim-e516 = E516: No buffers were deleted
# `:ls`'s name for a buffer without a file.
workbench-workbench-vim-no-name = [No Name]
# $number is the buffer's number.
workbench-workbench-vim-e86 = E86: Buffer { $number } does not exist
workbench-workbench-vim-e89 = E89: No write since last change for buffer { $number } (add ! to override)
# $name is the name as typed.
workbench-workbench-vim-e93 = E93: More than one match for { $name }
workbench-workbench-vim-e94 = E94: No matching buffer for { $name }
# `:bd` of several buffers. $count is how many, at least 2.
workbench-workbench-vim-buffers-deleted =
    { $count ->
        [one] { $count } buffer deleted
       *[other] { $count } buffers deleted
    }
workbench-workbench-vim-e444 = E444: Cannot close last window
workbench-workbench-vim-e445-window = E445: Other window contains changes
workbench-workbench-vim-only-window = Already only one window
workbench-workbench-vim-e445-tab = E445: Other tab page contains changes
workbench-workbench-vim-only-tab = Already only one tab page

## Vim's shell commands: filters (`:{range}!`) and `:r !`.

workbench-workbench-filter-untrusted = Shell commands run only in a trusted folder
workbench-workbench-filter-busy = A shell command is already running in this editor
workbench-workbench-filter-e499 = E499: Empty file name for '%' or '#', only works with ":p:h"
workbench-workbench-filter-e194 = E194: No alternate file name to substitute for '#'
workbench-workbench-filter-no-names-on-windows = % and # aren't put in a shell command on Windows: cmd.exe's quoting isn't safe to build
workbench-workbench-filter-unknown-shell = % goes in a shell command only for sh, bash, zsh, ksh, dash or fish: vim.shell names another
workbench-workbench-filter-no-name-here = % goes only in a command without quotes, backticks, $, \, #, <<, (…) or { "{" }…{ "}" }, and not after ~ or a leading =
workbench-workbench-filter-unnameable = % can't name a file whose name has a line break or another control character
workbench-workbench-filter-not-normal = The shell command ended outside normal mode, so the text is kept as it was
workbench-workbench-filter-changed = The text changed while the shell command ran, so it's kept as it was
workbench-workbench-filter-cancelled = Shell command cancelled: nothing changed
workbench-workbench-filter-not-utf8 = The file's name isn't UTF-8, so % can't name it
# The shell couldn't start. $program is the shell, $error why.
workbench-workbench-filter-cant-run = Can't run { $program }: { $error }
# Waiting for the command failed. $error is why.
workbench-workbench-filter-cant-wait = Can't wait for it: { $error }
# The toast while a command runs, with a Cancel button. $line is the command.
workbench-workbench-filter-running = Filtering through { $line }…
workbench-workbench-filter-cancel = Cancel
# A toast: the command failed. $line is the command, $code its exit code.
workbench-workbench-filter-exited = { $line } exited with { $code }
# Vim's message for the same.
workbench-workbench-filter-shell-returned = shell returned { $code }
# $limit is the time limit in milliseconds.
workbench-workbench-filter-timed-out = { $line } ran past { $limit } ms (vim.filter_time_limit): stopped, nothing changed
workbench-workbench-filter-too-much = { $line } wrote more than 64 MiB: stopped, nothing changed
workbench-workbench-filter-vim-off = { $line } ended with vim off: nothing changed
workbench-workbench-filter-stderr = { $line } wrote to stderr

## External formatters: why one failed.

# $program is the formatter's program.
workbench-workbench-format-not-found = { $program }: program not found
# $ms is its time limit in milliseconds.
workbench-workbench-format-timed-out = { $program }: timed out after { $ms } ms
workbench-workbench-format-empty = the formatter's command is empty
# $error is why.
workbench-workbench-format-cant-copy = can't write a temporary copy: { $error }
workbench-workbench-format-cant-read-copy = can't read the temporary copy: { $error }
workbench-workbench-format-not-utf8 = its output isn't UTF-8
workbench-workbench-format-no-output = it gave no output
# $code is its exit status.
workbench-workbench-format-exited = exited with status { $code }
workbench-workbench-format-killed = killed by a signal
# $limit is the limit in MiB.
workbench-workbench-format-too-much = it wrote more than { $limit } MiB
workbench-workbench-format-not-a-file = it isn't a regular file any more
workbench-workbench-format-names-taken = every name is taken
workbench-workbench-format-no-answer = its thread ended without an answer
# $path is the folder the formatter was to run in.
workbench-workbench-format-no-directory = no such directory: { $path }
workbench-workbench-format-not-runnable = it's on the PATH, but isn't a program this user may run

## Plugin formatters.

workbench-workbench-plugin-format-no-thread = no thread to ask it on: { $error }
workbench-workbench-plugin-format-dropped = its request was dropped
# $ms is how long it waited, in milliseconds.
workbench-workbench-plugin-format-timed-out = timed out after { $ms } ms
# $plugin is the plugin's name, $why the reason.
workbench-workbench-plugin-format-failed = Plugin { $plugin }'s formatter failed: { $why }

## Commands plugins invoke.

workbench-workbench-plugin-not-a-command = it isn't a workbench command
workbench-workbench-plugin-no-focus = no workbench element is focused to run it

## Workspace edits (a language server's rename or code action): why one is
## refused. $path is the file's path; $error the system's reason.

workbench-workbench-edit-cant-write = can't write { $path }: { $error }
# The same, after earlier files were written. $written lists them.
workbench-workbench-edit-cant-write-after = can't write { $path }: { $error } ({ $written } already written)
workbench-workbench-edit-outside = { $path } isn't in the project
workbench-workbench-edit-cant-create = can't create { $path }: { $error }
workbench-workbench-edit-exists = { $path } already exists
workbench-workbench-edit-is-folder = { $path } is a folder
workbench-workbench-edit-is-open = { $path } is open
workbench-workbench-edit-cant-overwrite = can't overwrite { $path }: { $error }
workbench-workbench-edit-not-utf8 = can't create { $path }: its name isn't UTF-8
workbench-workbench-edit-cant-trash = can't move { $path } to the Trash: { $error }
workbench-workbench-edit-missing = { $path } doesn't exist
workbench-workbench-edit-cant-rename = can't rename { $path }: { $error }
# $to is where it was to go.
workbench-workbench-edit-other-folder = Tenmoku can't move { $path } to another folder ({ $to })
workbench-workbench-edit-cant-delete = can't delete { $path }: { $error }
workbench-workbench-edit-folder-contents = { $path } is a folder, and the edit doesn't delete what's in it
workbench-workbench-edit-unknown = { $path }: an edit Tenmoku doesn't know
workbench-workbench-edit-saving = { $path } is being saved
workbench-workbench-edit-opening = { $path } is being opened
workbench-workbench-edit-working = { $path } is being worked on
workbench-workbench-edit-closed = { $path } is no longer open
workbench-workbench-edit-overlap = { $path }: its edits overlap the edit's earlier ones
workbench-workbench-edit-changed = { $path } has changed since
workbench-workbench-edit-renamed = { $path } has been saved under another name since
workbench-workbench-edit-cant-resolve = can't resolve { $path }
workbench-workbench-edit-opened = { $path } has been opened since
workbench-workbench-edit-named-twice = { $path } is named twice, by another path
workbench-workbench-edit-cant-read = can't read { $path }: { $error }
# $encoding is the file's encoding, such as UTF-8.
workbench-workbench-edit-invalid = { $path } isn't valid { $encoding }: { $error }
workbench-workbench-edit-cant-encode = { $path } can't be written as { $encoding }: { $error }
workbench-workbench-edit-depends = a file the edit depends on isn't one of its operations

## TODO comments: the picker and ]t / [t.

workbench-workbench-todo-placeholder = Search TODO comments…
workbench-workbench-todo-no-folder = No folder open
workbench-workbench-todo-searching = Searching…
workbench-workbench-todo-none = No TODO comments
workbench-workbench-todo-no-match = No matching TODO comments
# The search stopped at its cap. $rows is the cap, its thousands grouped.
workbench-workbench-todo-capped = Stopped at the first { $rows } matches
workbench-workbench-todo-none-capped = No TODO comments: stopped at the first { $rows } matches
workbench-workbench-todo-no-match-capped = No matching TODO comments: stopped at the first { $rows } matches
# Toasts.
workbench-workbench-todo-no-more = No more TODO comments
workbench-workbench-todo-not-yet = TODO comments aren't known yet: the file is still being parsed
workbench-workbench-todo-too-big = This file is too big to look for TODO comments in

## What a vim map's keys stopped after, inside fe-gpui's
## gpui-vim-map-keys-not-run ("a map's keys after { $what } aren't run").

workbench-workbench-vim-what-quit = a quit
workbench-workbench-vim-what-later = a command that finishes later
