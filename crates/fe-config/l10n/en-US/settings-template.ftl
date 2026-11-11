### The prose of settings-template.toml: generated from it, one message per
### block of comment lines that aren't commented TOML. The template is the
### en-US source, so don't edit this by hand: change the template, then
### rerun fe-config's tests with UPDATE_GOLDEN=1.
###
### A translation's lines are written after `# `, wrapped at 80 columns;
### an empty line is `#` alone. Commented TOML around a block, such as
### `# font_size = 13.0`, is never translated.

settings-template-intro =
    Tenmoku settings.

    This file reloads when you save it. A project's `.tenmoku/settings.toml`
    overrides it, key by key. Uncomment a line to change that setting.

settings-template-editor-font_family =
    The font family. Unset, Spline Sans Mono, which Tenmoku bundles, or else
    the first installed of JetBrains Mono, SF Mono, Menlo, DejaVu Sans Mono,
    Ubuntu Mono, Liberation Mono and Noto Sans Mono.

settings-template-editor-font_ligatures = Draw the font's ligatures, such as `=>` as ⇒ and `->` as →.

settings-template-editor-font_size = The font size, in points (6 to 72).

settings-template-editor-line_height =
    The line height, as a multiple of the font size (1 to 3). Rows round to
    whole pixels: 1.54 gives 20 px rows at 13 px.

settings-template-editor-tab_size = How many columns a tab advances to the next multiple of (1 to 16).

settings-template-editor-soft_wrap = Whether long lines wrap at the window's edge.

settings-template-editor-wrap_column =
    The column soft wrap wraps at (20 to 500), for wrap_mode "column" and
    "bounded"; 0 leaves wrapping at the window's edge. Only used with soft_wrap.

settings-template-editor-wrap_mode =
    Where soft wrap wraps: "viewport" (the window's edge), "column" (wrap_column
    columns whatever the window) or "bounded" (the smaller of the two). Only used
    with soft_wrap.

settings-template-editor-format_on_save =
    Format a file with its language server before saving it. A server that
    doesn't answer within a second is skipped and the file saves unformatted.

settings-template-editor-format_on_type =
    Format the row as you type with the language server's on-type formatting
    (textDocument/onTypeFormatting): typing one of its trigger characters, such as
    {"}"} or ;, with one caret applies its edits, which one undo takes back with the
    character.

settings-template-editor-format_on_paste =
    Format pasted text spanning more than one row with the language server's range
    formatting, which one undo takes back with the paste.

settings-template-editor-inlay_hints = Show language servers' inlay hints (type and parameter names) inside the text.

settings-template-editor-semantic_highlighting = Colour code with language servers' semantic tokens over the syntax colours.

settings-template-editor-lightbulb =
    Show a lightbulb by the caret's row when a language server has quick fixes or
    refactorings there.

settings-template-editor-code_lens = Show language servers' code lenses ("3 references", "Run") above rows.

settings-template-editor-links =
    Underline language servers' links (URLs and file paths) under the pointer
    while cmd (ctrl off macOS) is held, and open them on cmd-click.

settings-template-editor-color_decorators =
    Draw a small square in the colour before each colour in the text (CSS, SCSS,
    Tailwind…), from a language server or found by Tenmoku, as
    default_color_decorators says. Off, there are none of either.

