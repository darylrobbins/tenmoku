### The Settings editor's text: generated from fe-config's settings schema,
### whose doc comments are the en-US source. Don't edit it by hand: change
### the doc comment, then rerun fe-config's tests with UPDATE_GOLDEN=1.

## The Settings editor's group titles, by table name.

settings-group-chat = Chat
settings-group-editor = Editor
settings-group-minimap = Minimap
settings-group-sticky_scroll = Sticky Scroll
settings-group-extensions = Extensions
settings-group-lsp = LSP
settings-group-timeouts = Timeouts
settings-group-status_bar = Status Bar
settings-group-terminal = Terminal
settings-group-theme = Theme
settings-group-tree = Tree
settings-group-update = Update
settings-group-vim = Vim
settings-group-workbench = Workbench
settings-group-zen_mode = Zen Mode

## Each table's and setting's description, by its dotted path, with an
## attribute for each value that has one of its own.

settings-chat = The AI chat.

settings-chat-context_budget =
    The most tokens of attached context (files, selection, diagnostics)
    one message sends.

settings-chat-effort =
    How much Claude thinks before it answers: lower is faster and
    cheaper. Applies to new chats.
    .low = The least thinking.
    .medium = A balance of speed and depth.
    .high = More thinking.
    .xhigh = Even more thinking.
    .max = The most thinking.

settings-chat-fallbacks =
    When Claude declines a request, let Anthropic's API answer with its
    recommended fallback model.

settings-chat-max_tokens = The most a reply may use, its thinking included.

settings-chat-model = The Claude model new chats use. A chat keeps the model it started with.

settings-chat-show_thinking =
    Show a summary of Claude's reasoning above each reply. Applies to new
    chats.

settings-editor = The text editor.

settings-editor-auto_close_pairs =
    Type the closing bracket or quote when typing an opening one, type over
    it, and delete both with backspace. Off, typing inserts only what's
    typed.

settings-editor-auto_save = When a changed file saves itself.
    .off = Files save only when you save them.
    .after_delay =
        Save a changed file `auto_save_delay` ms after the last edit; in vim
        insert mode, once insert mode ends.
    .on_focus_change = Save a changed file when its editor loses focus or the window does.
    .on_window_change = Save changed files when the window loses focus.
    .on_insert_leave =
        With vim on, save a changed file when insert mode ends and after each
        normal-mode change; with vim off, as `on_focus_change`.

settings-editor-auto_save_delay =
    How long after the last edit, in milliseconds, `after_delay` saves.
    Used only by `after_delay`.

settings-editor-bracket_pair_colors =
    Colour bracket pairs by how deeply they nest, in the theme's
    `bracket_1`, `bracket_2` and `bracket_3`, then round again. Brackets
    in strings and comments, and ones without a pair, keep their colour.

settings-editor-code_actions_on_save =
    Code action kinds to run, in order, when you save a file, before
    `format_on_save`, such as `["source.organizeImports", "source.fixAll"]`.

settings-editor-code_lens = Show language servers' code lenses ("3 references", "Run") above rows.

settings-editor-color_decorators =
    Draw a small square in the colour before each colour in the text
    (CSS, SCSS, Tailwind…), from a language server or found by Tenmoku,
    as `default_color_decorators` says. Off, there are none of either.

settings-editor-cursor_line =
    Fill the caret's row with the theme's active-line colour, as VS
    Code's `editor.renderLineHighlight` and vim's 'cursorline'. nvim's
    default is off and VS Code's on; Tenmoku keeps the editor default.
    `:set cursorline` changes one editor's.

settings-editor-cursor_surrounding_lines =
    How many rows autoscroll keeps between the caret and the window's
    edge, as VS Code's `editor.cursorSurroundingLines` and vim's
    'scrolloff'. A value past half the window's rows centres the caret.
    `:set scrolloff` changes one editor's.

