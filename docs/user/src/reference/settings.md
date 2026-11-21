# Settings reference

> Generated; do not edit. `cargo run -p fastedit --example gen_user_docs` writes this page from the settings schema (`fe_config::settings_schema()`).

Every key `settings.toml` takes, by table: its type, its default, the values it takes and what it does. `<name>` stands for a name you choose, such as a language server's.

## `agents`

AI agents. A project file can only tighten their permissions.

### `agents.command_timeout_secs`

How long one command an agent runs may take, in seconds.

- Type: integer, 1 to 3600
- Default: `120`

### `agents.max_rounds`

The most requests one turn makes.

- Type: integer, 1 to 200
- Default: `25`

### `agents.task_budget`

The most tokens a turn's task may use, 0 for no budget, else at least 20000.

- Type: integer, `0`, or at least 20000
- Default: `0`

### `agents.tool_loop`

The API chat offers Claude tools it runs for you, in trusted folders only.

- Type: boolean
- Default: `true`

### `agents.turn_time_limit_secs`

How long one turn may run, in seconds.

- Type: integer, 30 to 7200
- Default: `600`

## `agents.claude_code`

The Claude Code agent. A project file can't set any of it.

### `agents.claude_code.adapter`

The path of the claude-agent-acp adapter. Empty finds it on your login shell's PATH.

- Type: string
- Default: `""`

### `agents.claude_code.claude`

The path of the claude program. Empty finds it on your login shell's PATH.

- Type: string
- Default: `""`

### `agents.claude_code.mcp_servers`

Let Claude Code run the MCP servers its own settings configure.

- Type: boolean
- Default: `false`

### `agents.claude_code.sandbox_allow_write`

Extra paths Claude Code's sandbox may write.

- Type: array of string
- Default: `[]`

## `agents.permissions`

What an agent may do without asking, by kind of action.

### `agents.permissions.commands_allow`

Command patterns that run without asking.

- Type: array of string
- Default: `[]`

### `agents.permissions.commands_ask`

Command patterns that always ask.

- Type: array of string
- Default: `[]`

### `agents.permissions.commands_deny`

Command patterns that never run.

- Type: array of string
- Default: `[]`

### `agents.permissions.fetch`

Fetching from the network.

- Type: string
- Default: `"ask"`
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.permissions.other`

Any other action.

- Type: string
- Default: `"ask"`
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.permissions.propose_edit`

Proposing an edit, which you review before it applies.

- Type: string
- Default: `"allow"`
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.permissions.read`

Reading files in the folder.

- Type: string
- Default: `"allow"`
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.permissions.run_command`

Running a command.

- Type: string
- Default: `"ask"`
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.permissions.search`

Searching the folder.

- Type: string
- Default: `"allow"`
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

## `agents.project_permissions`

Permissions a project file asked for: each can only tighten what `permissions` allows, so the effective level is the stricter of the two. A project file's `agents.permissions` is moved here.

### `agents.project_permissions.commands_ask`

Command patterns that always ask, added to yours.

- Type: array of string
- Default: `[]`

### `agents.project_permissions.commands_deny`

Command patterns that never run, added to yours.

- Type: array of string
- Default: `[]`

### `agents.project_permissions.fetch`

Fetching from the network.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.project_permissions.other`

Any other action.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.project_permissions.propose_edit`

Proposing an edit.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.project_permissions.read`

Reading files in the folder.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.project_permissions.run_command`

Running a command.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

### `agents.project_permissions.search`

Searching the folder.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"allow"`: Do it without asking.
  - `"ask"`: Ask first.
  - `"deny"`: Never.

## `chat`

The AI chat.

### `chat.context_budget`

The most tokens of attached context (files, selection, diagnostics) one message sends.

- Type: integer, 0 to 500000
- Default: `32000`

### `chat.effort`

How much Claude thinks before it answers: lower is faster and cheaper. Applies to new chats.

- Type: string
- Default: `"medium"`
- Values:
  - `"low"`: The least thinking.
  - `"medium"`: A balance of speed and depth.
  - `"high"`: More thinking.
  - `"xhigh"`: Even more thinking.
  - `"max"`: The most thinking.

### `chat.fallbacks`

When Claude declines a request, let Anthropic's API answer with its recommended fallback model.

- Type: boolean
- Default: `true`

### `chat.max_tokens`

The most a reply may use, its thinking included.

- Type: integer, 1024 to 128000
- Default: `64000`

### `chat.model`

The Claude model new chats start with.

- Type: string, at least 1 character
- Default: `"claude-opus-5-5"`

### `chat.show_thinking`

Show a summary of Claude's reasoning above each reply. Applies to new chats.

- Type: boolean
- Default: `false`

## `debug`

The debugger. A project file can only set its timeouts.

### `debug.log_payloads`

Record whole protocol messages in the debug log, not only what kind they are. They may hold secrets, such as a password in a variable. A project file can't set it.

- Type: boolean
- Default: `false`

## `debug.adapters.<name>`

Debug adapters by name: changes to the built-in lldb-dap, codelldb, debugpy and delve, or adapters of your own, which need a `command`. A project file can't set them.

A table of these tables, `[debug.adapters.<name>]`.

Default: `{}`

### `debug.adapters.<name>.args`

- Type: array of string
- Default: unset (the schema gives no default)

### `debug.adapters.<name>.command`

The program: a bare name is looked up on the login shell's `PATH`.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

### `debug.adapters.<name>.env`

Set on top of Tenmoku's environment.

- Type: table of string
- Default: unset (the schema gives no default)

### `debug.adapters.<name>.languages`

The languages it debugs, by Tenmoku's names: rust, python, go and so on.

- Type: array of string
- Default: unset (the schema gives no default)

### `debug.adapters.<name>.path`

