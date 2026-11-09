# fe-project's messages.

project-name-empty = a name is required
project-name-dot = `.` is not a name
project-name-dot-dot = `..` is not a name
project-name-nul = a name can't hold a NUL
project-name-empty-component = a name can't have an empty part between `/`s
project-name-separator = a name can't hold a `/` here

project-search-invalid-glob = invalid glob `{ $glob }`: { $message }
project-search-invalid-pattern = invalid pattern: { $message }

project-ignore-cannot-reinclude = cannot re-include `{ $pattern }`: its parent directory `{ $dir }` is excluded by line { $line }

## Detail text of I/O errors and pattern diagnostics shown to the user.

project-io-not-resolved-path = not a resolved path
project-io-name-exists = something by that name already exists
project-io-symlink-outside-root = a symlink, which could lead outside the project root
project-io-not-regular-file = not a regular file
project-io-no-temp-name = no free temporary name to rename through
project-io-path-not-utf8 = a path that isn't UTF-8
project-io-no-file-reference = no file reference
project-io-path-no-longer-leads = the path no longer leads to the entry found under the project root
project-io-no-trash = no Trash on this system
project-io-no-home = no home folder
project-io-trash-not-yours = the volume's trash belongs to someone else
project-io-no-trash-name = no free name in the trash
project-io-copy-into-itself = a folder can't be copied into itself
project-io-no-copy-name = no free name for a copy
project-io-move-into-itself = a folder can't be moved into itself
project-io-copy-folder-replaced = a folder of the copy was replaced while it was made
project-io-duplicate-kind = only files, folders and symlinks can be duplicated
project-io-entry-in-new-copy = an entry appeared in a new copy
project-io-project-root-itself = the project root itself
project-io-path-outside-root = a path outside the project root
project-io-dotdot-in-root-path = `..` in a path under the project root
project-io-symlinked-folder-outside-root = a symlinked folder, which could lead outside the project root
project-io-file-for-folder = a file where a folder should be
project-io-symlink-copy-windows = copying a symlink isn't supported on Windows
project-io-folder-replaced = a folder was replaced while it was worked on
project-io-folder-unreadable = a folder its owner can't read, which can't be opened to change that
project-io-rename-may-replace = this file system can't rename without the risk of replacing
project-io-not-plain-path = not a plain path
project-io-dotdot-under-root = `..` in a path under a root
project-io-absolute-under-root = an absolute path under a root
project-io-no-name-under-root = no file named under a root
project-io-symlink-or-file-on-way = a symlink or a file on the way
project-io-symlink = a symlink
project-io-not-absolute = not an absolute path
project-io-watched-replaced = replaced while being watched
project-io-not-a-name = not a name
project-io-separator-in-name = a separator in a name
project-io-name-too-long = a name too long
project-io-directory = a directory
project-io-same-file-link = the name is another link to the same file
project-io-recycle-cancelled = moving to the Recycle Bin was cancelled
project-docker-illegal-exclusion = illegal exclusion pattern
project-docker-pattern-syntax = syntax error in pattern
project-io-mount-unknown = can't tell which mount the entry is on: { $detail }
project-io-mount-not-procfs = /proc is not a procfs
project-io-mount-fdinfo-other-file = fdinfo for another file
project-io-mount-no-mount-id = no mount id in fdinfo
project-io-listing-replaced = { $path } was replaced while listing
project-io-listing-unreachable = { $path } couldn't be reached while listing
project-io-not-directory = { $path } is not a directory
project-io-recycle-failed = couldn't move { $path } to the Recycle Bin (error { $code })
project-ignore-unknown-class = unknown character class `[:{ $name }:]`
project-search-unclosed-class = unclosed character class