settings-editor-default_color_decorators =
    Where colour squares come from: `"auto"` finds colours itself (CSS
    colours, and `#rrggbb` in other languages' strings) unless the
    language server finds some, `"always"` finds them itself and never
    asks the server, `"never"` shows only the server's.
    .auto =
        Tenmoku's own squares, unless the language server finds colours:
        then the server's in their place.
    .always = Tenmoku's own squares only; the server isn't asked for colours.
    .never = The language server's squares only.

settings-editor-font_family =
    The font family. Unset, Spline Sans Mono, which Tenmoku bundles, or
    else the first installed of JetBrains Mono, SF Mono, Menlo, DejaVu Sans
    Mono, Ubuntu Mono, Liberation Mono and Noto Sans Mono.

settings-editor-font_ligatures =
    Draw the font's ligatures, such as `=>` as ⇒ and `->` as →. Off, as in
    VS Code's `editor.fontLigatures`.

settings-editor-font_size = The font size, in points.

settings-editor-format_on_paste =
    Format pasted text spanning more than one row with the language
    server's range formatting, which one undo takes back with the paste.

settings-editor-format_on_save =
    Format a file with its language server before saving it. A server that
    doesn't answer within a second is skipped and the file saves unformatted.

settings-editor-format_on_type =
    Format the row as you type with the language server's on-type
    formatting (`textDocument/onTypeFormatting`): typing one of its
    trigger characters, such as `{"}"}` or `;`, with one caret applies its
    edits, which one undo takes back with the character.

settings-editor-indent_guides = Draw a thin vertical line at each indent level.

settings-editor-inlay_hints =
    Show language servers' inlay hints (type and parameter names) inside
    the text.

settings-editor-inline_blame =
    Show the caret row's git blame after the line in every new window; Git:
    Toggle Line Blame turns it on or off for one window.

settings-editor-inline_diagnostics =
    Which diagnostics show their message, dimmed, after the end of their
    row, as VS Code's Error Lens extension: `"off"`, `"error"`,
    `"warning"` (errors and warnings) or `"all"` (information and hints
    too).
    .off = No messages after rows.
    .error = Errors' messages only.
    .warning = Errors' and warnings' messages.
    .all = Every diagnostic's message, information and hints too.

settings-editor-lightbulb =
    Show a lightbulb by the caret's row when a language server has quick
    fixes or refactorings there.

settings-editor-line_height =
    The line height, as a multiple of the font size. Rows round to whole
    pixels: 1.54 gives 20 px rows at 13 px.

settings-editor-line_numbers =
    The gutter's line numbers: `"on"` (absolute), `"off"`, `"relative"`
    (rows from the caret's, as vim's `nonu rnu`) or `"hybrid"`
    (relative, the caret's row absolute, as vim's `nu rnu`). `:set nu`
    and `:set rnu` change one editor's.
    .on = Each row's number.
    .off = No numbers.
    .relative = How many rows each row is from the caret's, which shows 0.
    .hybrid = Relative, with the caret's row showing its number.

settings-editor-linked_editing =
    Rename an HTML tag's pair as you type in the open or close tag, and
    other ranges a language server links (`textDocument/linkedEditingRange`).

settings-editor-links =
    Underline language servers' links (URLs and file paths) under the
    pointer while cmd (ctrl off macOS) is held, and open them on
    cmd-click.

settings-editor-minimap =
    The minimap: the whole file drawn small beside the text, VS Code's
    `editor.minimap.*`.

settings-editor-minimap-enabled = Show the minimap.

settings-editor-minimap-max_column = The most columns of a row the minimap draws.

settings-editor-minimap-render_characters = Draw each character's shape; off, draw blocks of colour.

settings-editor-minimap-scale =
    How big the minimap draws: each column scale pixels wide and each
    row 2 × scale pixels tall (3 × scale drawing blocks).

settings-editor-minimap-show_slider = When the slider over the visible rows shows.
    .mouseover = While the pointer is over the minimap.
    .always = All the time.

settings-editor-minimap-side = Which side of the text the minimap is on.
    .right = Right of the text.
    .left = Left of the text.

settings-editor-minimap-size = How tall the minimap's rows are.
    .proportional =
        Rows keep their size, and the minimap scrolls with the editor when
        they don't all fit.
    .fill =
        Rows stretch or shrink so the file fills the minimap's height, no
        taller than the editor's rows; it never scrolls.
    .fit =
        As `proportional` when the file fits the minimap's height, else
        shrunk as `fill`.

settings-editor-persistent_undo =
    Keep each file's undo history on disk when it's saved or closed, so
    `u` and `<C-r>` reach edits made before it was last closed, as nvim's
    `undofile`. History is kept only while the file's text is what it was
    left at; one changed outside the editor starts with none. Off, none is
    read or written, and none kept is removed.

settings-editor-render_whitespace =
    Which spaces and tabs show a mark, a dot or an arrow, in the theme's
    `whitespace` colour, as VS Code's `editor.renderWhitespace`.
    .none = No marks.
    .boundary = Every space and tab except a single space between two words.
    .selection = Only spaces and tabs inside a selection.
    .trailing = Only spaces and tabs after a row's last other character.
    .all = Every space and tab.

settings-editor-rulers =
    A thin vertical line at each of these columns, in the theme's `ruler`
    colour, as VS Code's `editor.rulers`. Columns of 0 or less are
    ignored.

settings-editor-semantic_highlighting =
    Colour code with language servers' semantic tokens over the syntax
    colours.

settings-editor-soft_wrap =
    Whether long lines wrap at the window's edge. `alt-z` toggles it per
    editor.

settings-editor-spell_check = Underline misspelled words.

settings-editor-spell_check_language =
    The language to check words in: `"auto"` for the system's languages,
    or a BCP-47 tag such as `"en-GB"`.

settings-editor-spell_check_scopes =
    Where words are checked: `"comments"` and `"strings"` in code, and
    `"text"` for plain text and Markdown prose. Anything else is ignored,
    with a warning.

settings-editor-sticky_scroll =
    Sticky scroll: the header rows of the scopes enclosing the top of the
    view stay pinned there, VS Code's `editor.stickyScroll.*`.

settings-editor-sticky_scroll-default_model =
    Where the pinned scopes come from first. Each falls back to the next
    for a file it can't serve: the outline for a language without one,
    the folding ranges for text without a syntax tree.
    .outline =
        The file's symbols: functions, types, sections. Languages without an
        outline use folding.
    .folding =
        The ranges the fold gutter folds. Text without a syntax tree uses
        indentation.
    .indentation = Blocks of rows indented under a less indented one.

settings-editor-sticky_scroll-enabled =
    Pin the header rows of the functions, types and other scopes the top
    of the view is inside, as you scroll.

settings-editor-sticky_scroll-max_line_count = The most header rows pinned at once.

settings-editor-sticky_scroll-scroll_with_editor =
    The pinned rows' text scrolls sideways with the editor's; off, it
    stays at the first column. Their line numbers never scroll.

settings-editor-tab_size = How many columns a tab advances to the next multiple of.

settings-editor-todo_comments =
    Highlight `TODO:`, `FIXME:`, `HACK:`, `NOTE:`, `PERF:` and `WARN:`
    keywords (and their aliases) in comments.

settings-editor-wrap_column =
    The column soft wrap wraps at, 20 to 500, for `wrap_mode` `column` and
    `bounded`; 0 leaves wrapping at the window's edge. Only meaningful
    when `soft_wrap` is on.

settings-editor-wrap_mode =
    Where soft wrap wraps: `viewport` (the window's edge), `column`
    (`wrap_column` columns whatever the window, so wider text scrolls) or
    `bounded` (the smaller of the two). Only meaningful when `soft_wrap`
    is on, as VS Code's `editor.wordWrap`.
    .viewport = Wrap at the window's edge.
    .column = Wrap at `wrap_column` columns, however wide the window.
    .bounded = Wrap at the smaller of the window's edge and `wrap_column` columns.

settings-extensions = Installed extensions. A project file can't set them.

settings-extensions-disabled =
    The IDs of installed extensions that contribute nothing, as in
    "acme.solarized": they stay installed and listed. An ID no
    extension has is ignored.

settings-extensions-plugins_preview =
    Run code plugins (WebAssembly) from installed extensions. A preview:
    their API can change in any release. Each plugin still asks for what
    it may do before it runs.

settings-lsp =
    Language servers. A project file may only turn them off, but in a
    trusted folder its settings for them apply.

settings-lsp-enabled = Whether language servers run at all.

settings-lsp-max_file_bytes =
    The largest file, in bytes, synced to a language server. Applies to
    files opened after it changes.

settings-lsp-servers =
    Servers by name: changes to the built-in rust-analyzer, pyright,
    typescript-language-server, typescript-go, gopls,
    vscode-css-language-server, jdtls, kotlin-lsp and csharp-ls, or servers of your own, which
    need a `command`.

settings-lsp-timeouts = How long requests may take, in milliseconds, by method.

settings-lsp-timeouts-code_action =
    Code actions (cmd-.), an explicit request: the picker says there are
    none only once this has passed.

settings-lsp-timeouts-definition = Go to definition.

settings-lsp-timeouts-references = Find references.

settings-lsp-timeouts-resolve = Resolving a completion item.

settings-lsp-timeouts-signature_help = Signature help, which pops up while typing.

settings-lsp-timeouts-workspace_symbol = Workspace symbols.

settings-lsp-typescript_server = Which built-in server TypeScript and JavaScript files get.
    .typescript-language-server = typescript-language-server, over tsserver.
    .typescript-go = typescript-go's `tsc --lsp`.

settings-status_bar = The status bar.

settings-status_bar-visible =
    Whether the status bar shows at the bottom of the window. View >
    Toggle Status Bar flips it and saves it here.

settings-terminal = The terminal panel.

settings-terminal-font_fallbacks =
    Fonts to take a character from when the terminal's font lacks it, in
    order: "Symbols Nerd Font Mono" gives Nerd Font icons with any font.

settings-terminal-font_family = The terminal's font family. Unset uses the editor's.

settings-terminal-font_size =
    The terminal's font size, in points. Unset, 12, the design's small
    code size.

settings-terminal-scrollback =
    How many lines the terminal keeps above the screen. Applies to
    terminals started after it changes.

settings-terminal-shell =
    The program the terminal runs. Empty runs your login shell. A running
    terminal keeps its shell; the next one started uses the new one.

settings-terminal-shell_args = The shell's arguments. Ignored when `shell` is empty.

settings-theme = The colour theme.

settings-theme-dark =
    The installed theme the dark theme is, by ID: an extension's
    theme file, as in "acme.solarized/solarized-dark". Unset is Yohen,
    Tenmoku's own dark theme.

settings-theme-light =
    The installed theme the light theme is, by ID. Unset is Hakuji,
    Tenmoku's own light theme.

settings-theme-mode =
    dark, light, or system to follow the system's appearance. Your own
    colours go in theme.toml beside this file; its appearance says whether
    it replaces the dark or the light theme.
    .dark = The dark theme.
    .light = The light theme.
    .system = The dark or light theme as the system's appearance is.

settings-theme-name =
    A bundled theme by its title or ID, as in "Dark Modern" or "Tenmoku
    Ember": the dark or the light theme, as its appearance is. "dark" and
    "light" are Yohen and Hakuji. theme.dark or theme.light, if set, wins
    for that one.

settings-tree = The project tree.

settings-tree-show_ignored =
    List entries that `.gitignore` and `.ignore` files match (`target`,
    `.idea`), greyed. Off hides them. `.git` is never listed. Applies to
    trees opened after it changes.

settings-update = Updates to Tenmoku itself.

settings-update-automatic_checks =
    Check for a newer Tenmoku in the background once a day and offer to
    install it. Check for Updates… always works.

settings-vim = Vim mode.

settings-vim-clipboard =
    Which register vim's unnamed register shares with the system
    clipboard: `"unnamedplus"` makes `yy` copy to it and `p` paste from
    it, as nvim's `clipboard=unnamedplus` does. Empty, only `"+` and `"*`
    reach it.
    .empty = Only the `+` and `*` registers reach the system clipboard.
    .unnamedplus = Yanks, deletes and puts that name no register use the system clipboard.

settings-vim-enabled =
    Whether the editor starts in vim's normal mode, where keys run vim
    commands rather than type. View > Toggle Vim Mode flips it and saves
    it here.

settings-vim-filter_time_limit =
    How long, in milliseconds, a `!` filter or `:r !` may run before it is
    killed and nothing changes.

settings-vim-highlight_yank =
    true: a yank briefly highlights the text it took, as nvim's
    vim.hl.on_yank does.

settings-vim-highlight_yank_ms = How long, in milliseconds, the yank highlight stays.

settings-vim-ignorecase =
    true: a search with `/`, `?`, `n` or `N` ignores case, as nvim's
    'ignorecase' does. `\c` and `\C` in a pattern override it.

settings-vim-inccommand =
    true: a `:s` shows what it would replace as it is typed, its matches
    highlighted and each row it changes as it would read after the row,
    as nvim's 'inccommand' (`nosplit`) does; Esc puts everything back.

settings-vim-incsearch =
    true: a search shows its first match as the pattern is typed, as
    nvim's 'incsearch' does.

settings-vim-jump_on_s =
    true: `s` and `S` start the forward and backward jump, as leap and
    flash map them, instead of changing a char or a line (`cl` and `cc`
    still do). `gs` and `gS` start it either way.

settings-vim-jump_style =
    How the jump (`gs`, `gS`, and `s` and `S` with `jump_on_s`) reads
    its target: `"leap"` takes two chars, then labels the matches in the
    view; `"flash"` takes one char, then labels every match of it in the
    view at once, before and after the caret (`gS` before only), as
    flash.nvim does.
    .leap = Two chars, then a label, as leap.nvim does.
    .flash =
        One char, then a label on every match in the view, both ways, as
        flash.nvim does.

settings-vim-leader =
    The leader key `<leader>` stands for in a remap, in vim notation
    (`"<Space>"` works). Left at `"\\"`, a preset's leader applies.

settings-vim-preset =
    A bundled set of remaps loaded beneath the vimrc and `[[vim.remap]]`,
    which replace any of its maps for the same keys: `"none"`, plain vim,
    or `"lazyvim"`, LazyVim's, with `<Space>` as the leader unless
    `leader` is set.
    .none = Plain vim: no maps but the user's, and `\` as the leader.
    .lazyvim = LazyVim's maps, with `<Space>` as the leader.

settings-vim-quick_scope =
    Underlines the first char of each word on the caret's row that `f` or
    `F` reaches in one or two presses, as quick-scope does: `always` in
    normal and visual mode; `on_keys` only while `f`, `F`, `t` or `T`
    waits for its char, in that direction.
    .off = No underlines.
    .on_keys = While `f`, `F`, `t` or `T` waits for its char, that direction's.
    .always = In normal and visual mode both directions', or the waiting one's.

settings-vim-regex_time_limit =
    How long, in milliseconds, a vim search or :s may run on one command
    before it stops with Interrupted and changes nothing (nvim's
    'redrawtime' is the same default).

settings-vim-remap =
    Key remaps, as vim's `:map` makes them. Each is a `[[vim.remap]]`
    table; a later one for the same keys in the same mode replaces an
    earlier one.

settings-vim-replace_with_register =
    true: `gr{"{"}motion{"}"}`, `grr` and visual `gr` replace text with a
    register's contents, as the ReplaceWithRegister plugin does, instead
    of running nvim 0.12's language-server `gr` keys (`grr` references,
    `grn` rename, `gra` code action, `gri`, `grt`).

settings-vim-shell =
    The program vim's `!` filter and `:r !` run commands with, as
    `shell -c command`. Empty: your shell (`$SHELL`, else `/bin/sh`; on
    Windows `%COMSPEC%`). A project file can't set it.

settings-vim-smartcase =
    true: with `ignorecase` on, a typed search pattern with an uppercase
    letter matches case (`*` and `#` still ignore it), as nvim's
    'smartcase' does.

settings-vim-subword_motions =
    true: `w`, `b`, `e` and `ge` stop at each part of a camelCase,
    PascalCase, snake_case or digit-joined name, as vim-wordmotion does;
    `W B E gE` still move by WORDs. `iS` and `aS` select a subword either
    way.

settings-vim-which_key =
    true: a popup lists the keys that go on from a pending prefix (`g`,
    `z`, `<leader>`, a remap's first keys) with what each does, as
    which-key.nvim's does, once `which_key_delay_ms` passes with no key.

settings-vim-which_key_delay_ms =
    How long, in milliseconds, a prefix waits for its next key before the
    which-key popup shows (which-key.nvim's default). 0 shows it at once.

settings-vim-which_key_group =
    Names for groups of remaps, which the which-key popup shows as
    `+{"{"}name{"}"}` in place of `+{"{"}count{"}"}`. Each is a `[[vim.which_key_group]]`
    table; it loads after the vimrc's `:fegroup`s and the preset's, and
    a later name for the same keys in the same mode wins, as does a
    later icon.

settings-workbench = The workbench: its panes and tabs.

settings-workbench-contrast =
    Whether colours are drawn at high contrast: `system` follows the
    system's Increase Contrast option (System Settings › Accessibility ›
    Display), `high` and `normal` decide it here, whatever the system
    says. High contrast strengthens every theme's borders, focus ring,
    selections, muted text and the editor's line numbers.
    .system = As the system's Increase Contrast option is.
    .high = High contrast, whatever the system says.
    .normal = The theme's own contrast, whatever the system says.

settings-workbench-icon_theme =
    The file and folder icons: `tenmoku` draws line icons tinted from the
    theme, by file type and folder role; `material` draws Material Icon
    Theme's, in colour, by language, file name and folder name; `minimal`
    draws plain line icons in the theme's colours. A project file can't
    set it.
    .tenmoku = Line icons tinted from the theme, by file type and folder role.
    .material =
        Material Icon Theme's coloured icons, by language, file name and
        folder name.
    .minimal = Plain line icons in the theme's colours.

settings-workbench-locale =
    The language of Tenmoku's own text: `auto` follows the system's
    preferred languages; a tag such as `de` or `pt-BR` picks a shipped
    one. Menus and panels change at once; macOS's own panels and the
    updater follow the system (System Settings › Language & Region ›
    Applications). A language Tenmoku doesn't ship is skipped, with a
    warning naming the one used. A project file can't set it.

settings-workbench-preview_tabs =
    Whether a file single-clicked in the project tree opens in a preview
    tab, in italics, that the next one replaces. Editing it,
    double-clicking it or its tab, or pinning it keeps it open.

settings-workbench-rail =
    Which edge of the window the activity rail is on, `left` or
    `right`. The sidebar opens beside it, so they move together, at
    once.
    .left = On the window's left edge, the sidebar to its right.
    .right = On the window's right edge, the sidebar to its left.

settings-workbench-reduce_motion =
    Whether animation is reduced: `system` follows the system's Reduce
    Motion option (System Settings › Accessibility › Display), `on` and
    `off` decide it here, whatever the system says.
    .system = As the system's Reduce Motion option is.
    .on = Reduced, whatever the system says.
    .off = Not reduced, whatever the system says.

settings-workbench-start_page =
    Whether a window with no tabs open shows the Start page in its empty
    pane: New File, Open File, Open Folder, the recent folders and files,
    and the settings, with LazyVim's dashboard keys while vim is on.
    Off, the pane says "No open files".

settings-workbench-zen_mode = Zen Mode: the editor alone, VS Code's `zenMode.*`.

settings-workbench-zen_mode-center_layout =
    Center the editors, with margins of a fifth of the window's width on
    each side.

settings-workbench-zen_mode-hide_status_bar = Hide the status bar.

settings-workbench-zen_mode-hide_tabs = Hide the panes' tab strips and breadcrumbs.
