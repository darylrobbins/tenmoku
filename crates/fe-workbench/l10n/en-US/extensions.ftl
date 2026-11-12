# fe-workbench's extensions, plugins, updates and trust strings (i18n
# design, W5): `workbench-extensions-…`.

## Workspace trust (trust.rs)

workbench-extensions-trust-question = Do you trust the authors of the files in this folder?
workbench-extensions-trust-detail =
    { $folder }

    Trusting this folder lets Tenmoku start language servers in it, which can run the project's code: rust-analyzer runs build scripts and proc macros. Its subfolders are trusted too. Change this later with File > Trust This Folder or Untrust This Folder.

## Check for Updates (update_check.rs)

workbench-extensions-update-checking = Checking for updates…
workbench-extensions-update-already-checking = Already checking for updates
workbench-extensions-update-no-release = No release was found on GitHub
workbench-extensions-update-no-release-status = No release was found on GitHub (HTTP { $status })
workbench-extensions-update-failed = Couldn't check for updates: { $reason }
workbench-extensions-update-download = Download
workbench-extensions-update-up-to-date = Tenmoku is up to date (you have { $running })
workbench-extensions-update-available = Tenmoku { $version } is available (you have { $running })
workbench-extensions-update-refused = GitHub refused it, usually its rate limit, so try again later (HTTP { $status })
workbench-extensions-update-http-status = GitHub answered HTTP { $status }
workbench-extensions-update-invalid-json = GitHub's reply isn't valid JSON (HTTP { $status })
workbench-extensions-update-version-not-comparable = this build's version, { $running }, can't be compared
workbench-extensions-update-no-get = no GET method
workbench-extensions-update-broke-off = GitHub's reply broke off: { $error } (HTTP { $status })
workbench-extensions-update-too-long = GitHub's reply is too long (HTTP { $status })

## Shared by the prompts' buttons. An answer is matched by its button's
## index, never its title.

workbench-extensions-allow = Allow
workbench-extensions-dont-allow = Don't Allow
workbench-extensions-cancel = Cancel

## A plugin's grant prompt (plugin_prompt.rs). The detail's paragraphs are
## each a message; capability lines are indented by the code.

workbench-extensions-plugin-allow-full-access = Allow Full Access
workbench-extensions-plugin-prompt-header =
    Plugin: { $id } { $version }, licensed { $license }
    From: { $dir }
workbench-extensions-plugin-asks-to = It asks to:
workbench-extensions-plugin-load-question = Allow the plugin { $name } to load?
workbench-extensions-plugin-adds = This version asks for more than you granted. It adds:
workbench-extensions-plugin-keeps = It keeps:
workbench-extensions-plugin-more-access-question = Allow { $name } { $version } more access?
workbench-extensions-plugin-warnings = Warnings:
workbench-extensions-plugin-full-access-note = It asks for full access: after Allow, you'll be asked to accept that on its own.
workbench-extensions-plugin-install-footer = It doesn't load until you allow it. Don't Allow keeps it from loading, and isn't asked again until it asks for something else.
workbench-extensions-plugin-escalation-footer = The installed version keeps running with what you granted it until you allow this. Don't Allow keeps it running as it is, and isn't asked again until the request changes.
workbench-extensions-plugin-escalation-footer-version = The installed version, { $version }, keeps running with what you granted it until you allow this. Don't Allow keeps it running as it is, and isn't asked again until the request changes.
workbench-extensions-plugin-warning-interpreter = { $program } runs through { $interpreter }, which reads its arguments again.
workbench-extensions-plugin-full-access-question = Give { $name } full access?
workbench-extensions-plugin-full-access-asks-for = { $id } { $version } asks for:
workbench-extensions-plugin-full-access-footer = Only Allow Full Access grants it. Don't Allow grants nothing it asks for.
# A capability and what it lets the plugin do: `net:example.com: connect
# to example.com`.
workbench-extensions-plugin-capability = { $capability }: { $description }
# `lsp:<server>`'s description in the prompt, by whether fastedit ships the
# server. `$server` is the server's name.
workbench-extensions-plugin-lsp-built-in = send requests to { $server }, a built-in language server
workbench-extensions-plugin-lsp-not-built-in = send requests to { $server }, a language server that is not built in

