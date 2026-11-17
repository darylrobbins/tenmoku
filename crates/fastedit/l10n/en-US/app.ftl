### fastedit's messages: the shell command, theme settings, the updater, the
### speller and grammar installs.

## The `tenmoku` shell command (Install and Uninstall).

# $path is the link's path.
fastedit-shell-exists-not-ours = { $path } exists and isn't a link to tenmoku
fastedit-shell-install-failed = Couldn't install the 'tenmoku' command: { $error }
fastedit-shell-linked = Linked { $link } to { $target }
# $dir is the folder; $line is the shell line that puts it on PATH.
fastedit-shell-not-on-path = { $dir } isn't on your PATH. Add this to your shell config: { $line }
fastedit-shell-not-installed = { $path } isn't installed
fastedit-shell-removed = Removed { $path }
fastedit-shell-remove-failed = Couldn't remove { $path }: { $error }
fastedit-shell-remove-not-ours = Not removing { $path }: it isn't a link to tenmoku
fastedit-shell-no-home = HOME isn't set
fastedit-shell-no-binary = Couldn't find the running binary: { $error }

## The `theme.name` setting.

# $themes is the bundled themes' titles, joined with commas.
fastedit-theme-not-bundled = theme { $name } isn't bundled: the bundled themes are { $themes }
fastedit-theme-name-ignored = theme.name { $title } is ignored: theme.{ $appearance } chooses the { $appearance } theme

## Grammar installs. $language is a language's name; $error another
## message's text.

fastedit-grammar-file-not-wasm = { $language }'s file { $file } isn't a .wasm file's name
fastedit-grammar-wasm-not-wasm = { $language }'s wasm { $file } isn't a .wasm file's name
fastedit-grammar-manifest-error = { $language }'s extension manifest: { $error }
fastedit-grammar-manifest-no-grammar = { $language }'s extension manifest has no grammar for it
fastedit-grammar-manifest = the grammar manifest: { $error }
fastedit-grammar-unknown-language = there's no first-party grammar for { $language }
fastedit-grammar-bundle = the downloaded grammars: { $error }
fastedit-grammar-removed = { $language }, which it injects, was removed while it downloaded: try again
# An index entry refused before anything is written: `$why` is one of the
# fastedit-grammar-file-not-allowed … fastedit-grammar-files-differ messages.
fastedit-grammar-refused = { $language }'s download from the grammar index: { $why }
fastedit-grammar-file-not-allowed = it has a file { $file } that isn't one an extension's grammar may have
fastedit-grammar-no-manifest = it has no extension.toml
fastedit-grammar-manifest-other-id = its extension.toml is for { $id }
fastedit-grammar-manifest-grammars =
    { $count ->
        [one] its extension.toml has { $count } grammar, not one
       *[other] its extension.toml has { $count } grammars, not one
    }
fastedit-grammar-manifest-other-language = its extension.toml's grammar is for { $other }
fastedit-grammar-manifest-other-wasm = its extension.toml names the wasm { $wasm }, which isn't its download's
fastedit-grammar-files-differ = its files aren't the ones the index lists
fastedit-grammar-io = { $path }: { $error }
fastedit-grammar-not-restored = { $path }: { $error }; the previous version couldn't be put back ({ $restoring }) and is at { $previous }
# A warning toast: $name is the language's name; $waiting the language it
# uses that hasn't loaded.
fastedit-grammar-injected-slow = Installed { $name } syntax, but { $waiting }, which it uses, hasn't loaded: it may after a restart
fastedit-grammar-offline = no connection to the download
fastedit-grammar-hash-mismatch = the download didn't match its recorded hash
fastedit-grammar-bundle-short = it's cut short
fastedit-grammar-bundle-name = a language's name isn't UTF-8
fastedit-grammar-bundle-other = it isn't { $language }'s

## `vim.remap` problems and vimrc maps the keymap hides.

fastedit-vim-leader-unreadable = vim.leader can't be read
fastedit-vim-read-failed = cannot read the file: { $error }
fastedit-vim-read-not-regular = cannot read the file: not a regular file
fastedit-vim-read-too-big = cannot read the file: bigger than { $kib } KiB
fastedit-vim-read-not-utf8 = cannot read the file: it isn't valid UTF-8
# $from is the map's left side, $keys the binding's keys, $command its command.
fastedit-vim-conflict-cmdline = "{ $from }" is bound in the keymap in the command line ("{ $keys }" runs { $command }); the keymap wins there
fastedit-vim-conflict-insert = "{ $from }" is bound in the keymap in insert mode ("{ $keys }" runs { $command }); the keymap wins there
