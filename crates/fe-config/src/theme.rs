//! The editor's colours in a theme. kiln owns theme files, the dark and
//! light pair, the mode and the user's theme file
//! (docs/decisions/app-framework.md §8); a Tenmoku theme file adds two app
//! tables to kiln's format ([`EXTENSIONS`]), which kiln keeps as
//! [`ThemeData::extensions`] for this module to read:
//!
//! ```toml
//! appearance = "light"
//!
//! [editor]
//! background = "#fafafa"
//!
//! [syntax]
//! keyword = "#d00000"
//! "function.method" = "#123456"
//! ```
//!
//! Whatever a theme leaves out takes the bundled palette's colour for its
//! appearance ([`EditorPalette::fastedit_dark`],
//! [`EditorPalette::fastedit_light`]). Turning colours into GPUI's lives in
//! the UI crates, so this stays GPUI-free.

use std::{path::Path, sync::LazyLock};

use kiln_core::theme::{Appearance, ThemeData, ThemeFile};
use toml::{Table, Value};

pub use kiln_core::theme::Color;

/// The app tables a Tenmoku theme file has, beside kiln's own keys: for
/// [`ThemeFile::load`] and [`ThemeFile::parse`], and kiln's user theme.
pub const EXTENSIONS: &[&str] = &["editor", "syntax", "terminal"];

/// A table of named colours: the struct, and its keys for parsing.
macro_rules! colors {
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        pub struct $name {
            $($(#[$field_meta])* pub $field: Color,)*
        }

        impl $name {
            /// Every key, in declaration order.
            pub const KEYS: &[&str] = &[$(stringify!($field)),*];

            /// The colour at `key`, if there's such a key.
            pub fn get(&self, key: &str) -> Option<Color> {
                match key {
                    $(stringify!($field) => Some(self.$field),)*
                    _ => None,
                }
            }

            fn get_mut(&mut self, key: &str) -> Option<&mut Color> {
                match key {
                    $(stringify!($field) => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

colors! {
    /// The text editor's colours, apart from syntax highlighting: a theme's
    /// `[editor]` table.
    EditorColors {
        background,
        foreground,
        gutter_foreground,
        /// The gutter's number on the caret's line.
        gutter_active_foreground,
        /// Behind the caret's line.
        active_line,
        selection,
        cursor,
        /// Error diagnostics.
        error,
        /// Warning diagnostics.
        warning,
        /// Information diagnostics.
        information,
        /// Hint diagnostics.
        hint,
        /// Behind hover and completion popovers.
        popover_background,
        popover_border,
        /// A vim jump's live label: the box of a label that picks its target.
        jump_label,
        /// A vim jump's preview label, the one `{c2}` would give its match,
        /// and a concealed label's box, one on another page.
        jump_label_preview,
        /// The glyph on a jump label's box.
        jump_label_foreground,
        /// The flash over text a vim yank took.
        vim_yank_highlight,
        /// The underline under a char `f{c}` or `F{c}` reaches (quick-scope's
        /// primary).
        quick_scope_primary,
        /// The underline under a word's char only `2f{c}` or `2F{c}` reaches
        /// (quick-scope's secondary).
        quick_scope_secondary,
        /// The gutter bar of rows added since the git base.
        change_added,
        /// The gutter bar of rows modified since the git base.
        change_modified,
        /// The mark where rows were deleted since the git base.
        change_deleted,
        /// Behind a diff view's added rows, and the right side's rows of a
        /// modified pair, VS Code's diffEditor.insertedLineBackground.
        diff_added_row,
        /// Behind a diff view's removed rows, and the left side's rows of a
        /// modified pair, VS Code's diffEditor.removedLineBackground.
        diff_removed_row,
        /// Behind the changed words of a diff view's added rows, over
        /// `diff_added_row`, VS Code's diffEditor.insertedTextBackground.
        diff_added_word,
        /// Behind the changed words of a diff view's removed rows, over
        /// `diff_removed_row`, VS Code's diffEditor.removedTextBackground.
        diff_removed_word,
        /// The diagonal hatch on a diff view's filler rows, across from the
        /// other side's added or removed rows, VS Code's
        /// diffEditor.diagonalFill.
        diff_filler,
        /// A vertical line at an indent level.
        indent_guide,
        /// The indent guide of the block the caret is in.
        indent_guide_active,
        /// Behind sticky scroll's band of pinned scope headers.
        sticky_scroll_background,
        /// The line under the band, when it's visible.
        sticky_scroll_border,
        /// The shadow fading down from the band.
        sticky_scroll_shadow,
        /// Behind a band row under the pointer.
        sticky_scroll_hover,
        /// The marks `editor.render_whitespace` draws on spaces and tabs, VS
        /// Code's editorWhitespace.foreground.
        whitespace,
        /// The lines `editor.rulers` draws, VS Code's editorRuler.foreground.
        ruler,
        /// The vim mode's segment in the status bar in normal mode, operator
        /// pending or while a jump is pending.
        vim_mode_normal,
        /// The vim mode's segment in insert mode.
        vim_mode_insert,
        /// The vim mode's segment in the visual, visual line and visual block
        /// modes.
        vim_mode_visual,
        /// The vim mode's segment in replace mode.
        vim_mode_replace,
        /// The segment on vim's command line, for `:`, `/` and `?`.
        vim_mode_command,
        /// The label on every vim mode's segment.
        vim_mode_foreground,
        /// The brackets of an outermost pair, with `editor.bracket_pair_colors`.
        bracket_1,
        /// The brackets of a pair inside a `bracket_1` pair.
        bracket_2,
        /// The brackets of a pair inside a `bracket_2` pair. A pair inside
        /// this one takes `bracket_1` again.
        bracket_3,
        /// Behind a find or search match.
        find_match,
        /// Behind the other matches when one is the current match.
        find_match_other,
        /// Behind the symbol under the caret and its other uses.
        identifier_highlight,
        /// The gutter's lane beside rows an agent wrote.
        agent_lane,
        /// Behind the fold chevron under the pointer.
        fold_hover,
        /// Behind the minimap.
        minimap_background,
        /// The minimap's slider over the visible rows.
        minimap_slider,
        /// The minimap's slider under the pointer.
        minimap_slider_hover,
        /// The minimap's slider while it's dragged.
        minimap_slider_active,
        /// The selection's mark in the minimap.
        minimap_selection,
        /// A find match's mark in the minimap.
        minimap_find_match,
        /// An error diagnostic's mark in the minimap.
        minimap_error,
        /// A warning diagnostic's mark in the minimap.
        minimap_warning,
        /// The minimap's mark by rows added since the git base.
        minimap_gutter_added,
        /// The minimap's mark by rows modified since the git base.
        minimap_gutter_modified,
        /// The minimap's mark where rows were deleted since the git base.
        minimap_gutter_deleted,
    }
}

/// The editor's colours: a theme's `[editor]` and `[syntax]` tables laid
/// over the bundled palette for its appearance.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorPalette {
    pub editor: EditorColors,
    /// Colours for highlight captures by dotted name, such as `keyword` or
    /// `function.macro`. A capture takes the colour of its longest listed
    /// prefix. The base's entries come first, then the theme's new ones in
    /// table order; no name appears twice.
    pub syntax: Vec<(String, Color)>,
    /// The terminal's colours: a theme's `[terminal]` table.
    pub terminal: TerminalColors,
}

/// The terminal's colours: a theme's `[terminal]` table, whose keys are
/// `ansi_0` … `ansi_15`, `foreground`, `background` and `cursor`. A theme
/// that leaves the ANSI colours out gets the bundled palette's (xterm's hues
/// for its appearance); one that leaves out the other three draws them in the
/// editor's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalColors {
    /// ANSI colours 0–15.
    pub ansi: [Color; 16],
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub cursor: Option<Color>,
}

impl TerminalColors {
    /// Every key, in order.
    pub const KEYS: &[&str] = &[
        "ansi_0",
        "ansi_1",
        "ansi_2",
        "ansi_3",
        "ansi_4",
        "ansi_5",
        "ansi_6",
        "ansi_7",
        "ansi_8",
        "ansi_9",
        "ansi_10",
        "ansi_11",
        "ansi_12",
        "ansi_13",
        "ansi_14",
        "ansi_15",
        "foreground",
        "background",
        "cursor",
    ];

    fn set(&mut self, key: &str, color: Color) -> bool {
        if let Some(index) = key.strip_prefix("ansi_") {
            return match index.parse::<usize>() {
                Ok(number) if number < 16 && number.to_string() == index => {
                    self.ansi[number] = color;
                    true
                }
                _ => false,
            };
        }
        match key {
            "foreground" => self.foreground = Some(color),
            "background" => self.background = Some(color),
            "cursor" => self.cursor = Some(color),
            _ => return false,
        }
        true
    }
}

static FASTEDIT_DARK: LazyLock<EditorPalette> =
    LazyLock::new(|| bundled("yohen.toml", include_str!("../themes/yohen.toml")));

static FASTEDIT_LIGHT: LazyLock<EditorPalette> =
    LazyLock::new(|| bundled("hakuji.toml", include_str!("../themes/hakuji.toml")));

/// A bundled palette, from its file's tables alone.
fn bundled(file_name: &str, source: &str) -> EditorPalette {
    let (file, diagnostics) = ThemeFile::parse(Path::new(file_name), source, EXTENSIONS);
    debug_assert_eq!(diagnostics, [], "the bundled palette has problems");
    let file = file.unwrap_or_default();
    let mut palette = EditorPalette::default();
    let warnings = palette.overlay(
        file.extensions.get("editor"),
        file.extensions.get("syntax"),
        file.extensions.get("terminal"),
    );
    debug_assert_eq!(
        warnings,
        [] as [String; 0],
        "the bundled palette has problems"
    );
    palette
}

impl EditorPalette {
    /// The bundled dark palette: Yohen's, Tenmoku's default dark theme.
    pub fn fastedit_dark() -> &'static EditorPalette {
        &FASTEDIT_DARK
    }

    /// The bundled light palette: Hakuji's, Tenmoku's default light theme.
    pub fn fastedit_light() -> &'static EditorPalette {
        &FASTEDIT_LIGHT
    }

    /// The bundled palette for `appearance`.
    pub fn bundled(appearance: Appearance) -> &'static EditorPalette {
        match appearance {
            Appearance::Dark => Self::fastedit_dark(),
            Appearance::Light => Self::fastedit_light(),
        }
    }

    /// The bundled palette for `theme`'s appearance, with its `editor` and
    /// `syntax` tables laid over it. A key that isn't a colour key or a
    /// capture name, or a value that isn't a colour, is left out with a
    /// warning naming its dotted key. The tables carry no line numbers, so
    /// the warnings don't either.
    pub fn for_theme(theme: &ThemeData) -> (EditorPalette, Vec<String>) {
        let mut palette = Self::bundled(theme.appearance).clone();
        let warnings = palette.overlay(
            theme.extension("editor"),
            theme.extension("syntax"),
            theme.extension("terminal"),
        );
        (palette, warnings)
    }

    /// The colour listed for the capture named exactly `name`.
    pub fn syntax_color(&self, name: &str) -> Option<Color> {
        self.syntax
            .iter()
            .find(|(capture, _)| capture == name)
            .map(|&(_, color)| color)
    }

    /// Lays `editor`, `syntax` and `terminal` tables over the palette,
    /// returning the warnings.
    fn overlay(
        &mut self,
        editor: Option<&Table>,
        syntax: Option<&Table>,
        terminal: Option<&Table>,
    ) -> Vec<String> {
        let mut warnings = Vec::new();
        for (key, value) in terminal.into_iter().flatten() {
            let dotted = format!("terminal.{key}");
            match color(value) {
                Err(message) => warnings.push(format!("{dotted}: {message}")),
                Ok(color) => {
                    if !self.terminal.set(key, color) {
                        warnings.push(format!("{dotted}: unknown key"));
                    }
                }
            }
        }
        for (key, value) in editor.into_iter().flatten() {
            let dotted = format!("editor.{key}");
            match self.editor.get_mut(key) {
                None => warnings.push(format!("{dotted}: unknown key")),
                Some(slot) => match color(value) {
                    Ok(color) => *slot = color,
                    Err(message) => warnings.push(format!("{dotted}: {message}")),
                },
            }
        }
        if let Some(syntax) = syntax {
            let mut path = Vec::new();
            self.syntax_colors(syntax, &mut path, &mut warnings);
        }
        warnings
    }

    /// Sets the `[syntax]` table's colours. A nested table, as an unquoted
    /// `function.macro = …` makes, is a group of dotted names.
    fn syntax_colors(&mut self, table: &Table, path: &mut Vec<String>, warnings: &mut Vec<String>) {
        for (key, value) in table {
            path.push(key.clone());
            let name = path.join(".");
            if !is_capture_name(&name) {
                warnings.push(format!(
                    "syntax.{name}: not a capture name: expected dot-separated words of a-z, \
                     0-9 and _"
                ));
            } else if let Value::Table(table) = value {
                self.syntax_colors(table, path, warnings);
            } else {
                match color(value) {
                    Ok(color) => match self.syntax.iter_mut().find(|(capture, _)| *capture == name)
                    {
                        Some((_, slot)) => *slot = color,
                        None => self.syntax.push((name, color)),
                    },
                    Err(message) => warnings.push(format!("syntax.{name}: {message}")),
                }
            }
            path.pop();
        }
    }
}

/// Dot-separated non-empty segments of `[a-z0-9_]`.
fn is_capture_name(name: &str) -> bool {
    name.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
    })
}

fn color(value: &Value) -> Result<Color, String> {
    match value {
        Value::String(text) => text.parse(),
        _ => Err(format!(
            "expected a colour string, found {}",
            value.type_str()
        )),
    }
}
