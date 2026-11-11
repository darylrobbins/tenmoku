# fe-workbench's Settings tab (settings_editor.rs, settings_editor_layout.rs,
# settings_editor_controls.rs, settings_editor_choices.rs). Every key here
# starts `workbench-settings-`. A setting's help line is fe-config's
# `settings-<key>` message, not one of these.

## The page

# The tab's title, and the page title at its top.
workbench-settings-title = Settings
workbench-settings-search = Search settings and keys
# The scope switch: which settings file the page shows and writes.
workbench-settings-scope = Settings scope
workbench-settings-scope-user = You
workbench-settings-scope-project = This project
workbench-settings-open-file = Open settings.toml
# The Open button's tooltip, with no config directory to open.
workbench-settings-no-config-dir = There's no config directory
workbench-settings-nothing-found = No settings found
# The section index: a section with a setting you changed.
workbench-settings-section-changed = { $section }, changed

## The Common section, first

workbench-settings-common = Common
workbench-settings-common-description = The settings most people change first.
# The theme cards: each is theme.mode's value.
workbench-settings-theme-cards = Theme
workbench-settings-theme-yohen = Yohen
workbench-settings-theme-hakuji = Hakuji
workbench-settings-theme-system = Match system

## A setting's row

workbench-settings-reset = Reset
workbench-settings-reset-tooltip = Reset to Default
workbench-settings-restricted = This project's settings can't change this setting
workbench-settings-cant-change = Can't change { $key }

## The controls (settings_editor_controls.rs, settings_editor_choices.rs)

workbench-settings-default-choice = Default (built-in)
workbench-settings-not-installed = not installed
workbench-settings-other-choice = Other…
workbench-settings-not-set = Not set
workbench-settings-add-item = Add Item
workbench-settings-remove-item = Remove item
workbench-settings-edit-in-file = Edit in settings.toml
workbench-settings-cant-open = Can't open settings
workbench-settings-cant-create = Can't create { $path }
# Why a write failed: the file is there but can't be opened for writing.
workbench-settings-cant-write = { $path } can't be written to: { $error }
# Why a typed value wasn't written, under its input.
workbench-settings-not-whole-number = must be a whole number
workbench-settings-not-number = must be a number
# A bundled theme's appearance, beside its title in theme.name's list.
workbench-settings-appearance-dark = dark
workbench-settings-appearance-light = light

## Each setting's label, by its dotted key with dashes for dots: the name
## its key ends in, as a title, but the Common section's, which the design
## names (Rail position, Editor font, Font size, Vim mode). Looked up by key
## (`workbench-settings-label-` is a dynamic prefix).