## An extension's language server's consent prompt (lsp_consent_prompt.rs).
## The detail's paragraphs and lines are each a message; what the server
## runs is indented by the code.

workbench-extensions-lsp-consent-question = Allow { $name } to run a language server?
workbench-extensions-lsp-consent-header =
    Extension: { $id } { $version }, licensed { $license }
    From: { $dir }
    Language server: { $server } ({ $languages })
workbench-extensions-lsp-consent-changed =
    This extension changed what it runs.
    Before:
workbench-extensions-lsp-consent-now = Now:
workbench-extensions-lsp-consent-interpreter = { $program } can run any code in its arguments.
workbench-extensions-lsp-consent-script = Script: { $path }
workbench-extensions-lsp-consent-footer = It runs with your permissions, outside any sandbox. Don't Allow keeps it from running and isn't asked again until the command changes.
workbench-extensions-lsp-consent-runs = Runs: { $command }
workbench-extensions-lsp-consent-argument = Argument: { $arg }
workbench-extensions-lsp-consent-environment = Environment: { $name }={ $value }

## The code plugins in Extensions: Show Installed, revoking and
## uninstalling them (plugin_manage.rs)

workbench-extensions-plugin-uninstall = Uninstall
workbench-extensions-plugin-stopped = Stopped: { $diagnostic }
workbench-extensions-plugin-full-access = Full access
workbench-extensions-plugin-full-access-detail = Full access: it holds net:* or process.spawn:*
# `$capabilities` is a list of capabilities, each with what it lets the
# plugin do, joined by "; ".
workbench-extensions-plugin-row-asks = Asks to: { $capabilities }
workbench-extensions-plugin-row-granted = Granted: { $capabilities }
workbench-extensions-plugin-row-asks-title = Asks to:
workbench-extensions-plugin-granted = Granted:
workbench-extensions-plugin-nothing = nothing
workbench-extensions-plugin-needs-preview = Turn on { $setting } in the settings to load it
workbench-extensions-plugin-cant-revoke = Can't revoke { $name }'s grants
workbench-extensions-plugin-revoked = Revoked { $name }'s grants
workbench-extensions-plugin-revoked-detail = It's stopped, and asks for them again the next time it loads.
workbench-extensions-plugin-uninstall-question = Uninstall the plugin { $name }?
workbench-extensions-plugin-uninstall-detail =
    { $id } { $version }
    From: { $dir }

    Its stored secrets are deleted from the keychain, what you granted it is forgotten, and its folder is removed. This can't be undone.
workbench-extensions-plugin-cant-uninstall = Can't uninstall { $name }
workbench-extensions-plugin-uninstalled = Uninstalled { $name }
workbench-extensions-plugin-secrets-kept = Its stored secrets couldn't be deleted ({ $error }), so it stays installed.
workbench-extensions-plugin-manifest-unreadable = { $path } can't be read: { $error }
workbench-extensions-plugin-replaced = { $dir } no longer holds it: it was replaced while you were asked.
workbench-extensions-plugin-busy = It's being revoked or uninstalled already.
workbench-extensions-plugin-no-state-dir = There's no state directory to keep plugin grants in.

## Shared by the extension files

# A file or folder and what went wrong with it.
workbench-extensions-path-error = { $path }: { $error }
workbench-extensions-state = State: { $state }

## Extensions: Show Installed (extensions_picker.rs)

