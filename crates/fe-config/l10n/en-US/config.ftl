### fe-config's messages: problems in the settings files.

# A `workbench.locale` that no translation ships, or that isn't a language
# tag. $requested is the setting as written; $chosen is the language the
# text is in instead, the first of the chain negotiated without it.
config-locale-unshipped = No `{ $requested }` translation; using `{ $chosen }`

# A project's settings file sets `workbench.locale`, which only the user's may.
config-project-locale-ignored = workbench.locale in project files is ignored: the language is the same in every window

# A string setting that is only blanks, as serde reports it.
config-value-empty = can't be empty

## Project settings files: keys only the user's file may set.

config-project-lsp-untrusted = LSP settings in project files apply only in trusted folders
config-project-theme-ignored = theme settings in project files are ignored: the theme is the same in every window
config-project-formatters-ignored = formatters in project files are ignored: a folder can't choose the program that formats its files
config-project-extensions-ignored = extensions settings in project files are ignored: they're your own, in every window
config-project-chat-ignored = A project's settings can't set `chat`; set it in your own settings.toml
config-project-agents-ignored = A project's settings can only tighten `agents.permissions` (levels, commands_ask, commands_deny); everything else under `agents` is ignored
config-agents-project-permissions-set = agents.project_permissions can't be set in a file: a project's `agents.permissions` fill it, and you tighten through `agents.permissions`
config-agents-task-budget = must be 0 (off) or at least 20000
config-project-terminal-shell-ignored = terminal.shell and terminal.shell_args in project files are ignored: a folder can't choose the program its terminal runs
config-project-vim-user-only = vim.shell and vim.remap in project files are ignored: a folder can't choose the program ! runs or type keys as you
config-project-icon-theme-ignored = workbench.icon_theme in project files is ignored: the icons are the same in every window

## A `[formatters.<language>]` entry that is dropped. $language is the
## table's name, such as `rust`.

config-formatter-not-table = { $language } must be a table; entry ignored
config-formatter-command-empty = { $language }.command must not be empty; entry ignored
config-formatter-command-not-strings = { $language }.command must be an array of strings: the program, then its arguments; entry ignored
config-formatter-command-missing = { $language }.command is required; entry ignored
config-formatter-stdin-not-bool = { $language }.stdin must be true or false; entry ignored
config-formatter-prefer-unknown = { $language }.prefer must be "external" or "lsp"; entry ignored
config-formatter-timeout-out-of-range = { $language }.timeout_ms must be a whole number from 1 to 60000; entry ignored
config-formatters-not-table = must be a table of languages; ignored

## `editor.spell_check_scopes`. $scope is the entry as written, quotes and
## all.

config-spell-scope-unknown = { $scope } isn't a spell check scope: use "comments", "strings" or "text"; ignored
config-spell-scopes-not-list = must be a list of "comments", "strings" and "text"; ignored

## A settings file the Settings editor couldn't parse. $error is the TOML
## parser's.

config-settings-user-unparsed = user settings: { $error }
config-settings-workspace-unparsed = workspace settings: { $error }

## The Settings editor's writes. $key is a setting's dotted path, such as
## `editor.font_size`.

config-edit-workspace-not-allowed = a project's settings can't set this; set it in your own settings.toml
# $problem says why, such as `must be one of "on", "off"`.
config-edit-invalid = { $key }: { $problem }
config-edit-not-a-number = { $value } isn't a number
# $values are the setting's values, each in quotes, joined with `, `.
config-edit-not-one-of = must be one of { $values }
config-edit-reset-keeps-comment = { $key }: can't be removed without losing a comment; edit settings.toml by hand
config-edit-no-config-dir = there's no config directory

## theme.toml's problems. $key is a dotted key, such as `editor.background`.

# $problem says why, such as config-theme-not-colour's text.
config-theme-key-problem = { $key }: { $problem }
config-theme-unknown-key = { $key }: unknown key
# $name is the dotted name under [syntax].
config-theme-not-capture-name = syntax.{ $name }: not a capture name: expected dot-separated words of a-z, 0-9 and _
# $found is the TOML type written, such as `integer`.
config-theme-not-colour = expected a colour string, found { $found }

## The user's snippet files. $error is the JSON parser's or the system's.

config-snippets-not-object = a snippet file is a JSON object of snippets by name
config-snippets-not-json = not valid JSON: { $error }
# $name is the snippet's name, quoted; $problem one of the messages below.
config-snippet-dropped = snippet { $name } is dropped: { $problem }
config-snippet-not-object = it isn't an object
config-snippet-prefix-not-text = its prefix isn't a string or a list of strings
config-snippet-no-prefix = it has no prefix
config-snippet-no-body = it has no body
config-snippet-body-not-text = its body isn't a string or a list of strings
config-snippet-body-unparsed = its body doesn't parse: { $error }
config-snippet-description-not-text = its description isn't a string
config-snippet-scope-not-text = its scope isn't a string of languages
# $limit is in bytes.
config-snippets-too-large = skipped: larger than { $limit } bytes
config-snippets-not-utf8 = skipped: not UTF-8
config-snippets-unreadable = can't read it: { $error }

## The commented example Configure User Snippets writes into a new snippet
## file, each line after `// `. The example snippet itself stays as code.

config-snippets-example-language = Snippets for { $language }. Each has a name, and:
config-snippets-example-global = Snippets for any language. Each has a name, and:
config-snippets-example-fields =
    - prefix: what's typed to offer it in completions (one, or a list);
    - body: the text it inserts (one string, or a list of lines), where
    {"  "}$1, $2... are tab stops, $0 the final caret, ${"{"}1:label{"}"} a
    {"  "}placeholder and ${"{"}1|one,two|{"}"} a choice;
    - description: what the completion menu and Insert Snippet say.
config-snippets-example-scope = - scope: the languages it's offered in, by comma; every one if none.
config-snippets-example-intro = For example:
