//! The editor's settings, layered from defaults, the user file and the
//! project file, on kiln-core's typed [`kiln_core::SettingsStore`]: a
//! project file's `lsp` keys are narrowed and its `theme` dropped
//! ([`ProjectFilter`]), and each
//! file is checked against the schema with its servers listed.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use kiln_core::{ProjectFilter, SettingsOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toml::{
    Table,
    de::{DeTable, DeValue},
};

use crate::{
    diagnostic::{Diagnostic, Severity},
    layers::Layer,
};

/// Tenmoku's settings, as written in `settings.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Settings {
    /// The text editor.
    pub editor: EditorSettings,
    /// Vim mode.
    #[serde(default)]
    pub vim: VimSettings,
    /// The status bar.
    #[serde(default)]
    pub status_bar: StatusBarSettings,
    /// The workbench: its panes and tabs.
    #[serde(default)]
    pub workbench: WorkbenchSettings,
    /// Language servers. A project file may only turn them off, but in a
    /// trusted folder its settings for them apply.
    pub lsp: LspSettings,
    /// The colour theme.
    #[serde(default)]
    pub theme: ThemeSettings,
    /// The terminal panel.
    #[serde(default)]
    pub terminal: TerminalSettings,
    /// The project tree.
    #[serde(default)]
    pub tree: TreeSettings,
    /// The AI chat.
    #[serde(default)]
    pub chat: ChatSettings,
    /// Installed extensions. A project file can't set them.
    #[serde(default)]
    pub extensions: ExtensionsSettings,
}

/// Settings for the installed extensions (extensions record §10). They're
/// the user's, not a folder's, so a project file may not set them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ExtensionsSettings {
    /// The IDs of installed extensions that contribute nothing, as in
    /// "acme.solarized": they stay installed and listed. An ID no
    /// extension has is ignored.
    pub disabled: Vec<String>,
}

/// Settings for the project tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TreeSettings {
    /// List entries that `.gitignore` and `.ignore` files match (`target`,
    /// `.idea`), greyed. Off hides them. `.git` is never listed. Applies to
    /// trees opened after it changes.
    pub show_ignored: bool,
}

impl Default for TreeSettings {
    fn default() -> Self {
        Self { show_ignored: true }
    }
}

/// Settings for the AI chat (docs/decisions/*-ai-chat.md §12). A project
/// file can't set them: a cloned repo shouldn't pick the model, raise the
/// spend or turn fallbacks off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ChatSettings {
    /// The Claude model new chats use. A chat keeps the model it started with.
    #[schemars(length(min = 1))]
    pub model: String,
    /// How much Claude thinks before it answers: lower is faster and
    /// cheaper. Applies to new chats.
    pub effort: ChatEffort,
    /// Show a summary of Claude's reasoning above each reply. Applies to new
    /// chats.
    pub show_thinking: bool,
    /// The most a reply may use, its thinking included.
    #[schemars(range(min = 1024, max = 128000))]
    pub max_tokens: u32,
    /// The most tokens of attached context (files, selection, diagnostics)
    /// one message sends.
    #[schemars(range(min = 0, max = 500000))]
    pub context_budget: u32,
    /// When Claude declines a request, let Anthropic's API answer with its
    /// recommended fallback model.
    pub fallbacks: bool,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            model: "claude-opus-5-5".into(),
            effort: ChatEffort::default(),
            show_thinking: false,
            max_tokens: 64_000,
            context_budget: 32_000,
            fallbacks: true,
        }
    }
}

/// `chat.effort`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatEffort {
    /// The least thinking.
    Low,
    /// A balance of speed and depth.
    #[default]
    Medium,
    /// More thinking.
    High,
    /// Even more thinking.
    Xhigh,
    /// The most thinking.
    Max,
}

/// Settings for the terminal panel. A project file may set all but the
/// program it runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TerminalSettings {
    /// The program the terminal runs. Empty runs your login shell. A running
    /// terminal keeps its shell; the next one started uses the new one.
    pub shell: String,
    /// The shell's arguments. Ignored when `shell` is empty.
    pub shell_args: Vec<String>,
    /// How many lines the terminal keeps above the screen. Applies to
    /// terminals started after it changes.
    #[schemars(range(min = 0, max = 100000))]
    pub scrollback: usize,
    /// The terminal's font family. Unset uses the editor's.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub font_family: Option<String>,
    /// The terminal's font size, in points. Unset uses the editor's.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 6.0, max = 72.0))]
    pub font_size: Option<f64>,
    /// Fonts to take a character from when the terminal's font lacks it, in
    /// order: "Symbols Nerd Font Mono" gives Nerd Font icons with any font.
    pub font_fallbacks: Vec<String>,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            shell: String::new(),
            shell_args: Vec::new(),
            scrollback: 10_000,
            font_family: None,
            font_size: None,
            font_fallbacks: Vec::new(),
        }
    }
}

/// Settings for the colour theme. The theme is the app's, not a window's,
/// so a project file may not set them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ThemeSettings {
    /// dark, light, or system to follow the system's appearance. Your own
    /// colours go in theme.toml beside this file; its appearance says whether
    /// it replaces the dark or the light theme.
    pub mode: ThemeModeSetting,
    /// The installed theme the dark theme is, by ID: an extension's
    /// theme file, as in "acme.solarized/solarized-dark". Unset is the
    /// built-in one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub dark: Option<String>,
    /// The installed theme the light theme is, by ID. Unset is the
    /// built-in one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub light: Option<String>,
    /// A bundled theme by its title or ID, as in "Dark Modern" or "Tenmoku
    /// Ember": the dark or the light theme, as its appearance is. theme.dark
    /// or theme.light, if set, wins for that one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub name: Option<String>,
}