Folders a bare `command` is looked up in before the login shell's `PATH`.

- Type: array of string
- Default: unset (the schema gives no default)

### `debug.adapters.<name>.transport`

How Tenmoku talks to the adapter. With tcp, an argument holds `${port}` for the port it listens on.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"stdio"`: Over its standard input and output.
  - `"tcp"`: Over a connection to 127.0.0.1, on the port Tenmoku puts in its arguments for `${port}`.

### `debug.adapters.<name>.types`

The `type` strings of launch.json configurations it answers to.

- Type: array of string
- Default: unset (the schema gives no default)

## `debug.timeouts`

How long a debug adapter may take, in milliseconds. Applies to sessions started after it changes.

### `debug.timeouts.connect`

Connecting to an adapter that listens on a port.

- Type: integer, at least 1
- Default: `5000`

### `debug.timeouts.launch`

Starting the program: the answer to launch or attach.

- Type: integer, at least 1
- Default: `30000`

### `debug.timeouts.request`

A request's answer, such as a step's or a variable's.

- Type: integer, at least 1
- Default: `10000`

## `editor`

The text editor.

### `editor.auto_close_pairs`

Type the closing bracket or quote when typing an opening one, type over it, and delete both with backspace. Off, typing inserts only what's typed.

- Type: boolean
- Default: `true`

### `editor.auto_save`

When a changed file saves itself.

- Type: string
- Default: `"off"`
- Values:
  - `"off"`: Files save only when you save them.
  - `"after_delay"`: Save a changed file `auto_save_delay` ms after the last edit; in vim insert mode, once insert mode ends.
  - `"on_focus_change"`: Save a changed file when its editor loses focus or the window does.
  - `"on_window_change"`: Save changed files when the window loses focus.
  - `"on_insert_leave"`: With vim on, save a changed file when insert mode ends and after each normal-mode change; with vim off, as `on_focus_change`.

### `editor.auto_save_delay`

How long after the last edit, in milliseconds, `after_delay` saves. Used only by `after_delay`.

- Type: integer, 100 to 60000
- Default: `1000`

### `editor.bracket_pair_colors`

Colour bracket pairs by how deeply they nest, in the theme's `bracket_1`, `bracket_2` and `bracket_3`, then round again. Brackets in strings and comments, and ones without a pair, keep their colour.

- Type: boolean
- Default: `true`

### `editor.code_actions_on_save`

Code action kinds to run, in order, when you save a file, before `format_on_save`, such as `["source.organizeImports", "source.fixAll"]`.

- Type: array of string
- Default: `[]`

### `editor.code_lens`

Show language servers' code lenses ("3 references", "Run") above rows.

- Type: boolean
- Default: `true`

### `editor.color_decorators`

Draw a small square in the colour before each colour in the text (CSS, SCSS, Tailwind…), from a language server or found by Tenmoku, as `default_color_decorators` says. Off, there are none of either.

- Type: boolean
- Default: `true`

### `editor.cursor_line`

Fill the caret's row with the theme's active-line colour, as VS Code's `editor.renderLineHighlight` and vim's 'cursorline'. nvim's default is off and VS Code's on; Tenmoku keeps the editor default. `:set cursorline` changes one editor's.

- Type: boolean
- Default: `true`

### `editor.cursor_surrounding_lines`

How many rows autoscroll keeps between the caret and the window's edge, as VS Code's `editor.cursorSurroundingLines` and vim's 'scrolloff'. A value past half the window's rows centres the caret. `:set scrolloff` changes one editor's.

- Type: integer, 0 to 100
- Default: `3`

### `editor.default_color_decorators`

Where colour squares come from: `"auto"` finds colours itself (CSS colours, and `#rrggbb` in other languages' strings) unless the language server finds some, `"always"` finds them itself and never asks the server, `"never"` shows only the server's.

- Type: string
- Default: `"auto"`
- Values:
  - `"auto"`: Tenmoku's own squares, unless the language server finds colours: then the server's in their place.
  - `"always"`: Tenmoku's own squares only; the server isn't asked for colours.
  - `"never"`: The language server's squares only.

### `editor.font_family`

The font family. Unset, Spline Sans Mono, which Tenmoku bundles, or else the first installed of JetBrains Mono, SF Mono, Menlo, DejaVu Sans Mono, Ubuntu Mono, Liberation Mono and Noto Sans Mono.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

### `editor.font_ligatures`

Draw the font's ligatures, such as `=>` as ⇒ and `->` as →. Off, as in VS Code's `editor.fontLigatures`.

- Type: boolean
- Default: `false`

### `editor.font_size`

The font size, in points.

- Type: number, 6.0 to 72.0
- Default: `13.0`

### `editor.format_on_paste`

Format pasted text spanning more than one row with the language server's range formatting, which one undo takes back with the paste.

- Type: boolean
- Default: `false`

### `editor.format_on_save`

Format a file with its language server before saving it. A server that doesn't answer within a second is skipped and the file saves unformatted.

- Type: boolean
- Default: `false`

### `editor.format_on_type`

Format the row as you type with the language server's on-type formatting (`textDocument/onTypeFormatting`): typing one of its trigger characters, such as `}` or `;`, with one caret applies its edits, which one undo takes back with the character.

- Type: boolean
- Default: `false`

### `editor.indent_guides`

Draw a thin vertical line at each indent level.

- Type: boolean
- Default: `true`

### `editor.inlay_hints`

Show language servers' inlay hints (type and parameter names) inside the text.

- Type: boolean
- Default: `true`

### `editor.inline_blame`

Show the caret row's git blame after the line in every new window; Git: Toggle Line Blame turns it on or off for one window.

- Type: boolean
- Default: `false`

### `editor.inline_diagnostics`

Which diagnostics show their message, dimmed, after the end of their row, as VS Code's Error Lens extension: `"off"`, `"error"`, `"warning"` (errors and warnings) or `"all"` (information and hints too).

- Type: string
- Default: `"warning"`
- Values:
  - `"off"`: No messages after rows.
  - `"error"`: Errors' messages only.
  - `"warning"`: Errors' and warnings' messages.
  - `"all"`: Every diagnostic's message, information and hints too.

### `editor.lightbulb`

Show a lightbulb by the caret's row when a language server has quick fixes or refactorings there.

- Type: boolean
- Default: `true`

### `editor.line_height`

The line height, as a multiple of the font size. Rows round to whole pixels: 1.54 gives 20 px rows at 13 px.

- Type: number, 1.0 to 3.0
- Default: `1.54`

### `editor.line_numbers`

The gutter's line numbers: `"on"` (absolute), `"off"`, `"relative"` (rows from the caret's, as vim's `nonu rnu`) or `"hybrid"` (relative, the caret's row absolute, as vim's `nu rnu`). `:set nu` and `:set rnu` change one editor's.

- Type: string
- Default: `"on"`
- Values:
  - `"on"`: Each row's number.
  - `"off"`: No numbers.
  - `"relative"`: How many rows each row is from the caret's, which shows 0.
  - `"hybrid"`: Relative, with the caret's row showing its number.

### `editor.linked_editing`

Rename an HTML tag's pair as you type in the open or close tag, and other ranges a language server links (`textDocument/linkedEditingRange`).

- Type: boolean
- Default: `false`

### `editor.links`

Underline language servers' links (URLs and file paths) under the pointer while cmd (ctrl off macOS) is held, and open them on cmd-click.

- Type: boolean
- Default: `true`

### `editor.persistent_undo`

Keep each file's undo history on disk when it's saved or closed, so `u` and `<C-r>` reach edits made before it was last closed, as nvim's `undofile`. History is kept only while the file's text is what it was left at; one changed outside the editor starts with none. Off, none is read or written, and none kept is removed.

- Type: boolean
- Default: `true`

### `editor.render_whitespace`

Which spaces and tabs show a mark, a dot or an arrow, in the theme's `whitespace` colour, as VS Code's `editor.renderWhitespace`.

- Type: string
- Default: `"selection"`
- Values:
  - `"none"`: No marks.
  - `"boundary"`: Every space and tab except a single space between two words.
  - `"selection"`: Only spaces and tabs inside a selection.
  - `"trailing"`: Only spaces and tabs after a row's last other character.
  - `"all"`: Every space and tab.

### `editor.rulers`

A thin vertical line at each of these columns, in the theme's `ruler` colour, as VS Code's `editor.rulers`. Columns of 0 or less are ignored.

- Type: array of integer
- Default: `[]`

### `editor.semantic_highlighting`

Colour code with language servers' semantic tokens over the syntax colours.

- Type: boolean
- Default: `true`

### `editor.soft_wrap`

Whether long lines wrap at the window's edge. `alt-z` toggles it per editor.

- Type: boolean
- Default: `false`

### `editor.spell_check`

Underline misspelled words.

- Type: boolean
- Default: `true`

### `editor.spell_check_language`

The language to check words in: `"auto"` for the system's languages, or a BCP-47 tag such as `"en-GB"`.

- Type: string, at least 1 character
- Default: `"auto"`

### `editor.spell_check_scopes`

Where words are checked: `"comments"` and `"strings"` in code, and `"text"` for plain text and Markdown prose. Anything else is ignored, with a warning.

- Type: array of string
- Default: `["comments", "strings", "text"]`

### `editor.tab_size`

How many columns a tab advances to the next multiple of.

- Type: integer, 1 to 16
- Default: `4`

### `editor.todo_comments`

Highlight `TODO:`, `FIXME:`, `HACK:`, `NOTE:`, `PERF:` and `WARN:` keywords (and their aliases) in comments.

- Type: boolean
- Default: `true`

### `editor.wrap_column`

The column soft wrap wraps at, 20 to 500, for `wrap_mode` `column` and `bounded`; 0 leaves wrapping at the window's edge. Only meaningful when `soft_wrap` is on.

- Type: integer, `0`, or 20 to 500
- Default: `0`

### `editor.wrap_mode`