workbench-extensions-ok = OK
workbench-extensions-state-active = Active
workbench-extensions-state-needs-consent = Needs consent
workbench-extensions-state-declined = Declined
workbench-extensions-state-disabled = Disabled
workbench-extensions-state-skipped-untrusted = Skipped (untrusted)
workbench-extensions-state-running = Running
workbench-extensions-state-waiting-for-grant = Waiting for grant
workbench-extensions-state-stopped = Stopped
workbench-extensions-state-needs-plugins-preview = Needs plugins preview
workbench-extensions-state-starting = Starting
# A language server of an extension's, what it runs, and its consent.
workbench-extensions-server-pending = { $server }: { $invocation } (pending)
workbench-extensions-server-declined = { $server }: { $invocation } (declined)
workbench-extensions-server-granted = { $server }: { $invocation } (granted)
workbench-extensions-server-granted-shadowed = { $server }: { $invocation } (granted; not running: an earlier extension's server of this name runs)
# An extension's state, then its plugin's, where that's another.
workbench-extensions-label-with-plugin = { $state } · Plugin: { $plugin }
workbench-extensions-label-full-access = { $label } · Full access
workbench-extensions-license-permissive = { $license } (permissive)
workbench-extensions-license-copyleft = { $license } (copyleft)
workbench-extensions-license-unknown = { $license } (unknown)
workbench-extensions-linked-from = Linked from { $folder }
workbench-extensions-source-user = User
workbench-extensions-source-workspace = Workspace
workbench-extensions-detail-id = ID: { $id }
workbench-extensions-detail-version = Version: { $version }
workbench-extensions-detail-contributes = Contributes: { $kinds }
workbench-extensions-detail-licence = Licence: { $license }
workbench-extensions-detail-source = Source: { $source }
workbench-extensions-detail-directory = Directory: { $dir }
workbench-extensions-detail-servers = Language servers:
workbench-extensions-detail-plugin = Plugin:
# What an extension contributes, with counts, joined by ", ": "1 theme,
# 2 lsp servers".
workbench-extensions-kind-themes =
    { $count ->
        [one] { $count } theme
       *[other] { $count } themes
    }
workbench-extensions-kind-keymaps =
    { $count ->
        [one] { $count } keymap
       *[other] { $count } keymaps
    }
workbench-extensions-kind-menus =
    { $count ->
        [one] { $count } menu
       *[other] { $count } menus
    }
workbench-extensions-kind-lsp-servers =
    { $count ->
        [one] { $count } lsp server
       *[other] { $count } lsp servers
    }
workbench-extensions-kind-grammars =
    { $count ->
        [one] { $count } grammar
       *[other] { $count } grammars
    }
workbench-extensions-kind-snippet-files =
    { $count ->
        [one] { $count } snippet file
       *[other] { $count } snippet files
    }
workbench-extensions-kind-none = no contributions

## Extensions: Install from File or Folder (extension_install.rs)

workbench-extensions-install-replace = Replace
workbench-extensions-install-button = Install
workbench-extensions-install-unsupported = Pick a .zip, .tar.gz or .tgz file, or a folder
# `$dirs` is the folders kept, joined by ", ".
workbench-extensions-install-kept-aside = { $error }. The installed version couldn't be put back: it's kept in { $dirs }
workbench-extensions-install-extensions-folder = It's the folder the extensions are installed in, or holds it
workbench-extensions-install-taken = { $dir } is there already, and isn't { $id }
workbench-extensions-install-no-staging-dir = no fresh directory to unpack into
workbench-extensions-install-duplicate-names = It has two entries with one name
workbench-extensions-install-header-too-long = It has a long-name or pax header over { $kib } KiB
workbench-extensions-install-pax-size = It has a pax header giving an entry { $declared } bytes, and the entry's own header { $size }
workbench-extensions-install-pax-unreadable = It has a pax header that can't be read
# An archive's entry, by its name, that isn't installed.
workbench-extensions-install-not-utf8 = { $name } isn't named in UTF-8
workbench-extensions-install-symlink = { $name } is a symbolic link
workbench-extensions-install-hard-link = { $name } is a hard link
workbench-extensions-install-special = { $name } isn't a file or a folder
workbench-extensions-install-absolute = { $name } is an absolute path
workbench-extensions-install-outside = { $name } leads outside the extension
workbench-extensions-install-no-manifest = There's no { $file } at its top level, or in a single folder there
workbench-extensions-install-bad-id = { $id } isn't an extension ID
# A manifest's error: `extension.toml:3:11: version: …`.
workbench-extensions-install-manifest-error = { $file }:{ $line }:{ $column }: { $message }
workbench-extensions-install-manifest-key-error = { $file }:{ $line }:{ $column }: { $key }: { $message }
workbench-extensions-install-unknown-version = unknown
workbench-extensions-install-too-big = It unpacks to more than { $mib } MiB
workbench-extensions-install-too-many = It has more than { $max } entries
workbench-extensions-install-bad-zip = It isn't a zip archive that can be read: { $error }
workbench-extensions-install-replace-question = Replace { $name } { $old } with { $new }?
workbench-extensions-install-replace-detail = { $id } { $old } is installed in { $dir }. Replacing it removes that folder.
workbench-extensions-install-installed = Installed { $name } { $version }
workbench-extensions-install-plugin-asks = It asks before it runs
workbench-extensions-install-plugin-needs-preview = Turn on { $setting } to run it
workbench-extensions-install-cant-install = Can't install an extension
workbench-extensions-install-no-state-dir = There's no state directory to install extensions in.
workbench-extensions-install-cant-install-from = Can't install { $name }

## Extensions: Install Folder for Development (plugin_dev.rs)

workbench-extensions-dev-button = Install for Development
workbench-extensions-dev-pick-folder = Pick a folder
workbench-extensions-dev-extensions-folder = It's the folder the extensions are installed in, or in it, or holds it
workbench-extensions-dev-no-manifest = There's no { $file } at its top level
workbench-extensions-dev-taken = { $dir } is there already, and isn't a link to this folder
workbench-extensions-dev-junction-failed = Couldn't link it: symbolic links aren't allowed here, and a junction failed too: { $error }
workbench-extensions-dev-junction-network = Couldn't link { $folder }: symbolic links aren't allowed here, and a junction works on local volumes only, not a network one. Move the folder to a local drive
workbench-extensions-dev-junction-name = A name with %, a quote, ^ or ! can't be made a junction
workbench-extensions-dev-installed-elsewhere = { $id } is installed already, in { $dir }: uninstall it first
workbench-extensions-dev-installed = Installed { $name } { $version } for development
workbench-extensions-dev-linked = Linked from { $folder }: it reloads when it's rebuilt.
workbench-extensions-dev-linked-plugin = Linked from { $folder }: it reloads when it's rebuilt. It asks before it runs.
workbench-extensions-dev-linked-plugin-needs-preview = Linked from { $folder }: it reloads when it's rebuilt. Turn on { $setting } to run it.

## The offer to install a language's syntax highlighting (grammar_prompt.rs)

workbench-extensions-grammar-offer = Syntax highlighting for { $name } isn't installed
# The Install button, with the download's size (`workbench-extensions-grammar-size`).
workbench-extensions-grammar-install = Install ({ $size })
workbench-extensions-grammar-size = { $size } MB
workbench-extensions-grammar-not-now = Not now
# Never offer it again for files called `$name`, such as go.mod.
workbench-extensions-grammar-never-file = Never for { $name }
# Never offer it again for files with this extension, such as .kt.
workbench-extensions-grammar-never-extension = Never for .{ $extension }
workbench-extensions-grammar-installing = Installing { $name } syntax…
workbench-extensions-grammar-not-loaded = Installed { $name } syntax, but it hasn't loaded: it may after a restart
workbench-extensions-grammar-installed = Installed { $name } syntax
workbench-extensions-grammar-failed = Couldn't install { $name } syntax: { $reason }.
workbench-extensions-grammar-failed-offline = Couldn't install { $name } syntax: { $reason }. The language pack installs it without a connection.

## Extensions' language servers' consents (lsp_consent.rs)

workbench-extensions-lsp-consent-unreadable = can't read { $path }: { $error }
workbench-extensions-lsp-consent-changed-since = { $server } isn't started: what it runs changed since you allowed it
workbench-extensions-lsp-consent-not-found = { $command } not found

## Snippets: extensions' and the user's (extension_snippets.rs,
## user_snippets.rs). `$at` is a byte offset in the snippet's body.

workbench-extensions-user-snippet = User snippet
workbench-extensions-snippet-no-prefix = it has no prefix
workbench-extensions-snippet-no-body = it has no body
workbench-extensions-snippet-too-deep = its placeholders nest more than { $depth } deep
workbench-extensions-snippet-unclosed = a `${ "{" }` is never closed
workbench-extensions-snippet-tabstop-too-large = the tabstop number at byte { $at } is too large
workbench-extensions-snippet-names-nothing = the `${ "{" }` at byte { $at } names no tabstop or variable
workbench-extensions-snippet-brace-unclosed = the `${ "{" }` at byte { $at } is never closed
workbench-extensions-snippet-brace-unmatched = the `${ "{" }` at byte { $at } isn't closed by a `{ "}" }`
workbench-extensions-snippet-choice-unclosed = the choice at byte { $at } is never closed
workbench-extensions-snippet-choice-unmatched = the choice at byte { $at } isn't closed by `|{ "}" }`
workbench-extensions-snippet-transform-unclosed = the transform at byte { $at } is never closed

## What the editor answers a plugin's call it can't carry out
## (plugin_bridge.rs)

workbench-extensions-bridge-closed = the editor has closed
workbench-extensions-bridge-unfinished = the editor didn't finish before the call's deadline
workbench-extensions-bridge-unanswered = the editor didn't answer before the call's deadline
workbench-extensions-bridge-no-window = no window is active
workbench-extensions-bridge-window-closed = the window closed
workbench-extensions-bridge-no-editor = no editor is active
workbench-extensions-bridge-saving = a save of the buffer is under way
workbench-extensions-bridge-not-run = the active editor didn't run it
workbench-extensions-bridge-no-buffer = no open buffer has id { $buffer }
workbench-extensions-bridge-stale-version = buffer { $buffer } is at version { $now }, not { $version }

## A plugin's log (plugin_log.rs) and its status items (plugin_paint.rs)

# The log's tab title: `$name` is the plugin's.
workbench-extensions-plugin-log-title = { $name } Log
workbench-extensions-paint-no-plugin = no plugin is loaded

## The plugin host (plugin_host.rs): why a plugin stopped, its log's
## lifecycle lines, and what it's told of a call it made wrong

# Why a plugin stopped, and what it says of it: `trapped: its thread
# stopped`.
workbench-extensions-plugin-diagnostic = { $reason }: { $message }
workbench-extensions-plugin-stop-trap = trapped
workbench-extensions-plugin-stop-timeout = timed out
workbench-extensions-plugin-stop-memory = out of memory
workbench-extensions-plugin-stop-protocol = protocol error
workbench-extensions-plugin-stop-load = failed to load
workbench-extensions-plugin-stop-grants = grants unavailable
workbench-extensions-plugin-stop-unloaded = unloaded
workbench-extensions-plugin-rebuild = the plugin API is a preview and can change in any release; rebuild the plugin against this release
# A plugin that doesn't link against this host's world: why, then what to do.
workbench-extensions-plugin-unlinked = { $error }; { workbench-extensions-plugin-rebuild }
workbench-extensions-plugin-unloaded = plugin { $id } was unloaded
workbench-extensions-plugin-args-none = takes no args
workbench-extensions-plugin-args-table = args must be a table
workbench-extensions-plugin-args-null = args can't hold a null
workbench-extensions-plugin-args-missing = args need `{ $key }`
workbench-extensions-plugin-grants-no-state-dir = no state directory to keep its grants in
workbench-extensions-plugin-too-large = { $path } is over the { $limit }-byte limit
workbench-extensions-plugin-shut-down = shut down
workbench-extensions-plugin-shut-down-uninstalled = shut down: it's no longer installed
workbench-extensions-plugin-shut-down-preview-off = shut down: the plugins preview is off
workbench-extensions-plugin-loaded = loaded { $version }
workbench-extensions-plugin-reloaded = reloaded { $version }
workbench-extensions-plugin-log-stopped = stopped: { $diagnostic }
workbench-extensions-plugin-answer-unrecorded = can't record the answer to its prompt: { $message }
workbench-extensions-plugin-thread-stopped-loading = its thread stopped while loading it
workbench-extensions-plugin-thread-stopped = its thread stopped
workbench-extensions-plugin-still-loading = the plugin is still loading
workbench-extensions-plugin-no-export = no export { $interface }#{ $name }
workbench-extensions-plugin-shutdown-failed = its shutdown failed: { $diagnostic }
# `$deadline` is a duration, such as `2s`.
workbench-extensions-plugin-deadline = a call ran past its { $deadline } deadline
workbench-extensions-plugin-no-ticker = no thread to enforce plugins' deadlines: { $error }
workbench-extensions-plugin-load-cancelled = its load was cancelled