/// `theme.mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThemeModeSetting {
    /// The dark theme.
    #[default]
    Dark,
    /// The light theme.
    Light,
    /// The dark or light theme as the system's appearance is.
    System,
}

/// Settings for vim mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VimSettings {
    /// Whether the editor starts in vim's normal mode, where keys run vim
    /// commands rather than type. View > Toggle Vim Mode flips it and saves
    /// it here.
    pub enabled: bool,
    /// Which register vim's unnamed register shares with the system
    /// clipboard: `"unnamedplus"` makes `yy` copy to it and `p` paste from
    /// it, as nvim's `clipboard=unnamedplus` does. Empty, only `"+` and `"*`
    /// reach it.
    pub clipboard: VimClipboard,
    /// true: `s` and `S` start the forward and backward jump, as leap and
    /// flash map them, instead of changing a char or a line (`cl` and `cc`
    /// still do). `gs` and `gS` start it either way.
    pub jump_on_s: bool,
    /// true: a search with `/`, `?`, `n` or `N` ignores case, as nvim's
    /// 'ignorecase' does. `\c` and `\C` in a pattern override it.
    pub ignorecase: bool,
    /// true: with `ignorecase` on, a typed search pattern with an uppercase
    /// letter matches case (`*` and `#` still ignore it), as nvim's
    /// 'smartcase' does.
    pub smartcase: bool,
    /// true: a search shows its first match as the pattern is typed, as
    /// nvim's 'incsearch' does.
    pub incsearch: bool,
    /// true: `gr{motion}`, `grr` and visual `gr` replace text with a
    /// register's contents, as the ReplaceWithRegister plugin does, instead
    /// of running nvim 0.12's language-server `gr` keys (`grr` references,
    /// `grn` rename, `gra` code action, `gri`, `grt`).
    pub replace_with_register: bool,
    /// How long, in milliseconds, a vim search or :s may run on one command
    /// before it stops with Interrupted and changes nothing (nvim's
    /// 'redrawtime' is the same default).
    #[schemars(range(min = 1))]
    pub regex_time_limit: u64,
    /// The program vim's `!` filter and `:r !` run commands with, as
    /// `shell -c command`. Empty: your shell (`$SHELL`, else `/bin/sh`; on
    /// Windows `%COMSPEC%`). A project file can't set it.
    pub shell: String,
    /// How long, in milliseconds, a `!` filter or `:r !` may run before it is
    /// killed and nothing changes.
    #[schemars(range(min = 100))]
    pub filter_time_limit: u64,
    /// true: a popup lists the keys that go on from a pending prefix (`g`,
    /// `z`, `<leader>`, a remap's first keys) with what each does, as
    /// which-key.nvim's does, once `which_key_delay_ms` passes with no key.
    pub which_key: bool,
    /// How long, in milliseconds, a prefix waits for its next key before the
    /// which-key popup shows (which-key.nvim's default). 0 shows it at once.
    #[schemars(range(min = 0, max = 5000))]
    pub which_key_delay_ms: u64,
    /// true: a yank briefly highlights the text it took, as nvim's
    /// vim.hl.on_yank does.
    pub highlight_yank: bool,
    /// How long, in milliseconds, the yank highlight stays.
    #[schemars(range(min = 1, max = 5000))]
    pub highlight_yank_ms: u64,
    /// The leader key `<leader>` stands for in a remap, in vim notation
    /// (`"<Space>"` works). Left at `"\\"`, a preset's leader applies.
    pub leader: String,
    /// A bundled set of remaps loaded beneath the vimrc and `[[vim.remap]]`,
    /// which replace any of its maps for the same keys: `"none"`, plain vim,
    /// or `"lazyvim"`, LazyVim's, with `<Space>` as the leader unless
    /// `leader` is set.
    pub preset: VimPreset,
    /// Key remaps, as vim's `:map` makes them. Each is a `[[vim.remap]]`
    /// table; a later one for the same keys in the same mode replaces an
    /// earlier one.
    pub remap: Vec<VimRemap>,
}

impl Default for VimSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            clipboard: VimClipboard::default(),
            jump_on_s: false,
            ignorecase: false,
            smartcase: false,
            incsearch: true,
            replace_with_register: false,
            // `fe_vim::search::DEFAULT_TIME_LIMIT`; fe-config doesn't depend on fe-vim.
            regex_time_limit: 2000,
            shell: String::new(),
            filter_time_limit: 10_000,
            which_key: true,
            which_key_delay_ms: 200,
            highlight_yank: true,
            highlight_yank_ms: 150,
            leader: "\\".to_string(),
            preset: VimPreset::default(),
            remap: Vec::new(),
        }
    }
}

/// One `[[vim.remap]]` entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VimRemap {
    /// The modes the remap applies in: one of `"n"` (normal), `"x"` or `"v"`
    /// (visual), `"o"` (operator-pending), `"i"` (insert and replace), `"c"`
    /// (command line), or a list of them.
    pub mode: VimRemapModes,
    /// The keys to map, in vim notation (`"<leader>w"`, `"H"`).
    pub from: String,
    /// The keys they stand for, in vim notation (`":w<CR>"`).
    pub to: String,
    /// true: the keys `to` produces are themselves remapped, as `:map` does
    /// rather than `:noremap`.
    #[serde(default)]
    pub recursive: bool,
}

/// `vim.remap.mode`: one mode or a list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum VimRemapModes {
    One(VimRemapMode),
    Many(Vec<VimRemapMode>),
}

impl VimRemapModes {
    pub fn modes(&self) -> &[VimRemapMode] {
        match self {
            Self::One(mode) => std::slice::from_ref(mode),
            Self::Many(modes) => modes,
        }
    }
}

/// A map mode of `vim.remap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum VimRemapMode {
    /// Normal mode.
    #[serde(rename = "n")]
    Normal,
    /// Visual mode.
    #[serde(rename = "x")]
    Visual,
    /// Visual mode, as `x` (vim's `v` also maps select mode).
    #[serde(rename = "v")]
    VisualSelect,
    /// Operator-pending mode.
    #[serde(rename = "o")]
    OperatorPending,
    /// Insert and replace mode.
    #[serde(rename = "i")]
    Insert,
    /// Command-line mode.
    #[serde(rename = "c")]
    CmdLine,
}

/// `vim.clipboard`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum VimClipboard {
    /// Only the `+` and `*` registers reach the system clipboard.
    #[default]
    #[serde(rename = "")]
    Off,
    /// Yanks, deletes and puts that name no register use the system clipboard.
    #[serde(rename = "unnamedplus")]
    Unnamedplus,
}

/// `vim.preset`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum VimPreset {
    /// Plain vim: no maps but the user's, and `\` as the leader.
    #[default]
    #[serde(rename = "none")]
    None,
    /// LazyVim's maps, with `<Space>` as the leader.
    #[serde(rename = "lazyvim")]
    Lazyvim,
}

/// Settings for the status bar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct StatusBarSettings {
    /// Whether the status bar shows at the bottom of the window. View >
    /// Toggle Status Bar flips it and saves it here.
    pub visible: bool,
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// Settings for the workbench.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct WorkbenchSettings {
    /// Whether a file single-clicked in the project tree opens in a preview
    /// tab, in italics, that the next one replaces. Editing it,
    /// double-clicking it or its tab, or pinning it keeps it open.
    pub preview_tabs: bool,
    /// Zen Mode: the editor alone, VS Code's `zenMode.*`.
    pub zen_mode: ZenModeSettings,
}

impl Default for WorkbenchSettings {
    fn default() -> Self {
        Self {
            preview_tabs: true,
            zen_mode: ZenModeSettings::default(),
        }
    }
}

/// `[workbench.zen_mode]`: what Zen Mode hides beyond the docks, the
/// panels and the activity rail, which it always hides, after VS Code's
/// `zenMode.*` settings, with its defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ZenModeSettings {
    /// Hide the panes' tab strips and breadcrumbs.
    pub hide_tabs: bool,
    /// Hide the status bar.
    pub hide_status_bar: bool,
    /// Center the editors, with margins of a fifth of the window's width on
    /// each side.
    pub center_layout: bool,
}

impl Default for ZenModeSettings {
    fn default() -> Self {
        Self {
            hide_tabs: true,
            hide_status_bar: true,
            center_layout: true,
        }
    }
}

/// Settings for the text editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EditorSettings {
    /// The font family. Unset, the first installed of JetBrains Mono, SF Mono,
    /// Menlo, DejaVu Sans Mono, Ubuntu Mono, Liberation Mono and Noto Sans Mono.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub font_family: Option<String>,
    /// The font size, in points.
    #[schemars(range(min = 6.0, max = 72.0))]
    pub font_size: f64,
    /// The line height, as a multiple of the font size.
    #[schemars(range(min = 1.0, max = 3.0))]
    pub line_height: f64,
    /// How many columns a tab advances to the next multiple of.
    #[schemars(range(min = 1, max = 16))]
    pub tab_size: usize,
    /// Whether long lines wrap at the window's edge. `alt-z` toggles it per
    /// editor.
    pub soft_wrap: bool,
    /// Format a file with its language server before saving it. A server that
    /// doesn't answer within a second is skipped and the file saves unformatted.
    pub format_on_save: bool,
    /// Show language servers' inlay hints (type and parameter names) inside
    /// the text.
    pub inlay_hints: bool,
    /// Colour code with language servers' semantic tokens over the syntax
    /// colours.
    pub semantic_highlighting: bool,
    /// Show a lightbulb by the caret's row when a language server has quick
    /// fixes or refactorings there.
    pub lightbulb: bool,
    /// Show language servers' code lenses ("3 references", "Run") above rows.
    pub code_lens: bool,
    /// Underline language servers' links (URLs and file paths) under the
    /// pointer while cmd (ctrl off macOS) is held, and open them on
    /// cmd-click.
    pub links: bool,
    /// Draw a small square in the colour before each colour a language
    /// server finds in the text (CSS, SCSS, Tailwind…).
    pub color_decorators: bool,
    /// Rename an HTML tag's pair as you type in the open or close tag, and
    /// other ranges a language server links (`textDocument/linkedEditingRange`).
    pub linked_editing: bool,
    /// Code action kinds to run, in order, when you save a file, before
    /// `format_on_save`, such as `["source.organizeImports", "source.fixAll"]`.
    pub code_actions_on_save: Vec<String>,
    /// Type the closing bracket or quote when typing an opening one, type over
    /// it, and delete both with backspace. Off, typing inserts only what's
    /// typed.
    pub auto_close_pairs: bool,
    /// When a changed file saves itself.
    pub auto_save: AutoSave,
    /// How long after the last edit, in milliseconds, `after_delay` saves.
    /// Used only by `after_delay`.
    #[schemars(range(min = 100, max = 60000))]
    pub auto_save_delay: u64,
    /// Draw a thin vertical line at each indent level.
    pub indent_guides: bool,
    /// Show the caret row's git blame after the line in every new window; Git:
    /// Toggle Line Blame turns it on or off for one window.
    pub inline_blame: bool,
    /// Colour bracket pairs by how deeply they nest, in the theme's
    /// `bracket_1`, `bracket_2` and `bracket_3`, then round again. Brackets
    /// in strings and comments, and ones without a pair, keep their colour.
    pub bracket_pair_colors: bool,
    /// The minimap: the whole file drawn small beside the text, VS Code's
    /// `editor.minimap.*`.
    pub minimap: MinimapSettings,
    /// Sticky scroll: the header rows of the scopes enclosing the top of the
    /// view stay pinned there, VS Code's `editor.stickyScroll.*`.
    pub sticky_scroll: StickyScrollSettings,
    /// Draw the font's ligatures, such as `=>` as ⇒ and `->` as →. Off, as in
    /// VS Code's `editor.fontLigatures`.
    pub font_ligatures: bool,
    /// Which spaces and tabs show a mark, a dot or an arrow, in the theme's
    /// `whitespace` colour, as VS Code's `editor.renderWhitespace`.
    pub render_whitespace: RenderWhitespace,
    /// A thin vertical line at each of these columns, in the theme's `ruler`
    /// colour, as VS Code's `editor.rulers`. Columns of 0 or less are
    /// ignored.
    pub rulers: Vec<i32>,
    /// The gutter's line numbers: `"on"` (absolute), `"off"`, `"relative"`
    /// (rows from the caret's, as vim's `nonu rnu`) or `"hybrid"`
    /// (relative, the caret's row absolute, as vim's `nu rnu`). `:set nu`
    /// and `:set rnu` change one editor's.
    pub line_numbers: LineNumbersSetting,
    /// How many rows autoscroll keeps between the caret and the window's
    /// edge, as VS Code's `editor.cursorSurroundingLines` and vim's
    /// 'scrolloff'. A value past half the window's rows centres the caret.
    /// `:set scrolloff` changes one editor's.
    #[schemars(range(min = 0, max = 100))]
    pub cursor_surrounding_lines: u32,
    /// Fill the caret's row with the theme's active-line colour, as VS
    /// Code's `editor.renderLineHighlight` and vim's 'cursorline'. nvim's
    /// default is off and VS Code's on; Tenmoku keeps the editor default.
    /// `:set cursorline` changes one editor's.
    pub cursor_line: bool,
}

/// `editor.line_numbers`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineNumbersSetting {
    /// Each row's number.
    #[default]
    On,
    /// No numbers.
    Off,
    /// How many rows each row is from the caret's, which shows 0.
    Relative,
    /// Relative, with the caret's row showing its number.
    Hybrid,
}

/// `editor.render_whitespace`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RenderWhitespace {
    /// No marks.
    None,
    /// Every space and tab except a single space between two words.
    Boundary,
    /// Only spaces and tabs inside a selection.
    #[default]
    Selection,
    /// Only spaces and tabs after a row's last other character.
    Trailing,
    /// Every space and tab.
    All,
}

/// `[editor.minimap]`: VS Code's `editor.minimap.*` settings, with its
/// defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MinimapSettings {
    /// Show the minimap.
    pub enabled: bool,
    /// Which side of the text the minimap is on.
    pub side: MinimapSide,
    /// Draw each character's shape; off, draw blocks of colour.
    pub render_characters: bool,
    /// The most columns of a row the minimap draws.
    #[schemars(range(min = 1))]
    pub max_column: u32,
    /// How big the minimap draws: each column scale pixels wide and each
    /// row 2 × scale pixels tall (3 × scale drawing blocks).
    #[schemars(range(min = 1, max = 3))]
    pub scale: u32,
    /// When the slider over the visible rows shows.
    pub show_slider: MinimapShowSlider,
    /// How tall the minimap's rows are.
    pub size: MinimapSize,
}

impl Default for MinimapSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            side: MinimapSide::default(),
            render_characters: true,
            max_column: 120,
            scale: 1,
            show_slider: MinimapShowSlider::default(),
            size: MinimapSize::default(),
        }
    }
}

/// `[editor.sticky_scroll]`: VS Code's `editor.stickyScroll.*` settings,
/// with its defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct StickyScrollSettings {
    /// Pin the header rows of the functions, types and other scopes the top
    /// of the view is inside, as you scroll.
    pub enabled: bool,
    /// The most header rows pinned at once.
    #[schemars(range(min = 1, max = 20))]
    pub max_line_count: u32,
    /// Where the pinned scopes come from first. Each falls back to the next
    /// for a file it can't serve: the outline for a language without one,
    /// the folding ranges for text without a syntax tree.
    pub default_model: StickyScrollModel,
    /// The pinned rows' text scrolls sideways with the editor's; off, it
    /// stays at the first column. Their line numbers never scroll.
    pub scroll_with_editor: bool,
}

impl Default for StickyScrollSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_line_count: 5,
            default_model: StickyScrollModel::default(),
            scroll_with_editor: true,
        }
    }
}

/// `editor.sticky_scroll.default_model`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StickyScrollModel {
    /// The file's symbols: functions, types, sections. Languages without an
    /// outline use folding.
    #[default]
    Outline,
    /// The ranges the fold gutter folds. Text without a syntax tree uses
    /// indentation.
    Folding,
    /// Blocks of rows indented under a less indented one.
    Indentation,
}

/// `editor.minimap.side`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinimapSide {
    /// Right of the text.
    #[default]
    Right,
    /// Left of the text.
    Left,
}

/// `editor.minimap.show_slider`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinimapShowSlider {
    /// While the pointer is over the minimap.
    #[default]
    Mouseover,
    /// All the time.
    Always,
}

/// `editor.minimap.size`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinimapSize {
    /// Rows keep their size, and the minimap scrolls with the editor when
    /// they don't all fit.
    #[default]
    Proportional,
    /// Rows stretch or shrink so the file fills the minimap's height, no
    /// taller than the editor's rows; it never scrolls.
    Fill,
    /// As `proportional` when the file fits the minimap's height, else
    /// shrunk as `fill`.
    Fit,
}

/// `editor.auto_save`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoSave {
    /// Files save only when you save them.
    #[default]
    Off,
    /// Save a changed file `auto_save_delay` ms after the last edit; in vim
    /// insert mode, once insert mode ends.
    AfterDelay,
    /// Save a changed file when its editor loses focus or the window does.
    OnFocusChange,
    /// Save changed files when the window loses focus.
    OnWindowChange,
    /// With vim on, save a changed file when insert mode ends and after each
    /// normal-mode change; with vim off, as `on_focus_change`.
    OnInsertLeave,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 13.0,
            line_height: 1.6,
            tab_size: 4,
            soft_wrap: false,
            format_on_save: false,
            inlay_hints: true,
            semantic_highlighting: true,
            lightbulb: true,
            code_lens: true,
            links: true,
            color_decorators: true,
            linked_editing: false,
            code_actions_on_save: Vec::new(),
            auto_close_pairs: true,
            auto_save: AutoSave::default(),
            auto_save_delay: 1000,
            indent_guides: true,
            inline_blame: false,
            bracket_pair_colors: true,
            minimap: MinimapSettings::default(),
            sticky_scroll: StickyScrollSettings::default(),
            font_ligatures: false,
            render_whitespace: RenderWhitespace::default(),
            rulers: Vec::new(),
            line_numbers: LineNumbersSetting::default(),
            cursor_surrounding_lines: 3,
            cursor_line: true,
        }
    }
}

/// Settings for language servers (docs/decisions/lsp-client.md §3.3).
///
/// A server's spec is executable configuration, so a project's settings
/// file may only turn servers off: `lsp.enabled = false` and
/// `lsp.servers.<name>.enabled = false` apply from it, and every other
/// `lsp` key there is ignored with a warning, except in a trusted folder,
/// where its language servers get them all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LspSettings {
    /// Whether language servers run at all.
    pub enabled: bool,
    /// The largest file, in bytes, synced to a language server. Applies to
    /// files opened after it changes.
    #[schemars(range(min = 0))]
    pub max_file_bytes: u64,
    /// Which built-in server TypeScript and JavaScript files get.
    pub typescript_server: TypescriptServer,
    /// How long requests may take, in milliseconds, by method.
    pub timeouts: LspTimeouts,
    /// Servers by name: changes to the built-in rust-analyzer, pyright,
    /// typescript-language-server and typescript-go, or servers of your
    /// own, which need a `command`.
    pub servers: BTreeMap<String, LspServerSettings>,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_file_bytes: 50 * 1024 * 1024,
            typescript_server: TypescriptServer::default(),
            timeouts: LspTimeouts::default(),
            servers: BTreeMap::new(),
        }
    }
}

/// The built-in server for TypeScript and JavaScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TypescriptServer {
    /// typescript-language-server, over tsserver.
    TypescriptLanguageServer,
    /// typescript-go's `tsc --lsp`.
    #[default]
    TypescriptGo,
}

/// Request timeouts, in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LspTimeouts {
    /// Go to definition.
    #[schemars(range(min = 1))]
    pub definition: u64,
    /// Workspace symbols.
    #[schemars(range(min = 1))]
    pub workspace_symbol: u64,
    /// Find references.
    #[schemars(range(min = 1))]
    pub references: u64,
    #[schemars(range(min = 1))]
    pub hover: u64,
    /// Code actions (cmd-.), an explicit request: the picker says there are
    /// none only once this has passed.
    #[schemars(range(min = 1))]
    pub code_action: u64,
    /// Signature help, which pops up while typing.
    #[schemars(range(min = 1))]
    pub signature_help: u64,
    #[schemars(range(min = 1))]
    pub completion: u64,
    /// Resolving a completion item.
    #[schemars(range(min = 1))]
    pub resolve: u64,
}

impl Default for LspTimeouts {
    fn default() -> Self {
        Self {
            definition: 5000,
            workspace_symbol: 5000,
            references: 30000,
            hover: 3000,
            code_action: 10000,
            signature_help: 3000,
            completion: 3000,
            resolve: 2000,
        }
    }
}

/// One server's settings. Each key left unset keeps the built-in value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LspServerSettings {
    /// Whether the server runs.
    pub enabled: bool,
    /// The program: a bare name is looked up on the login shell's `PATH`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1))]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Set on top of Tenmoku's environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// The languages it serves, by Tenmoku's names: rust, python,
    /// javascript, typescript, tsx and so on. A file gets the first enabled
    /// server listing its language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
    /// How Tenmoku tells the server is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness: Option<LspReadiness>,
    /// Work-done progress token prefixes that don't hold back readiness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_ignores_progress: Option<Vec<String>>,
    /// File or folder names that mark a project root, in priority order;
    /// replaces the server's built-in list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_markers: Option<Vec<String>>,
    /// Sent with `initialize`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization_options: Option<serde_json::Value>,
    /// Answers the server's `workspace/configuration` requests, by section:
    /// `[lsp.servers.rust-analyzer.settings.rust-analyzer]` is the
    /// `rust-analyzer` section. Changing it doesn't restart the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Default for LspServerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            command: None,
            args: None,
            env: None,
            languages: None,
            readiness: None,
            ready_ignores_progress: None,
            root_markers: None,
            initialization_options: None,
            settings: None,
        }
    }
}

/// How a server's readiness is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LspReadiness {
    /// rust-analyzer's `experimental/serverStatus`.
    ServerStatus,
    /// No work-done progress open.
    Progress,
}

/// The JSON schema of `settings.toml`, which `settings.schema.json` holds,
/// with its keys sorted.
pub fn settings_schema() -> serde_json::Value {
    serde_json::Value::clone(&SCHEMA)
}

static SCHEMA: LazyLock<Arc<serde_json::Value>> =
    LazyLock::new(|| Arc::new(sorted(schemars::schema_for!(Settings).to_value())));

/// `value` with every object's keys in order, whether or not another crate
/// turns on serde_json's `preserve_order`.
fn sorted(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries: Vec<_> = object.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            entries
                .into_iter()
                .map(|(key, value)| (key, sorted(value)))
                .collect()
        }
        serde_json::Value::Array(values) => values.into_iter().map(sorted).collect(),
        value => value,
    }
}

/// What a reload changed: kiln's, for Tenmoku's [`Settings`].
pub type SettingsEvent = kiln_core::SettingsEvent<Settings>;

/// The effective settings: built-in defaults, then the user file, then the
/// project file, merged key by key.
///
/// A file with an error (syntax, type or range) keeps its layer's last good
/// table, which is empty until the file has loaded cleanly once, so a typo
/// never resets a setting to its default. Warnings, such as unknown keys,
/// don't reject a file. A project file's `lsp` keys may only turn servers
/// off ([`LspSettings`]); [`trusted_lsp`] applies them all, for a trusted
/// root.
///
/// kiln-core's typed store for [`Settings`], with Tenmoku's checks:
/// [`SettingsStore::into_inner`] gives it to kiln's config service.
pub struct SettingsStore(kiln_core::SettingsStore<Settings>);

impl SettingsStore {
    /// Loads the user and project settings files, either of which may be
    /// absent or missing.
    pub fn load(user: Option<PathBuf>, project: Option<PathBuf>) -> Self {
        Self(kiln_core::SettingsStore::load_with(
            user,
            project,
            options(),
        ))
    }

    pub fn settings(&self) -> &Arc<Settings> {
        self.0.settings()
    }

    /// Every layer's diagnostics, the user file's first.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.0.diagnostics()
    }

    /// The text each layer's last good table came from, the user's first:
    /// what the store accepted, which may differ from the files now.
    pub fn accepted_sources(&self) -> Vec<&str> {
        self.0.accepted_sources()
    }

    /// The settings files, for a [`crate::ConfigWatcher`].
    pub fn paths(&self) -> Vec<PathBuf> {
        self.0.paths()
    }

    /// Re-reads the layers whose files are in `changed`. Returns what changed
    /// if the effective settings or the diagnostics did, and `None` if
    /// neither did.
    pub fn reload(&mut self, changed: &[PathBuf]) -> Option<SettingsEvent> {
        self.0.reload(changed)
    }

    /// kiln-core's store underneath, for kiln's config service.
    pub fn into_inner(self) -> kiln_core::SettingsStore<Settings> {
        self.0
    }
}

/// How Tenmoku's settings files are checked: against [`SCHEMA`] with each
/// server listed ([`schema_for`]), a project's `lsp` keys narrowed
/// ([`narrow_lsp`]) and its `theme` and `extensions` dropped
/// ([`drop_theme`], [`drop_extensions`]).
fn options() -> SettingsOptions {
    SettingsOptions::new()
        .schema_for(schema_for)
        .project_filter(ProjectFilter::new("lsp", narrow_lsp))
        .project_filter(ProjectFilter::new("theme", drop_theme))
        .project_filter(ProjectFilter::new("terminal", drop_terminal_shell))
        .project_filter(ProjectFilter::new("vim", drop_vim_user_only))
        .project_filter(ProjectFilter::new("chat", drop_chat))
        .project_filter(ProjectFilter::new("extensions", drop_extensions))
}

/// [`SCHEMA`], with each server `table` names under `lsp.servers` listed
/// as a property, so its keys get checked like any other table's: a schema
/// map (`additionalProperties`) isn't descended into.
fn schema_for(table: &Table) -> Arc<serde_json::Value> {
    let names: Vec<&String> = match table.get("lsp").and_then(|lsp| lsp.get("servers")) {
        Some(toml::Value::Table(servers)) => servers.keys().collect(),
        _ => Vec::new(),
    };
    if names.is_empty() {
        return SCHEMA.clone();
    }
    let mut schema = serde_json::Value::clone(&SCHEMA);
    let server = serde_json::json!({ "$ref": "#/$defs/LspServerSettings" });
    let properties = names
        .into_iter()
        .map(|name| (name.clone(), server.clone()))
        .collect();
    schema["$defs"]["LspSettings"]["properties"]["servers"]["properties"] =
        serde_json::Value::Object(properties);
    Arc::new(schema)
}

/// The `lsp` settings of the user and project files for a trusted root:
/// each file's `lsp` table checked as [`SettingsStore::load`] checks a
/// file, the project's applied as written, not narrowed, over the user's,
/// over the defaults. Either file may be absent or missing.
///
/// It reads the files, blocking: call it off the main thread. A workbench
/// keeps an [`LspLayer`] for each file instead, so an invalid file keeps
/// its last good table, whatever the host's own reads of the files give.
pub fn trusted_lsp(user: Option<PathBuf>, project: Option<PathBuf>) -> LspSettings {
    let mut settings = LspSettings::default();
    if let Some(user) = user {
        let mut layer = LspLayer::user(user);
        layer.read();
        settings = layer.apply(&settings);
    }
    if let Some(project) = project {
        let mut layer = LspLayer::project(project);
        layer.read();
        settings = layer.apply(&settings);
    }
    settings
}