Where soft wrap wraps: `viewport` (the window's edge), `column` (`wrap_column` columns whatever the window, so wider text scrolls) or `bounded` (the smaller of the two). Only meaningful when `soft_wrap` is on, as VS Code's `editor.wordWrap`.

- Type: string
- Default: `"viewport"`
- Values:
  - `"viewport"`: Wrap at the window's edge.
  - `"column"`: Wrap at `wrap_column` columns, however wide the window.
  - `"bounded"`: Wrap at the smaller of the window's edge and `wrap_column` columns.

## `editor.completion`

The completion menu as you type, and the words of open files it offers, VS Code's `editor.quickSuggestions`, `editor.acceptSuggestionOnEnter` and `editor.wordBasedSuggestions`.

### `editor.completion.accept_on_enter`

What `enter` does while the menu shows: `"explicit"` accepts the selected item only once you've moved the selection or opened the menu with `ctrl-space`, so a menu that opened by itself never takes the new line; `"always"` accepts it; `"never"` types a new line and closes the menu. `tab` accepts either way.

- Type: string
- Default: `"explicit"`
- Values:
  - `"explicit"`: Only once the selection has moved, or the menu was opened with `ctrl-space` or a trigger character.
  - `"always"`: Whenever the menu shows.
  - `"never"`: Never: `enter` types a new line and closes the menu.

### `editor.completion.auto_trigger`

Open the completion menu by itself as you type a word. Off, only `ctrl-space` and a language server's trigger characters (such as `.`) open it.

- Type: boolean
- Default: `true`

### `editor.completion.delay_ms`

How long after the last key typed, in milliseconds, the menu is asked for: another key typed sooner waits again.

- Type: integer, 0 to 2000
- Default: `50`

### `editor.completion.in_comments`

Open the menu by itself in comments too.

- Type: boolean
- Default: `false`

### `editor.completion.in_strings`

Open the menu by itself in strings too.

- Type: boolean
- Default: `false`

### `editor.completion.min_word_chars`

The fewest characters of the word before the caret that open the menu by itself.

- Type: integer, 1 to 20
- Default: `1`

### `editor.completion.words`

Where the menu's words come from, after a language server's items or alone without one: `"open_buffers"` (this file and the open files in its language), `"current_buffer"` or `"off"`.

- Type: string
- Default: `"open_buffers"`
- Values:
  - `"open_buffers"`: This file's words, and those of the open files in its language.
  - `"current_buffer"`: This file's words only.
  - `"off"`: No words.

## `editor.minimap`

The minimap: the whole file drawn small beside the text, VS Code's `editor.minimap.*`.

### `editor.minimap.enabled`

Show the minimap.

- Type: boolean
- Default: `true`

### `editor.minimap.max_column`

The most columns of a row the minimap draws.

- Type: integer, at least 1
- Default: `120`

### `editor.minimap.render_characters`

Draw each character's shape; off, draw blocks of colour.

- Type: boolean
- Default: `true`

### `editor.minimap.scale`

How big the minimap draws: each column scale pixels wide and each row 2 × scale pixels tall (3 × scale drawing blocks).

- Type: integer, 1 to 3
- Default: `1`

### `editor.minimap.show_slider`

When the slider over the visible rows shows.

- Type: string
- Default: `"mouseover"`
- Values:
  - `"mouseover"`: While the pointer is over the minimap.
  - `"always"`: All the time.

### `editor.minimap.side`

Which side of the text the minimap is on.

- Type: string
- Default: `"right"`
- Values:
  - `"right"`: Right of the text.
  - `"left"`: Left of the text.

### `editor.minimap.size`

How tall the minimap's rows are.

- Type: string
- Default: `"proportional"`
- Values:
  - `"proportional"`: Rows keep their size, and the minimap scrolls with the editor when they don't all fit.
  - `"fill"`: Rows stretch or shrink so the file fills the minimap's height, no taller than the editor's rows; it never scrolls.
  - `"fit"`: As `proportional` when the file fits the minimap's height, else shrunk as `fill`.

## `editor.sticky_scroll`

Sticky scroll: the header rows of the scopes enclosing the top of the view stay pinned there, VS Code's `editor.stickyScroll.*`.

### `editor.sticky_scroll.default_model`

Where the pinned scopes come from first. Each falls back to the next for a file it can't serve: the outline for a language without one, the folding ranges for text without a syntax tree.

- Type: string
- Default: `"outline"`
- Values:
  - `"outline"`: The file's symbols: functions, types, sections. Languages without an outline use folding.
  - `"folding"`: The ranges the fold gutter folds. Text without a syntax tree uses indentation.
  - `"indentation"`: Blocks of rows indented under a less indented one.

### `editor.sticky_scroll.enabled`

Pin the header rows of the functions, types and other scopes the top of the view is inside, as you scroll.

- Type: boolean
- Default: `true`

### `editor.sticky_scroll.max_line_count`

The most header rows pinned at once.

- Type: integer, 1 to 20
- Default: `5`

### `editor.sticky_scroll.scroll_with_editor`

The pinned rows' text scrolls sideways with the editor's; off, it stays at the first column. Their line numbers never scroll.

- Type: boolean
- Default: `true`

## `extensions`

Installed extensions. A project file can't set them.

### `extensions.disabled`

The IDs of installed extensions that contribute nothing, as in "acme.solarized": they stay installed and listed. An ID no extension has is ignored.

- Type: array of string
- Default: `[]`

### `extensions.plugins_preview`

Run code plugins (WebAssembly) from installed extensions. A preview: their API can change in any release. Each plugin still asks for what it may do before it runs.

- Type: boolean
- Default: `false`

## `extensions.plugin_limits.<name>`

Limits for one installed plugin, by its ID, over what its manifest declares: `background_memory`, `background_wall` and `background_data` for its jobs and its stored data, `call_deadline_ms` and `memory` for what you wait on. A value past what this computer allows is lowered to it. A project file can't set them.

A table of these tables, `[extensions.plugin_limits.<name>]`.

Default: `{}`

### `extensions.plugin_limits.<name>.background_data`

The most data the plugin may keep for itself on disk, as "256MiB" (KiB, MiB or GiB), up to 2GiB. Lowered below what it holds, it can read and delete but not add.

- Type: string
- Default: unset

### `extensions.plugin_limits.<name>.background_memory`

The memory one background job may hold, as "512MiB" (KiB, MiB or GiB), from 1MiB up to 2GiB or a quarter of this computer's RAM.

- Type: string
- Default: unset

### `extensions.plugin_limits.<name>.background_wall`

How long one background job runs before it is cancelled, as "30m" (s or m), from 1s up to 60m.

- Type: string
- Default: unset

### `extensions.plugin_limits.<name>.call_deadline_ms`

How long one call may take of the plugin's own time, in milliseconds, from 50 to 1000.

- Type: integer, at least 0
- Default: unset

### `extensions.plugin_limits.<name>.memory`

The memory the plugin holds for what you wait on, as "128MiB", up to 256MiB.

- Type: string
- Default: unset

## `formatters.<name>`

External formatters, by language name (`rust`, `python`, `typescript`, ...: the names `lsp.servers.<name>.languages` uses). A project file can't set them: a cloned repo shouldn't choose a program to run on save.

A table of these tables, `[formatters.<name>]`.

Default: `{}`

### `formatters.<name>.command`

The program and its arguments. `{file}` in any of them is replaced with the buffer's absolute path, so a tool such as prettier finds its config. Not empty.

- Type: array of string, at least 1 item
- Required

### `formatters.<name>.prefer`

Which formatter runs first, this one or the language server's; the other is the fallback when the first has nothing to do or fails.

- Type: string
- Default: `"external"`
- Values:
  - `"external"`: The external formatter runs first.
  - `"lsp"`: The language server formats first.

### `formatters.<name>.stdin`

true: the text goes to the program's stdin and the formatted text is read from its stdout. false: a temporary copy is passed as `{file}` and read back, for tools that only format files.

- Type: boolean
- Default: `true`

### `formatters.<name>.timeout_ms`

How long, in milliseconds, the program may run before it is killed and nothing changes.

- Type: integer, 1 to 60000
- Default: `3000`

## `lsp`

Language servers. A project file may only turn them off, but in a trusted folder its settings for them apply.

### `lsp.enabled`

Whether language servers run at all.

- Type: boolean
- Default: `true`

### `lsp.max_file_bytes`

The largest file, in bytes, synced to a language server. Applies to files opened after it changes.

- Type: integer, at least 0
- Default: `52428800`

### `lsp.python_environment`

The Python environment (a virtualenv) Python's language servers run in: a server's program is looked for in its bin before the `PATH`, and the server is told of its interpreter. An absolute path, one under `~`, or one relative to the server's project root. Unset, it's `$VIRTUAL_ENV`, else the project's `.venv` or `venv`: the first with a `bin/python`. A user setting: a project file's applies only in a trusted folder.

- Type: string
- Default: unset (the schema gives no default)

### `lsp.python_server`

Which built-in server Python files get: only that one runs, and with it ruff, if it's installed. Unset, it's pyrefly, unless `[lsp.servers.ty]` or `[lsp.servers.pyright]` is there: then that one (ty first), so settings written for it keep it. A project file's applies only in a trusted folder.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"pyrefly"`: pyrefly's `pyrefly lsp`.
  - `"ty"`: ty's `ty server`.
  - `"pyright"`: pyright's `pyright-langserver --stdio`.

### `lsp.typescript_server`

Which built-in server TypeScript and JavaScript files get.

- Type: string
- Default: `"typescript-go"`
- Values:
  - `"typescript-language-server"`: typescript-language-server, over tsserver.
  - `"typescript-go"`: typescript-go's `tsc --lsp`.

## `lsp.servers.<name>`

Servers by name: changes to the built-in rust-analyzer, pyrefly, ty, pyright, ruff, typescript-language-server, typescript-go, gopls, vscode-css-language-server, jdtls, kotlin-lsp and csharp-ls, or servers of your own, which need a `command`.

A table of these tables, `[lsp.servers.<name>]`.

Default: `{}`

### `lsp.servers.<name>.add_on`

Whether the server only adds to the language's main server instead of being it: a language's lead is its first server that isn't an add-on, and a file also gets every add-on listing its language.

- Type: boolean
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.args`

- Type: array of string
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.command`

The program: a bare name is looked up on the login shell's `PATH`.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.enabled`

Whether the server runs.

- Type: boolean
- Default: `true`

### `lsp.servers.<name>.env`

Set on top of Tenmoku's environment.

- Type: table of string
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.initialization_options`

Sent with `initialize`.

- Type: any
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.languages`

The languages it serves, by Tenmoku's names: rust, python, javascript, typescript, tsx and so on. A file gets the first enabled server listing its language.

- Type: array of string
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.readiness`

How Tenmoku tells the server is ready.

- Type: string
- Default: unset (the schema gives no default)
- Values:
  - `"server-status"`: rust-analyzer's `experimental/serverStatus`.
  - `"progress"`: No work-done progress open.

### `lsp.servers.<name>.ready_ignores_progress`

Work-done progress token prefixes that don't hold back readiness.

- Type: array of string
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.repoll_ms`

How often, in milliseconds, the colours, hints, lenses and links of the files shown are asked of the server again, for a server whose answers change with no edit. 0 or unset never.

- Type: integer, at least 0
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.root_markers`

File or folder names that mark a project root, in priority order; replaces the server's built-in list. `*.ext` matches any name ending in `.ext`, such as `*.sln`; there is no other glob syntax.

- Type: array of string
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.settings`

Answers the server's `workspace/configuration` requests, by section: `[lsp.servers.rust-analyzer.settings.rust-analyzer]` is the `rust-analyzer` section. Changing it doesn't restart the server.

- Type: table
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.watch_excludes`

Globs, relative to the server's root, of the files and folders left out of `watch_files`: a folder matching one leaves out everything in it, as `**/node_modules` does.

- Type: array of string
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.watch_excludes_from`

A JSON pointer into `settings` to the list `watch_excludes` takes.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.watch_files`

Globs, relative to the server's root, of the files whose changes on disk it's told of, whether or not it asks: `**/*.css`, say. Files outside its root are never sent. Replaces the built-in list or pointer.

- Type: array of string
- Default: unset (the schema gives no default)

### `lsp.servers.<name>.watch_files_from`

A JSON pointer into `settings` to the list `watch_files` takes, read again whenever they change: `/cssVariables/lookupFiles`, say.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

## `lsp.timeouts`

How long requests may take, in milliseconds, by method.

### `lsp.timeouts.code_action`

Code actions (cmd-.), an explicit request: the picker says there are none only once this has passed.

- Type: integer, at least 1
- Default: `10000`

### `lsp.timeouts.completion`

- Type: integer, at least 1
- Default: `3000`

### `lsp.timeouts.definition`

Go to definition.

- Type: integer, at least 1
- Default: `5000`

### `lsp.timeouts.hover`

- Type: integer, at least 1
- Default: `3000`

### `lsp.timeouts.references`

Find references.

- Type: integer, at least 1
- Default: `30000`

### `lsp.timeouts.resolve`

Resolving a completion item.

- Type: integer, at least 1
- Default: `2000`

### `lsp.timeouts.signature_help`

Signature help, which pops up while typing.

- Type: integer, at least 1
- Default: `3000`

### `lsp.timeouts.workspace_symbol`

Workspace symbols.

- Type: integer, at least 1
- Default: `5000`

## `remote`

Working on a remote host over ssh. All four are the user's own: a project file can't set them.

### `remote.auto_forward`

What happens when a program on the host starts listening on a port: a notification offering to forward it, forwarding it straight away, or nothing. A port a run or task configuration declares is forwarded whatever this says.

- Type: string
- Default: `"notify"`
- Values:
  - `"notify"`: A notification offering to forward it.
  - `"forward"`: Forward it as it appears.
  - `"off"`: Leave it alone.

### `remote.dev_server`

A `tenmoku-server` built from this checkout, for a development build of Tenmoku, which has no server release to download. It is uploaded and installed like a release asset, and it must report exactly this build's version. Unset means the release download.

- Type: string
- Default: unset (the schema gives no default)

### `remote.ports`

Ports forwarded on every connect, each `3000` for the same port at both ends or `"5432:15432"` for the host's port and the local one it is offered on. The client listens on 127.0.0.1 only.

- Type: array of integer, 1 to 65535 or string, matching `^(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]):(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5])$`
- Default: `[]`

### `remote.reconnect_grace`

How many minutes a host keeps a session whose link dropped, with its terminals, language servers and debuggers still running, before it tears them down.

- Type: integer, 1 to 60
- Default: `10`

## `status_bar`

The status bar.

### `status_bar.visible`

Whether the status bar shows at the bottom of the window. View &gt; Toggle Status Bar flips it and saves it here.

- Type: boolean
- Default: `true`

## `terminal`

The terminal panel.

### `terminal.font_fallbacks`

Fonts to take a character from when the terminal's font lacks it, in order: "Symbols Nerd Font Mono" gives Nerd Font icons with any font.

- Type: array of string
- Default: `[]`

### `terminal.font_family`

The terminal's font family. Unset uses the editor's.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

### `terminal.font_size`

The terminal's font size, in points. Unset, 12, the design's small code size.

- Type: number, 6.0 to 72.0
- Default: unset (the schema gives no default)

### `terminal.scrollback`

How many lines the terminal keeps above the screen. Applies to terminals started after it changes.

- Type: integer, 0 to 100000
- Default: `10000`

### `terminal.shell`

The program the terminal runs. Empty runs your login shell. A running terminal keeps its shell; the next one started uses the new one.

- Type: string
- Default: `""`

### `terminal.shell_args`

The shell's arguments. Ignored when `shell` is empty.

- Type: array of string
- Default: `[]`

## `theme`

The colour theme.

### `theme.dark`

The installed theme the dark theme is, by ID: an extension's theme file, as in "acme.solarized/solarized-dark". Unset is Yohen, Tenmoku's own dark theme.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

### `theme.light`

The installed theme the light theme is, by ID. Unset is Hakuji, Tenmoku's own light theme.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

### `theme.mode`

dark, light, or system to follow the system's appearance. Your own colours go in theme.toml beside this file; its appearance says whether it replaces the dark or the light theme.

- Type: string
- Default: `"dark"`
- Values:
  - `"dark"`: The dark theme.
  - `"light"`: The light theme.
  - `"system"`: The dark or light theme as the system's appearance is.

### `theme.name`

A bundled theme by its title or ID, as in "Dark Modern" or "Tenmoku Ember": the dark or the light theme, as its appearance is. "dark" and "light" are Yohen and Hakuji. theme.dark or theme.light, if set, wins for that one.

- Type: string, at least 1 character
- Default: unset (the schema gives no default)

## `tree`

The project tree.

### `tree.show_ignored`

List entries that `.gitignore` and `.ignore` files match (`target`, `.idea`), greyed. Off hides them. `.git` is never listed. Applies to trees opened after it changes.

- Type: boolean
- Default: `true`

## `update`

Updates to Tenmoku itself.

### `update.automatic_checks`

Check for a newer Tenmoku in the background once a day and offer to install it. Check for Updates… always works.

- Type: boolean
- Default: `true`

### `update.channel`

Which releases to update to: `stable`, or `preview`, whose builds arrive more often and may be less polished. Switching back to stable waits for the next stable release that is newer than what is installed, never downgrading. A project file can't set it.

- Type: string
- Default: `"stable"`
- Values:
  - `"stable"`: Stable releases only.
  - `"preview"`: The newest build, preview or stable.

## `vim`

Vim mode.

### `vim.clipboard`

Which register vim's unnamed register shares with the system clipboard: `"unnamedplus"` makes `yy` copy to it and `p` paste from it, as nvim's `clipboard=unnamedplus` does. Empty, only `"+` and `"*` reach it.

- Type: string
- Default: `""`
- Values:
  - `""`: Only the `+` and `*` registers reach the system clipboard.
  - `"unnamedplus"`: Yanks, deletes and puts that name no register use the system clipboard.

### `vim.enabled`

Whether the editor starts in vim's normal mode, where keys run vim commands rather than type. View &gt; Toggle Vim Mode flips it and saves it here.

- Type: boolean
- Default: `false`

### `vim.filter_time_limit`

How long, in milliseconds, a `!` filter or `:r !` may run before it is killed and nothing changes.

- Type: integer, at least 100
- Default: `10000`

### `vim.highlight_yank`

true: a yank briefly highlights the text it took, as nvim's vim.hl.on\_yank does.

- Type: boolean
- Default: `true`

### `vim.highlight_yank_ms`

How long, in milliseconds, the yank highlight stays.

- Type: integer, 1 to 5000
- Default: `150`

### `vim.ignorecase`

true: a search with `/`, `?`, `n` or `N` ignores case, as nvim's 'ignorecase' does. `\c` and `\C` in a pattern override it.

- Type: boolean
- Default: `false`

### `vim.inccommand`

true: a `:s` shows what it would replace as it is typed, its matches highlighted and each row it changes as it would read after the row, as nvim's 'inccommand' (`nosplit`) does; Esc puts everything back.

- Type: boolean
- Default: `true`

### `vim.incsearch`

true: a search shows its first match as the pattern is typed, as nvim's 'incsearch' does.

- Type: boolean
- Default: `true`

### `vim.jump_on_s`

true: `s` and `S` start the forward and backward jump, as leap and flash map them, instead of changing a char or a line (`cl` and `cc` still do). `gs` and `gS` start it either way.

- Type: boolean
- Default: `false`

### `vim.jump_style`

How the jump (`gs`, `gS`, and `s` and `S` with `jump_on_s`) reads its target: `"leap"` takes two chars, then labels the matches in the view; `"flash"` takes one char, then labels every match of it in the view at once, before and after the caret (`gS` before only), as flash.nvim does.

- Type: string
- Default: `"leap"`
- Values:
  - `"leap"`: Two chars, then a label, as leap.nvim does.
  - `"flash"`: One char, then a label on every match in the view, both ways, as flash.nvim does.

### `vim.leader`

The leader key `<leader>` stands for in a remap, in vim notation (`"<Space>"` works). Left at `"\\"`, a preset's leader applies.

- Type: string
- Default: `"\\"`

### `vim.preset`

A bundled set of remaps loaded beneath the vimrc and `[[vim.remap]]`, which replace any of its maps for the same keys: `"none"`, plain vim, or `"lazyvim"`, LazyVim's, with `<Space>` as the leader unless `leader` is set.

- Type: string
- Default: `"none"`
- Values:
  - `"none"`: Plain vim: no maps but the user's, and `\` as the leader.
  - `"lazyvim"`: LazyVim's maps, with `<Space>` as the leader.

### `vim.quick_scope`

Underlines the first char of each word on the caret's row that `f` or `F` reaches in one or two presses, as quick-scope does: `always` in normal and visual mode; `on_keys` only while `f`, `F`, `t` or `T` waits for its char, in that direction.

- Type: string
- Default: `"on_keys"`
- Values:
  - `"off"`: No underlines.
  - `"on_keys"`: While `f`, `F`, `t` or `T` waits for its char, that direction's.
  - `"always"`: In normal and visual mode both directions', or the waiting one's.

### `vim.regex_time_limit`

How long, in milliseconds, a vim search or :s may run on one command before it stops with Interrupted and changes nothing (nvim's 'redrawtime' is the same default).

- Type: integer, at least 1
- Default: `2000`

### `vim.replace_with_register`

true: `gr{motion}`, `grr` and visual `gr` replace text with a register's contents, as the ReplaceWithRegister plugin does, instead of running nvim 0.12's language-server `gr` keys (`grr` references, `grn` rename, `gra` code action, `gri`, `grt`).

- Type: boolean
- Default: `false`

### `vim.shell`

The program vim's `!` filter and `:r !` run commands with, as `shell -c command`. Empty: your shell (`$SHELL`, else `/bin/sh`; on Windows `%COMSPEC%`). A project file can't set it.

- Type: string
- Default: `""`

### `vim.smartcase`

true: with `ignorecase` on, a typed search pattern with an uppercase letter matches case (`*` and `#` still ignore it), as nvim's 'smartcase' does.

- Type: boolean
- Default: `false`

### `vim.subword_motions`

true: `w`, `b`, `e` and `ge` stop at each part of a camelCase, PascalCase, snake\_case or digit-joined name, as vim-wordmotion does; `W B E gE` still move by WORDs. `iS` and `aS` select a subword either way.

- Type: boolean
- Default: `false`

### `vim.which_key`

true: a popup lists the keys that go on from a pending prefix (`g`, `z`, `<leader>`, a remap's first keys) with what each does, as which-key.nvim's does, once `which_key_delay_ms` passes with no key.

- Type: boolean
- Default: `true`

### `vim.which_key_delay_ms`

How long, in milliseconds, a prefix waits for its next key before the which-key popup shows (which-key.nvim's default). 0 shows it at once.

- Type: integer, 0 to 5000
- Default: `200`

## `vim.remap`

Key remaps, as vim's `:map` makes them. Each is a `[[vim.remap]]` table; a later one for the same keys in the same mode replaces an earlier one.

A list of these tables, `[[vim.remap]]`.

Default: `[]`

### `vim.remap.from`

The keys to map, in vim notation (`"<leader>w"`, `"H"`).

- Type: string
- Required

### `vim.remap.mode`

The modes the remap applies in: one of `"n"` (normal), `"x"` or `"v"` (visual), `"o"` (operator-pending), `"i"` (insert and replace), `"c"` (command line), or a list of them.

- Type: string or array of string
- Required
- Values:
  - `"n"`: Normal mode.
  - `"x"`: Visual mode.
  - `"v"`: Visual mode, as `x` (vim's `v` also maps select mode).
  - `"o"`: Operator-pending mode.
  - `"i"`: Insert and replace mode.
  - `"c"`: Command-line mode.

### `vim.remap.recursive`

true: the keys `to` produces are themselves remapped, as `:map` does rather than `:noremap`.

- Type: boolean
- Default: `false`

### `vim.remap.to`

The keys they stand for, in vim notation (`":w<CR>"`).

- Type: string
- Required

## `vim.which_key_group`

Names for groups of remaps, which the which-key popup shows as `+{name}` in place of `+{count}`. Each is a `[[vim.which_key_group]]` table; it loads after the vimrc's `:fegroup`s and the preset's, and a later name for the same keys in the same mode wins, as does a later icon.

A list of these tables, `[[vim.which_key_group]]`.

Default: `[]`

### `vim.which_key_group.icon`

An icon the popup draws beside the group, by its name in the bundled Lucide set (`"search"`, `"git-branch"`, `"bug"`). Unset keeps an icon an earlier name for the same keys gives, as the preset's; a name not in the set draws none.

- Type: string, matching `^[a-z0-9]+(-[a-z0-9]+)*$`
- Default: unset (the schema gives no default)

### `vim.which_key_group.keys`

The prefix the group's remaps start with, in vim notation (`"<leader>f"`).

- Type: string, at least 1 character
- Required

### `vim.which_key_group.mode`

The modes the name applies in, as `vim.remap`'s `mode`: `"n"` (normal) unless set.

- Type: string or array of string
- Default: `"n"`
- Values:
  - `"n"`: Normal mode.
  - `"x"`: Visual mode.
  - `"v"`: Visual mode, as `x` (vim's `v` also maps select mode).
  - `"o"`: Operator-pending mode.
  - `"i"`: Insert and replace mode.
  - `"c"`: Command-line mode.

### `vim.which_key_group.name`

What the popup calls the group (`"file/find"`), after a `+`.

- Type: string, at least 1 character
- Required

## `workbench`

The workbench: its panes and tabs.

### `workbench.color_blind_mode`

Colour-blind mode: the signals that tell green from red (added, passed and INSERT; removed, failed and errors) are drawn in blue and orange instead, with their soft grounds, in a theme that has colour-blind colours, as Yohen and Hakuji do. Warnings, modified and every glyph stay as they are. It applies at once.

- Type: boolean
- Default: `false`

### `workbench.contrast`

Whether colours are drawn at high contrast: `system` follows the system's Increase Contrast option (System Settings › Accessibility › Display), `high` and `normal` decide it here, whatever the system says. High contrast strengthens every theme's borders, focus ring, selections, muted text and the editor's line numbers.

- Type: string
- Default: `"system"`
- Values:
  - `"system"`: As the system's Increase Contrast option is.
  - `"high"`: High contrast, whatever the system says.
  - `"normal"`: The theme's own contrast, whatever the system says.

### `workbench.icon_theme`

The file and folder icons: `tenmoku` draws line icons tinted from the theme, by file type and folder role; `material` draws Material Icon Theme's, in colour, by language, file name and folder name; `minimal` draws plain line icons in the theme's colours. A project file can't set it.

- Type: string
- Default: `"tenmoku"`
- Values:
  - `"tenmoku"`: Line icons tinted from the theme, by file type and folder role.
  - `"material"`: Material Icon Theme's coloured icons, by language, file name and folder name.
  - `"minimal"`: Plain line icons in the theme's colours.

### `workbench.locale`

The language of Tenmoku's own text: `auto` follows the system's preferred languages; a tag such as `de` or `pt-BR` picks a shipped one. Menus and panels change at once; macOS's own panels and the updater follow the system (System Settings › Language &amp; Region › Applications). A language Tenmoku doesn't ship is skipped, with a warning naming the one used. A project file can't set it.

- Type: string
- Default: `"auto"`
- Values:
  - `"auto"`
  - `"en-US"`

### `workbench.notifications`

Which notifications show as toasts: `all` shows every one; `needs_you` only those that need you (they have an action or a question), the rest going straight to the notifications inbox; `off` sends every one to the inbox, though an error still shows once. A project file can't set it.

- Type: string
- Default: `"all"`
- Values:
  - `"all"`: Every notification shows as a toast.
  - `"needs_you"`: Only a notification that needs you shows as a toast.
  - `"off"`: None shows as a toast but an error, once.

### `workbench.preview_tabs`

Whether a file single-clicked in the project tree opens in a preview tab, in italics, that the next one replaces. Editing it, double-clicking it or its tab, or pinning it keeps it open.

- Type: boolean
- Default: `true`

### `workbench.rail`

Which edge of the window the activity rail is on, `left` or `right`. The sidebar opens beside it, so they move together, at once.

- Type: string
- Default: `"left"`
- Values:
  - `"left"`: On the window's left edge, the sidebar to its right.
  - `"right"`: On the window's right edge, the sidebar to its left.

### `workbench.reduce_motion`

Whether animation is reduced: `system` follows the system's Reduce Motion option (System Settings › Accessibility › Display), `on` and `off` decide it here, whatever the system says.

- Type: string
- Default: `"system"`
- Values:
  - `"system"`: As the system's Reduce Motion option is.
  - `"on"`: Reduced, whatever the system says.
  - `"off"`: Not reduced, whatever the system says.

### `workbench.start_page`

Whether a window with no tabs open shows the Start page in its empty pane: New File, Open File, Open Folder, the recent folders and files, and the settings, with LazyVim's dashboard keys while vim is on. Off, the pane says "No open files".

- Type: boolean
- Default: `true`

## `workbench.zen_mode`

Zen Mode: the editor alone, VS Code's `zenMode.*`.

### `workbench.zen_mode.center_layout`

Center the editors, with margins of a fifth of the window's width on each side.

- Type: boolean
- Default: `true`

### `workbench.zen_mode.hide_status_bar`

Hide the status bar.

- Type: boolean
- Default: `true`

### `workbench.zen_mode.hide_tabs`

Hide the panes' tab strips and breadcrumbs.

- Type: boolean
- Default: `true`
