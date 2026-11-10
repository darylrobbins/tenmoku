### kiln-core's extension messages: problems in an extension's manifest
### and files, and what the user is told of its licence. Quoted arguments
### (`$id`, `$version`, `$entry`, `$expr`, `$written`) are quoted as
### written; `$reason` is an error's own text.

## The manifest.

# `$file` is the manifest's file name.
kiln-core-extension-no-manifest = no { $file } in the extension's directory
kiln-core-extension-plugins-unsupported = code plugins aren't supported by this app
kiln-core-extension-version-invalid = { $version } isn't a version: expected MAJOR.MINOR.PATCH, such as 1.0.0
kiln-core-extension-id-invalid = { $id } isn't an extension ID: { $reason }
# The reason an ID with one segment isn't an extension ID.
kiln-core-extension-id-needs-segments = needs two or more segments, as in `acme.solarized`
kiln-core-extension-id-reserved = { $id } starts with `{ $namespace }`, which is reserved for the app's own commands
kiln-core-extension-license-invalid = { $expr } isn't an SPDX licence expression: { $reason }
# `$id` is the extension's ID, `$expr` its licence expression.
kiln-core-extension-license-not-allowed = { $id }: licence { $expr } isn't in extensions.allowed_licenses
kiln-core-extension-license-not-allowed-unnamed = this extension: licence { $expr } isn't in extensions.allowed_licenses

## The manifest's [engine] table. `$app` is the app's name in the table,
## `$version` its extension API version, `$needed` the one written.

kiln-core-extension-engine-missing = missing [engine]: the app and extension API version this extension needs, as in `[engine] { $app } = "{ $version }"`
kiln-core-extension-engine-other-app = this extension is for { $other }, not this app ({ $app })
kiln-core-extension-engine-no-app = [engine] names no app, as in `[engine] { $app } = "{ $version }"`
kiln-core-extension-engine-expected-string = expected a string, such as "{ $version }", found { $found }
kiln-core-extension-api-version-invalid = { $written } isn't an API version: expected MAJOR.MINOR, such as { $version }
kiln-core-extension-api-version-unmet = needs extension API { $needed }; this app has { $version }

## Contributed files. `$kind` is a contribution's kind, such as `themes`.

kiln-core-extension-workspace-contributes = workspace extensions can't contribute { $kind }, so `contributes.{ $kind }` is ignored
kiln-core-extension-too-many-files = contributes { $count } { $count ->
        [one] file
       *[other] files
    }; an extension may contribute at most { $max }
kiln-core-extension-expected-path = expected a path, found { $found }
kiln-core-extension-path-empty = the path is empty
kiln-core-extension-path-backslash = use / in extension paths
kiln-core-extension-path-absolute = { $entry } must be relative to the extension's directory
kiln-core-extension-path-outside = { $entry } is outside the extension's directory
kiln-core-extension-path-unreadable = { $entry } can't be read: { $reason }
# `$target` is where a link in the path leads.
kiln-core-extension-path-leads-outside = { $entry } leads to { $target }, outside the extension's directory
kiln-core-extension-path-not-a-file = { $entry } isn't a file
kiln-core-extension-path-missing = { $entry } doesn't exist

## The extensions directories, the user's and a workspace's.

kiln-core-extension-dir-unreadable = can't read the extensions directory: { $reason }
# `$dir` is the directory of the extension that doesn't load.
kiln-core-extension-id-taken = `{ $id }` is also the ID of the extension in { $dir }, which loads instead
# An extension in both the user's and a workspace's directory. `$owner`
# has the one that doesn't load, `$winner` the one that does: `user` or
# `workspace`.
kiln-core-extension-shadowed = { $owner ->
        [workspace] the workspace's
       *[user] the user's
    } { $id } in { $dir } isn't loaded: { $winner ->
        [workspace] the workspace's
       *[user] the user's
    } copy in { $winner_dir } loads instead

## Licences.

# What the user is told of an extension they installed that isn't
# permissively licensed. `$name` and `$id` are the extension's, `$license`
# its licence expression, `$app` the app's name.
kiln-core-extension-license-notice = { $name } ({ $id }) is licensed { $license }. { $app } doesn't ship it; you installed it.
# Comment lines at the top of the file that records those notices, each
# starting `# `, above its `acknowledged` list.
kiln-core-extension-acknowledged-header =
    # The extensions you installed, as id@version, whose licence you've been
    # told of. Remove an entry to be told again.

## Why a licence expression isn't SPDX. `$word` is a word of it, `$ch` a
## character (quoted), `$token` what was found: a word in backticks, or
## kiln-core-spdx-end.

kiln-core-spdx-plus-alone = `+` must follow a licence ID directly
kiln-core-spdx-plus-run-on = `{ $word }` must be followed by a space, a parenthesis or the end
kiln-core-spdx-character = { $ch } can't be in a licence expression
kiln-core-spdx-not-an-id = `{ $word }` isn't a licence ID
kiln-core-spdx-reference-plus = `{ $word }+`: a licence reference takes no `+`
kiln-core-spdx-too-long = it's longer than { $max } bytes
kiln-core-spdx-empty = it's empty
kiln-core-spdx-unexpected = unexpected { $token }
kiln-core-spdx-end = end
kiln-core-spdx-too-deep = parentheses nest more than { $max } deep
kiln-core-spdx-expected-close = expected `)`, found { $token }
kiln-core-spdx-expected-exception = expected an exception ID after WITH, found { $token }
kiln-core-spdx-expected-id = expected a licence ID, found { $token }