settings-template-editor-default_color_decorators =
    Where colour squares come from: "auto" finds colours itself (CSS colours, and
    #rrggbb in other languages' strings) unless the language server finds some,
    "always" finds them itself and never asks the server, "never" shows only the
    server's.

settings-template-editor-linked_editing =
    Rename an HTML tag's pair as you type in the open or close tag, and other
    ranges a language server links (textDocument/linkedEditingRange).

settings-template-editor-code_actions_on_save =
    Code action kinds to run, in order, when you save a file, before
    format_on_save, such as ["source.organizeImports", "source.fixAll"].

settings-template-editor-auto_close_pairs =
    Type the closing bracket or quote when typing an opening one, type over it,
    and delete both with backspace.

settings-template-editor-persistent_undo =
    Keep each file's undo history on disk when it's saved or closed, so u and
    <C-r> reach edits made before it was last closed, as nvim's undofile.
    History is kept only while the file's text is what it was left at.

settings-template-editor-auto_save =
    When a changed file saves itself: "off", "after_delay", "on_focus_change",
    "on_window_change" or "on_insert_leave".

settings-template-editor-auto_save_delay =
    How long after the last edit, in milliseconds, "after_delay" saves
    (100 to 60000).

settings-template-editor-indent_guides = Draw a thin vertical line at each indent level.

settings-template-editor-inline_blame =
    Show the caret row's git blame after the line in every new window; Git:
    Toggle Line Blame turns it on or off for one window.

settings-template-editor-bracket_pair_colors =
    Colour bracket pairs by how deeply they nest, in the theme's three bracket
    colours.

settings-template-editor-render_whitespace =
    Which spaces and tabs show a mark, in the theme's whitespace colour: "none",
    "boundary" (all but a single space between words), "selection" (only those
    selected), "trailing" (only those after a row's last other character) or
    "all".

settings-template-editor-rulers = A thin vertical line at each of these columns, such as [80, 120].

settings-template-editor-line_numbers =
    The gutter's line numbers: "on", "off", "relative" (rows from the caret's,
    which shows 0) or "hybrid" (relative, the caret's row showing its number).
    :set number and :set relativenumber change one editor's.

settings-template-editor-cursor_surrounding_lines =
    How many rows autoscroll keeps between the caret and the window's edge, 0 to
    100; a value past half the window centres the caret. :set scrolloff changes
    one editor's.

settings-template-editor-cursor_line =
    Fill the caret's row with a band. nvim's default is off and VS Code's on;
    Tenmoku keeps the editor default. :set cursorline changes one editor's.

settings-template-editor-inline_diagnostics =
    Show a row's most severe diagnostic message, dimmed, after the row's end:
    "off", "error", "warning" (errors and warnings) or "all" (information and
    hints too).

settings-template-editor-spell_check = Underline misspelled words.

settings-template-editor-spell_check_scopes =
    Where words are checked: "comments" and "strings" in code, and "text" for
    plain text and Markdown prose.

settings-template-editor-spell_check_language =
    The language to check words in: "auto" for the system's languages, or a
    BCP-47 tag such as "en-GB".

settings-template-editor-todo_comments =
    Highlight TODO:, FIXME:, HACK:, NOTE:, PERF: and WARN: keywords (and their
    aliases) in comments.

settings-template-editor-minimap-enabled = Show the minimap: the whole file drawn small beside the text.

settings-template-editor-minimap-side = Which side of the text the minimap is on: "right" or "left".

settings-template-editor-minimap-render_characters = Draw each character's shape; false draws blocks of colour.

settings-template-editor-minimap-max_column = The most columns of a row the minimap draws (at least 1).

settings-template-editor-minimap-scale =
    How big the minimap draws, 1 to 3: each column scale pixels wide and each
    row 2 × scale pixels tall (3 × scale drawing blocks).

settings-template-editor-minimap-show_slider = When the slider over the visible rows shows: "mouseover" or "always".

settings-template-editor-minimap-size =
    How tall the minimap's rows are: "proportional" keeps their size and
    scrolls the minimap with the editor, "fill" stretches or shrinks them so the
    file fills the minimap's height, and "fit" shrinks them only if it doesn't
    fit.

settings-template-editor-sticky_scroll-enabled =
    Pin the header rows of the functions, types and other scopes the top of the
    view is inside, as you scroll.

settings-template-editor-sticky_scroll-max_line_count = The most header rows pinned at once, 1 to 20.

settings-template-editor-sticky_scroll-default_model =
    Where the pinned scopes come from first: "outline" (the file's symbols),
    "folding" (the ranges the fold gutter folds) or "indentation". Each falls
    back to the next for a file it can't serve: the outline for a language
    without one, the folding ranges for text without a syntax tree.

settings-template-editor-sticky_scroll-scroll_with_editor =
    The pinned rows' text scrolls sideways with the editor's; false keeps it at
    the first column. Their line numbers never scroll.

settings-template-vim-enabled = Whether the editor starts in vim's normal mode, where keys run vim commands.

settings-template-vim-clipboard = Set to "unnamedplus" to share yanks and puts with the system clipboard.

settings-template-vim-jump_on_s =
    true: s and S start the forward and backward jump, as leap and flash map
    them, instead of changing a char or a line. gs and gS start it either way.

settings-template-vim-jump_style =
    How the jump reads its target: "leap" takes two chars, then labels; "flash"
    takes one char, then labels every match in the view, before and after the
    caret (gS before only).

settings-template-vim-ignorecase =
    true: a search ignores case, as nvim's 'ignorecase'. \c and \C in a pattern
    override it.

settings-template-vim-smartcase =
    true: with ignorecase on, a search pattern with an uppercase letter matches
    case, as nvim's 'smartcase'.

settings-template-vim-incsearch = true: a search shows its first match as the pattern is typed.

settings-template-vim-inccommand = true: a :s shows what it would replace as it is typed (nvim's inccommand).

settings-template-vim-replace_with_register =
    true: gr{"{"}motion{"}"}, grr and visual gr replace text with a register's contents,
    as the ReplaceWithRegister plugin does, instead of running nvim 0.12's
    language-server gr keys (grr references, grn rename, gra code action, gri,
    grt).

settings-template-vim-subword_motions =
    true: w, b, e and ge stop at each part of a camelCase, PascalCase, snake_case
    or digit-joined name, as vim-wordmotion does; W B E gE still move by WORDs.
    iS and aS select a subword either way.

settings-template-vim-regex_time_limit =
    How long, in milliseconds, a vim search or :s may run before it stops and
    changes nothing.

settings-template-vim-shell =
    The program vim's ! filter and :r ! run commands with. Empty: your shell
    ($SHELL, else /bin/sh; %COMSPEC% on Windows). A project file can't set it.

settings-template-vim-filter_time_limit =
    How long, in milliseconds, a ! filter or :r ! may run before it is killed
    and changes nothing.

settings-template-vim-which_key =
    true: a popup lists the keys that go on from a pending prefix (g, z, <leader>,
    a remap's first keys), once which_key_delay_ms passes with no key.

settings-template-vim-which_key_delay_ms =
    How long, in milliseconds, a prefix waits for its next key before the
    which-key popup shows. 0 shows it at once.

settings-template-vim-highlight_yank = true: a yank briefly highlights the text it took, as nvim's vim.hl.on_yank does.

settings-template-vim-highlight_yank_ms = How long, in milliseconds, the yank highlight stays.

settings-template-vim-quick_scope =
    Underlines the first char of each word on the caret's row that f or F reaches
    in one or two presses, as quick-scope does: "always" in normal and visual
    mode; "on_keys" only while f, F, t or T waits for its char, in that direction;
    "off" never.

settings-template-vim-leader =
    The key <leader> stands for in a remap, in vim notation. Left at "\\", a
    preset's leader applies.

settings-template-vim-preset =
    A bundled set of remaps beneath your vimrc and [[vim.remap]], which replace
    any of its maps for the same keys: "none" (plain vim) or "lazyvim" (LazyVim's,
    with <Space> as the leader).

settings-template-vim-remap =
    Remaps, as vim's :map makes them. mode is "n", "x" (or "v"), "o", "i", "c", or a
    list of them; recursive = true remaps the keys `to` produces too.

settings-template-vim-which_key_group =
    Names for groups of remaps, which the which-key popup shows as +name in place
    of +count; after the vimrc's :fegroup and the preset's, a later name for the
    same keys wins. mode is as vim.remap's, "n" unless set. icon, if set, names a
    Lucide icon drawn beside the group ("search", "git-branch", "bug"); unset
    keeps the one an earlier name gives.

settings-template-status_bar-visible = Whether the status bar shows at the bottom of the window.

settings-template-workbench-preview_tabs =
    Open a file single-clicked in the project tree in a preview tab, in
    italics, that the next one replaces. Editing it, double-clicking it or its
    tab, or pinning it keeps it open.

settings-template-workbench-icon_theme =
    The file and folder icons: tenmoku draws line icons tinted from the theme,
    by file type and folder role; material draws Material Icon Theme's, in
    colour, by language, file name and folder name; minimal draws plain line
    icons in the theme's colours. A project file can't set it.

settings-template-workbench-start_page =
    Show the Start page in a window's empty pane while no tabs are open: New
    File, Open File, Open Folder, the recent folders and files, and the
    settings, with LazyVim's dashboard keys while vim is on. Off, the pane says
    "No open files".

settings-template-workbench-rail =
    Which edge of the window the activity rail is on, "left" or "right". The
    sidebar opens beside it, so they move together, at once.

settings-template-workbench-reduce_motion =
    Whether animation is reduced: "system" follows the system's Reduce Motion
    option, "on" and "off" decide it here, whatever the system says.

settings-template-workbench-locale =
    The language of Tenmoku's own text: auto follows the system's preferred
    languages; a tag such as "de" or "pt-BR" picks a shipped one. Menus and
    panels change at once; macOS's own panels and the updater follow the
    system (System Settings > Language & Region > Applications). A language
    Tenmoku doesn't ship is skipped, with a warning naming the one used. A
    project file can't set it.

settings-template-workbench-zen_mode-hide_tabs =
    What Zen Mode (cmd-k z, ctrl-k z off macOS, again to leave it) hides
    beyond the docks, the panels and the activity rail, which it always hides.
    Hide the panes' tab strips and breadcrumbs.

settings-template-workbench-zen_mode-hide_status_bar = Hide the status bar.

settings-template-workbench-zen_mode-center_layout =
    Center the editors, with margins of a fifth of the window's width on each
    side.

settings-template-theme-mode =
    dark, light, or system to follow the system's appearance. Your own colours
    go in theme.toml beside this file; its appearance says whether it replaces
    the dark or the light theme.

settings-template-theme-dark =
    The installed theme the dark theme is, by ID: an extension's theme file, as
    in "acme.solarized/solarized-dark". Unset is Yohen, Tenmoku's own dark
    theme. Your theme.toml goes over it.

settings-template-theme-light =
    The installed theme the light theme is, by ID. Unset is Hakuji, Tenmoku's
    own light theme.

settings-template-theme-name =
    A bundled theme by name: Yohen, Hakuji, Dark Modern, Dark 2026, Light 2026,
    Catppuccin Latte, Catppuccin Frappe, Catppuccin Macchiato, Catppuccin Mocha,
    Monokai, Tenmoku Dark, Tenmoku Light or Tenmoku Ember ("dark" and "light"
    are Yohen and Hakuji). It's the dark or the light theme, as its appearance
    is; dark or light, if set, wins for that one.

settings-template-terminal-shell =
    The program the terminal runs. Empty runs your login shell. A project file
    can't set it.

settings-template-terminal-shell_args =
    The shell's arguments. Ignored when shell is empty. A project file can't
    set it.

settings-template-terminal-scrollback = How many lines the terminal keeps above the screen.

settings-template-terminal-font_family = The terminal's font family. Unset uses the editor's.

settings-template-terminal-font_size =
    The terminal's font size, in points. Unset, 12, the design's small code
    size.

settings-template-terminal-font_fallbacks =
    Fonts to take a character from when the terminal's font lacks it, in
    order: "Symbols Nerd Font Mono" gives Nerd Font icons with any font.
    Or set font_family to a Nerd Font, such as "JetBrainsMono Nerd Font Mono".

settings-template-chat-model =
    The Claude model new chats use. A chat keeps the model it started with. A
    project file can't set any [chat] key.

settings-template-chat-effort =
    How much Claude thinks before it answers: low, medium, high, xhigh or max.
    Lower is faster and cheaper. Applies to new chats.

settings-template-chat-show_thinking = Show a summary of Claude's reasoning above each reply. Applies to new chats.

settings-template-chat-max_tokens = The most a reply may use, its thinking included (1024 to 128000).

settings-template-chat-context_budget =
    The most tokens of attached context (files, selection, diagnostics) one
    message sends (0 to 500000).

settings-template-chat-fallbacks =
    When Claude declines a request, let Anthropic's API answer with its
    recommended fallback model.

settings-template-update-automatic_checks =
    Check for a newer Tenmoku in the background once a day and offer to install
    it. Check for Updates… always works.

settings-template-extensions-disabled =
    The IDs of installed extensions to turn off, as in ["acme.solarized"]: they
    stay installed but contribute nothing. An ID no extension has is ignored. A
    project file can't set it.

settings-template-extensions-plugins_preview =
    Run code plugins (WebAssembly) from installed extensions. A preview, off by
    default: their API can change in any release. A project file can't set it.

settings-template-formatters-typescript =
    External formatters, one table per language, named as the language is
    (rust, python, typescript, go, ...). A project file can't set them. A table
    needs a command; the rest are optional:
    {"  "}command    the program, then its arguments. {"{"}file{"}"} is replaced with the
    {"             "}buffer's absolute path, so prettier finds its config.
    {"  "}stdin      true (default): the text goes to stdin, the result comes from
    {"             "}stdout. false: a temporary copy is passed as {"{"}file{"}"} and read
    {"             "}back, for tools that only format files.
    {"  "}prefer     "external" (default) or "lsp": which runs first; the other is
    {"             "}the fallback when the first has nothing to do or fails.
    {"  "}timeout_ms how long the program may run, 1 to 60000 (default 3000).
    An entry with a bad value is ignored, with a warning; the others apply.