workbench-settings-label-chat-context_budget = Context Budget
workbench-settings-label-chat-effort = Effort
workbench-settings-label-chat-fallbacks = Fallbacks
workbench-settings-label-chat-max_tokens = Max Tokens
workbench-settings-label-chat-model = Model
workbench-settings-label-chat-show_thinking = Show Thinking
workbench-settings-label-editor-auto_close_pairs = Auto Close Pairs
workbench-settings-label-editor-auto_save = Auto Save
workbench-settings-label-editor-auto_save_delay = Auto Save Delay
workbench-settings-label-editor-bracket_pair_colors = Bracket Pair Colors
workbench-settings-label-editor-code_actions_on_save = Code Actions On Save
workbench-settings-label-editor-code_lens = Code Lens
workbench-settings-label-editor-color_decorators = Color Decorators
workbench-settings-label-editor-cursor_line = Cursor Line
workbench-settings-label-editor-cursor_surrounding_lines = Cursor Surrounding Lines
workbench-settings-label-editor-default_color_decorators = Default Color Decorators
workbench-settings-label-editor-font_family = Editor font
workbench-settings-label-editor-font_ligatures = Font Ligatures
workbench-settings-label-editor-font_size = Font size
workbench-settings-label-editor-format_on_paste = Format On Paste
workbench-settings-label-editor-format_on_save = Format On Save
workbench-settings-label-editor-format_on_type = Format On Type
workbench-settings-label-editor-indent_guides = Indent Guides
workbench-settings-label-editor-inlay_hints = Inlay Hints
workbench-settings-label-editor-inline_blame = Inline Blame
workbench-settings-label-editor-inline_diagnostics = Inline Diagnostics
workbench-settings-label-editor-lightbulb = Lightbulb
workbench-settings-label-editor-line_height = Line Height
workbench-settings-label-editor-line_numbers = Line Numbers
workbench-settings-label-editor-linked_editing = Linked Editing
workbench-settings-label-editor-links = Links
workbench-settings-label-editor-minimap-enabled = Enabled
workbench-settings-label-editor-minimap-max_column = Max Column
workbench-settings-label-editor-minimap-render_characters = Render Characters
workbench-settings-label-editor-minimap-scale = Scale
workbench-settings-label-editor-minimap-show_slider = Show Slider
workbench-settings-label-editor-minimap-side = Side
workbench-settings-label-editor-minimap-size = Size
workbench-settings-label-editor-persistent_undo = Persistent Undo
workbench-settings-label-editor-render_whitespace = Render Whitespace
workbench-settings-label-editor-rulers = Rulers
workbench-settings-label-editor-semantic_highlighting = Semantic Highlighting
workbench-settings-label-editor-soft_wrap = Soft Wrap
workbench-settings-label-editor-spell_check = Spell Check
workbench-settings-label-editor-spell_check_language = Spell Check Language
workbench-settings-label-editor-spell_check_scopes = Spell Check Scopes
workbench-settings-label-editor-sticky_scroll-default_model = Default Model
workbench-settings-label-editor-sticky_scroll-enabled = Enabled
workbench-settings-label-editor-sticky_scroll-max_line_count = Max Line Count
workbench-settings-label-editor-sticky_scroll-scroll_with_editor = Scroll With Editor
workbench-settings-label-editor-tab_size = Tab Size
workbench-settings-label-editor-todo_comments = Todo Comments
workbench-settings-label-editor-wrap_column = Wrap Column
workbench-settings-label-editor-wrap_mode = Wrap Mode
workbench-settings-label-extensions-disabled = Disabled
workbench-settings-label-extensions-plugins_preview = Plugins Preview
workbench-settings-label-lsp-enabled = Enabled
workbench-settings-label-lsp-max_file_bytes = Max File Bytes
workbench-settings-label-lsp-servers = Servers
workbench-settings-label-lsp-timeouts-code_action = Code Action
workbench-settings-label-lsp-timeouts-completion = Completion
workbench-settings-label-lsp-timeouts-definition = Definition
workbench-settings-label-lsp-timeouts-hover = Hover
workbench-settings-label-lsp-timeouts-references = References
workbench-settings-label-lsp-timeouts-resolve = Resolve
workbench-settings-label-lsp-timeouts-signature_help = Signature Help
workbench-settings-label-lsp-timeouts-workspace_symbol = Workspace Symbol
workbench-settings-label-lsp-typescript_server = Typescript Server
workbench-settings-label-status_bar-visible = Visible
workbench-settings-label-terminal-font_fallbacks = Font Fallbacks
workbench-settings-label-terminal-font_family = Font Family
workbench-settings-label-terminal-font_size = Font Size
workbench-settings-label-terminal-scrollback = Scrollback
workbench-settings-label-terminal-shell = Shell
workbench-settings-label-terminal-shell_args = Shell Args
workbench-settings-label-theme-dark = Dark
workbench-settings-label-theme-light = Light
workbench-settings-label-theme-mode = Mode
workbench-settings-label-theme-name = Name
workbench-settings-label-tree-show_ignored = Show Ignored
workbench-settings-label-update-automatic_checks = Automatic Checks
workbench-settings-label-vim-clipboard = Clipboard
workbench-settings-label-vim-enabled = Vim mode
workbench-settings-label-vim-filter_time_limit = Filter Time Limit
workbench-settings-label-vim-highlight_yank = Highlight Yank
workbench-settings-label-vim-highlight_yank_ms = Highlight Yank Ms
workbench-settings-label-vim-ignorecase = Ignorecase
workbench-settings-label-vim-inccommand = Inccommand
workbench-settings-label-vim-incsearch = Incsearch
workbench-settings-label-vim-jump_on_s = Jump On S
workbench-settings-label-vim-jump_style = Jump Style
workbench-settings-label-vim-leader = Leader
workbench-settings-label-vim-preset = Preset
workbench-settings-label-vim-quick_scope = Quick Scope
workbench-settings-label-vim-regex_time_limit = Regex Time Limit
workbench-settings-label-vim-remap = Remap
workbench-settings-label-vim-replace_with_register = Replace With Register
workbench-settings-label-vim-shell = Shell
workbench-settings-label-vim-smartcase = Smartcase
workbench-settings-label-vim-subword_motions = Subword Motions
workbench-settings-label-vim-which_key = Which Key
workbench-settings-label-vim-which_key_delay_ms = Which Key Delay Ms
workbench-settings-label-vim-which_key_group = Which Key Group
workbench-settings-label-workbench-contrast = Contrast
workbench-settings-label-workbench-icon_theme = Icon Theme
workbench-settings-label-workbench-locale = Locale
workbench-settings-label-workbench-preview_tabs = Preview Tabs
workbench-settings-label-workbench-rail = Rail position
workbench-settings-label-workbench-reduce_motion = Reduce motion
workbench-settings-label-workbench-start_page = Start Page
workbench-settings-label-workbench-zen_mode-center_layout = Center Layout
workbench-settings-label-workbench-zen_mode-hide_status_bar = Hide Status Bar
workbench-settings-label-workbench-zen_mode-hide_tabs = Hide Tabs