/// A settings file's `lsp` table, for a trusted root: checked as the
/// settings store checks a file (TOML, serde, then the schema), and kept
/// at its last good value while the file has errors, which is empty until
/// it has read cleanly once. A missing file has an empty table.
///
/// The user's file and the project's are layers of their own, applied over
/// the defaults in turn ([`LspLayer::apply`]), never over the host's
/// effective settings: those have the project's keys narrowed, and may
/// have come from a read of the user's file the host has no last good
/// table for.
#[derive(Clone, Debug, PartialEq)]
pub struct LspLayer {
    path: PathBuf,
    good: Table,
    /// The `theme` isn't the project's to set, so it can't reject its file.
    project: bool,
}

impl LspLayer {
    /// The user's file `path`'s table, not read yet: empty.
    pub fn user(path: PathBuf) -> Self {
        Self {
            path,
            good: Table::new(),
            project: false,
        }
    }

    /// A project's file `path`'s table, not read yet: empty.
    pub fn project(path: PathBuf) -> Self {
        Self {
            project: true,
            ..Self::user(path)
        }
    }

    /// The settings file.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Reads the file again, blocking, keeping the last good table if it
    /// has errors.
    pub fn read(&mut self) {
        let mut layer = kiln_core::load_layer(&self.path);
        if layer.has_errors() {
            return;
        }
        if self.project {
            layer.table.remove("theme");
        }
        if toml::Value::Table(layer.table.clone())
            .try_into::<Settings>()
            .is_err()
        {
            return;
        }
        if layer
            .check_schema(&schema_for(&layer.table))
            .iter()
            .any(Diagnostic::is_error)
        {
            return;
        }
        self.good = match layer.table.remove("lsp") {
            Some(toml::Value::Table(lsp)) => lsp,
            _ => Table::new(),
        };
    }

    /// `base` with this layer's `lsp` table laid over it as written. If the
    /// two don't merge into valid settings, `base`.
    pub fn apply(&self, base: &LspSettings) -> LspSettings {
        if self.good.is_empty() {
            return base.clone();
        }
        let mut table = match Table::try_from(base) {
            Ok(table) => table,
            Err(error) => {
                log::error!("the LSP settings don't serialize to a table: {error}");
                return base.clone();
            }
        };
        crate::layers::merge(&mut table, &self.good);
        let mut settings = Table::new();
        settings.insert("lsp".into(), toml::Value::Table(table.clone()));
        let errors = schema_errors(&settings);
        if !errors.is_empty() {
            log::warn!(
                "the LSP settings of {} don't apply: {errors}",
                self.path.display()
            );
            return base.clone();
        }
        toml::Value::Table(table)
            .try_into()
            .unwrap_or_else(|error| {
                log::warn!(
                    "the LSP settings of {} don't apply: {error}",
                    self.path.display()
                );
                base.clone()
            })
    }
}

