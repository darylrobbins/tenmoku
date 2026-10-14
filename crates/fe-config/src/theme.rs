//! The editor's colours in a theme. kiln owns theme files, the dark and
//! light pair, the mode and the user's theme file
//! (docs/decisions/app-framework.md §8); a fastedit theme file adds two app
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

/// The app tables a fastedit theme file has, beside kiln's own keys: for
/// [`ThemeFile::load`] and [`ThemeFile::parse`], and kiln's user theme.
pub const EXTENSIONS: &[&str] = &["editor", "syntax"];

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
        /// The gutter bar of rows added since the git base.
        change_added,
        /// The gutter bar of rows modified since the git base.
        change_modified,
        /// The mark where rows were deleted since the git base.
        change_deleted,
        /// A vertical line at an indent level.
        indent_guide,
        /// The indent guide of the block the caret is in.
        indent_guide_active,
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
}

static FASTEDIT_DARK: LazyLock<EditorPalette> = LazyLock::new(|| {
    bundled(
        "fastedit-dark.toml",
        include_str!("../themes/fastedit-dark.toml"),
    )
});

static FASTEDIT_LIGHT: LazyLock<EditorPalette> = LazyLock::new(|| {
    bundled(
        "fastedit-light.toml",
        include_str!("../themes/fastedit-light.toml"),
    )
});

/// A bundled palette, from its file's tables alone.
fn bundled(file_name: &str, source: &str) -> EditorPalette {
    let (file, diagnostics) = ThemeFile::parse(Path::new(file_name), source, EXTENSIONS);
    debug_assert_eq!(diagnostics, [], "the bundled palette has problems");
    let file = file.unwrap_or_default();
    let mut palette = EditorPalette::default();
    let warnings = palette.overlay(file.extensions.get("editor"), file.extensions.get("syntax"));
    debug_assert_eq!(
        warnings,
        [] as [String; 0],
        "the bundled palette has problems"
    );
    palette
}

impl EditorPalette {
    /// The bundled dark palette, fastedit's colours since before themes.
    pub fn fastedit_dark() -> &'static EditorPalette {
        &FASTEDIT_DARK
    }

    /// The bundled light palette, on Kiln Light's background.
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
        let warnings = palette.overlay(theme.extension("editor"), theme.extension("syntax"));
        (palette, warnings)
    }

    /// The colour listed for the capture named exactly `name`.
    pub fn syntax_color(&self, name: &str) -> Option<Color> {
        self.syntax
            .iter()
            .find(|(capture, _)| capture == name)
            .map(|&(_, color)| color)
    }

    /// Lays `editor` and `syntax` tables over the palette, returning the
    /// warnings.
    fn overlay(&mut self, editor: Option<&Table>, syntax: Option<&Table>) -> Vec<String> {
        let mut warnings = Vec::new();
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