/// `table`'s schema errors, joined, or empty.
fn schema_errors(table: &Table) -> String {
    let mut layer = kiln_core::Layer::default();
    layer.table = table.clone();
    layer
        .check_schema(&schema_for(table))
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The warning for each `lsp` key a project file sets that it may not.
pub const UNTRUSTED_LSP: &str = "LSP settings in project files apply only in trusted folders";

/// Drops every `lsp` key of a project layer's table except
/// `lsp.enabled = false` and `lsp.servers.<name>.enabled = false`, with
/// an [`UNTRUSTED_LSP`] warning at each, whatever its value.
fn narrow_lsp(layer: &mut Layer) {
    let off = toml::Value::Boolean(false);
    let mut kept = Table::new();
    let mut dropped: Vec<Vec<String>> = Vec::new();
    let lsp = match layer.table.remove("lsp") {
        None => return,
        Some(toml::Value::Table(lsp)) => lsp,
        Some(_) => {
            dropped.push(Vec::new());
            Table::new()
        }
    };
    for (key, value) in lsp {
        match (key.as_str(), value) {
            ("enabled", value) if value == off => {
                kept.insert(key, value);
            }
            ("servers", toml::Value::Table(servers)) => {
                let mut kept_servers = Table::new();
                for (name, server) in servers {
                    let toml::Value::Table(server) = server else {
                        dropped.push(vec![key.clone(), name]);
                        continue;
                    };
                    let mut kept_server = Table::new();
                    for (field, value) in server {
                        if field == "enabled" && value == off {
                            kept_server.insert(field, value);
                        } else {
                            dropped.push(vec![key.clone(), name.clone(), field]);
                        }
                    }
                    if !kept_server.is_empty() {
                        kept_servers.insert(name, toml::Value::Table(kept_server));
                    }
                }
                if !kept_servers.is_empty() {
                    kept.insert(key, toml::Value::Table(kept_servers));
                }
            }
            _ => dropped.push(vec![key]),
        }
    }
    if !kept.is_empty() {
        layer.table.insert("lsp".into(), toml::Value::Table(kept));
    }
    let document = DeTable::parse(&layer.source).ok();
    for path in dropped {
        let offset = document
            .as_ref()
            .map_or(0, |document| key_offset(document.get_ref(), "lsp", &path));
        let key = std::iter::once("lsp")
            .chain(path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".");
        let warning =
            layer.diagnostic_at(Severity::Warning, Some(key), offset, UNTRUSTED_LSP.into());
        layer.diagnostics.push(warning);
    }
}

/// The warning for each `theme` key a project file sets.
pub const PROJECT_THEME: &str =
    "theme settings in project files are ignored: the theme is the same in every window";

/// Drops a project layer's `theme` table, with a [`PROJECT_THEME`] warning
/// at each key it set (at `theme` itself if it isn't a table).
fn drop_theme(layer: &mut Layer) {
    drop_table(layer, "theme", PROJECT_THEME);
}

/// Drops a project layer's `name` table, with a `message` warning at each
/// key it set (at `name` itself if it isn't a table).
fn drop_table(layer: &mut Layer, name: &str, message: &str) {
    let dropped: Vec<Vec<String>> = match layer.table.remove(name) {
        None => return,
        Some(toml::Value::Table(table)) => table.into_iter().map(|(key, _)| vec![key]).collect(),
        Some(_) => vec![Vec::new()],
    };
    let document = DeTable::parse(&layer.source).ok();
    for path in dropped {
        let offset = document
            .as_ref()
            .map_or(0, |document| key_offset(document.get_ref(), name, &path));
        let key = std::iter::once(name)
            .chain(path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".");
        let warning = layer.diagnostic_at(Severity::Warning, Some(key), offset, message.into());
        layer.diagnostics.push(warning);
    }
}

/// The warning for each `extensions` key a project file sets.
pub const PROJECT_EXTENSIONS: &str =
    "extensions settings in project files are ignored: they're your own, in every window";

/// Drops a project layer's `extensions` table, with a [`PROJECT_EXTENSIONS`]
/// warning at each key it set (at `extensions` itself if it isn't a table).
fn drop_extensions(layer: &mut Layer) {
    drop_table(layer, "extensions", PROJECT_EXTENSIONS);
}

/// The warning for a project file's `[chat]` table.
pub const PROJECT_CHAT: &str =
    "A project's settings can't set `chat`; set it in your own settings.toml";

/// Drops a project layer's `chat` table, with one [`PROJECT_CHAT`] warning at
/// the table.
fn drop_chat(layer: &mut Layer) {
    if layer.table.remove("chat").is_none() {
        return;
    }
    let offset = DeTable::parse(&layer.source)
        .ok()
        .map_or(0, |document| key_offset(document.get_ref(), "chat", &[]));
    let warning = layer.diagnostic_at(
        Severity::Warning,
        Some("chat".into()),
        offset,
        PROJECT_CHAT.into(),
    );
    layer.diagnostics.push(warning);
}

/// The warning for each `terminal.shell` and `terminal.shell_args` a project
/// file sets.
pub const PROJECT_TERMINAL_SHELL: &str = "terminal.shell and terminal.shell_args in project files are ignored: a folder can't choose the program its terminal runs";

/// Drops a project layer's `terminal.shell` and `terminal.shell_args`, with a
/// [`PROJECT_TERMINAL_SHELL`] warning at each; its other `terminal` keys stay.
fn drop_terminal_shell(layer: &mut Layer) {
    let Some(toml::Value::Table(terminal)) = layer.table.get_mut("terminal") else {
        return;
    };
    let dropped: Vec<&str> = ["shell", "shell_args"]
        .into_iter()
        .filter(|key| terminal.remove(*key).is_some())
        .collect();
    let document = DeTable::parse(&layer.source).ok();
    for key in dropped {
        let offset = document.as_ref().map_or(0, |document| {
            key_offset(document.get_ref(), "terminal", &[key.to_owned()])
        });
        let warning = layer.diagnostic_at(
            Severity::Warning,
            Some(format!("terminal.{key}")),
            offset,
            PROJECT_TERMINAL_SHELL.into(),
        );
        layer.diagnostics.push(warning);
    }
}

/// The warning for each `vim.shell` and `vim.remap` a project file sets.
pub const PROJECT_VIM_USER_ONLY: &str = "vim.shell and vim.remap in project files are ignored: a folder can't choose the program ! runs or type keys as you";

/// Drops a project layer's `vim.shell` and `vim.remap`, with a
/// [`PROJECT_VIM_USER_ONLY`] warning at each; its other `vim` keys stay.
fn drop_vim_user_only(layer: &mut Layer) {
    let Some(toml::Value::Table(vim)) = layer.table.get_mut("vim") else {
        return;
    };
    let dropped: Vec<&str> = ["shell", "remap"]
        .into_iter()
        .filter(|key| vim.remove(*key).is_some())
        .collect();
    let document = DeTable::parse(&layer.source).ok();
    for key in dropped {
        let offset = document.as_ref().map_or(0, |document| {
            key_offset(document.get_ref(), "vim", &[key.to_owned()])
        });
        let warning = layer.diagnostic_at(
            Severity::Warning,
            Some(format!("vim.{key}")),
            offset,
            PROJECT_VIM_USER_ONLY.into(),
        );
        layer.diagnostics.push(warning);
    }
}

/// Where the key at `first` then `rest` is in the source, or where the
/// deepest of its tables found is.
fn key_offset(table: &DeTable<'_>, first: &str, rest: &[String]) -> usize {
    let mut table = table;
    let mut offset = 0;
    for key in std::iter::once(first).chain(rest.iter().map(String::as_str)) {
        let Some((found, value)) = table.iter().find(|(name, _)| name.get_ref() == key) else {
            break;
        };
        offset = found.span().start;
        match value.get_ref() {
            DeValue::Table(inner) => table = inner,
            _ => break,
        }
    }
    offset
}
