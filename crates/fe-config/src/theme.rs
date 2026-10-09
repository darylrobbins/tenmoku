//! Colour themes: the file format, the bundled fastedit Dark, and the
//! user's themes directory. Turning colours into GPUI's lives in the UI
//! crates, so this stays GPUI-free.
//!
//! A theme file has a top-level `name` and `appearance`, then `[ui]`,
//! `[editor]` and `[syntax]` tables of colour strings. Any of it may be left
//! out, and whatever is takes fastedit Dark's value. Unlike a settings file,
//! a theme with an error still loads: an error is about one key, which
//! falls back as if missing. Only a file that can't be read or parsed is
//! left out.

use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
    sync::LazyLock,
};

use toml::{Table, Value};

use crate::{
    diagnostic::{Diagnostic, Severity},
    layers::{Layer, load_layer, parse_layer},
};

/// The bundled fastedit Dark theme's source.
pub const FASTEDIT_DARK_SOURCE: &str = include_str!("../themes/fastedit-dark.toml");

/// An sRGB colour with alpha, written `"#rgb"`, `"#rrggbb"` or
/// `"#rrggbbaa"`. Leaving alpha out makes it opaque.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// `0xrrggbbaa`, as GPUI's `rgba` takes it.
    pub fn to_rgba_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let invalid =
            || format!("{text:?} isn't a colour: expected \"#rgb\", \"#rrggbb\" or \"#rrggbbaa\"");
        let digits = text.strip_prefix('#').ok_or_else(invalid)?;
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let nibble = |ix: usize| u8::from_str_radix(&digits[ix..ix + 1], 16).unwrap();
        let byte = |ix: usize| u8::from_str_radix(&digits[ix..ix + 2], 16).unwrap();
        match digits.len() {
            3 => Ok(Self {
                r: nibble(0) * 0x11,
                g: nibble(1) * 0x11,
                b: nibble(2) * 0x11,
                a: 0xff,
            }),
            6 | 8 => Ok(Self {
                r: byte(0),
                g: byte(2),
                b: byte(4),
                a: if digits.len() == 8 { byte(6) } else { 0xff },
            }),
            _ => Err(invalid()),
        }
    }
}

/// `#rrggbb`, or `#rrggbbaa` unless opaque.
impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;
        if self.a != 0xff {
            write!(f, "{:02x}", self.a)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

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
    /// The workbench chrome's colours, as fe-ui's `UiTheme` has them.
    UiColors {
        /// Behind the editor and the empty pane.
        background,
        panel_background,
        border,
        text,
        /// Ignored entries and placeholders.
        muted_text,
        /// Entries that couldn't be read.
        error_text,
        row_hover,
        row_selected,
        /// The selected row while its list doesn't have focus.
        row_selected_inactive,
        /// The outline of the selected row while its list has focus.
        row_focus_border,
        /// A file's icon in a tree row.
        icon,
        /// A folder's icon in a tree row.
        folder_icon,
        /// A tree row's disclosure chevron.
        chevron,
        /// The vertical lines marking a tree row's ancestor levels.
        indent_guide,
        tab_active,
        tab_active_text,
        tab_inactive,
        /// The dot on a tab with unsaved changes.
        dirty_dot,
        /// Where a dragged tab would land.
        drop_target,
        /// The bar beside the activity rail's highlighted button.
        accent,
    }
}

colors! {
    /// The text editor's colours, apart from syntax highlighting.
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
    }
}

/// A colour theme.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub appearance: Appearance,
    pub ui: UiColors,
    pub editor: EditorColors,
    /// Colours for highlight captures by dotted name, such as `keyword` or
    /// `function.macro`. A capture takes the colour of its longest listed
    /// prefix. Fallback entries come first, then the theme's own new ones in
    /// file order; no name appears twice.
    pub syntax: Vec<(String, Color)>,
}

static FASTEDIT_DARK: LazyLock<Theme> = LazyLock::new(|| {
    let layer = parse_layer(Path::new("fastedit-dark.toml"), FASTEDIT_DARK_SOURCE.into());
    let mut diagnostics = layer.diagnostics.clone();
    let theme = build(&layer, Theme::default(), &mut diagnostics);
    debug_assert_eq!(diagnostics, [], "the bundled theme has problems");
    theme
});

impl Theme {
    /// The bundled dark theme, which fills in whatever another theme leaves
    /// out.
    pub fn fastedit_dark() -> &'static Theme {
        &FASTEDIT_DARK
    }

    /// The colour listed for the capture named exactly `name`.
    pub fn syntax_color(&self, name: &str) -> Option<Color> {
        self.syntax
            .iter()
            .find(|(capture, _)| capture == name)
            .map(|&(_, color)| color)
    }

    /// Reads the theme file at `path`. It's `None`, with an error, if the
    /// file is missing, unreadable or not TOML. Otherwise any key it leaves
    /// out or gets wrong takes fastedit Dark's value, and a missing `name`
    /// is the file's stem.
    pub fn load(path: &Path) -> (Option<Theme>, Vec<Diagnostic>) {
        let (theme, _, diagnostics) = load(path);
        (theme, diagnostics)
    }

    /// Parses `source` as the theme file at `path`, as [`Theme::load`] does.
    pub fn parse(path: &Path, source: &str) -> (Option<Theme>, Vec<Diagnostic>) {
        from_layer(&parse_layer(path, source.into()))
    }
}

/// [`Theme::load`], with the file's layer.
fn load(path: &Path) -> (Option<Theme>, Layer, Vec<Diagnostic>) {
    let layer = load_layer(path);
    // A missing file loads as an empty layer, like an empty file.
    if layer.source.is_empty() && layer.diagnostics.is_empty() && !path.exists() {
        let error = layer.diagnostic_at(Severity::Error, None, 0, "no such file".into());
        return (None, layer, vec![error]);
    }
    let (theme, diagnostics) = from_layer(&layer);
    (theme, layer, diagnostics)
}

fn from_layer(layer: &Layer) -> (Option<Theme>, Vec<Diagnostic>) {
    let mut diagnostics = layer.diagnostics.clone();
    if layer.has_errors() {
        return (None, diagnostics);
    }
    let mut base = Theme::fastedit_dark().clone();
    base.name = layer
        .path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let theme = build(layer, base, &mut diagnostics);
    // In file order, not the table's sorted-key order.
    diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
    (Some(theme), diagnostics)
}

/// `base` with the layer's values over it.
fn build(layer: &Layer, mut theme: Theme, diagnostics: &mut Vec<Diagnostic>) -> Theme {
    let mut path = Vec::new();
    let mut syntax = Vec::new();
    for (key, value) in &layer.table {
        path.push(key.clone());
        match key.as_str() {
            "name" => match value {
                Value::String(name) if !name.trim().is_empty() => theme.name = name.clone(),
                Value::String(_) => diagnostics.push(error(layer, &path, "must not be empty")),
                _ => diagnostics.push(wrong_type(layer, &path, "a string", value)),
            },
            "appearance" => match value.as_str() {
                Some("dark") => theme.appearance = Appearance::Dark,
                Some("light") => theme.appearance = Appearance::Light,
                _ => diagnostics.push(error(layer, &path, "expected \"dark\" or \"light\"")),
            },
            "ui" => colors(
                layer,
                value,
                &mut path,
                diagnostics,
                UiColors::KEYS,
                |key, color| *theme.ui.get_mut(key).unwrap() = color,
            ),
            "editor" => colors(
                layer,
                value,
                &mut path,
                diagnostics,
                EditorColors::KEYS,
                |key, color| *theme.editor.get_mut(key).unwrap() = color,
            ),
            "syntax" => match value {
                Value::Table(table) => {
                    syntax_colors(layer, table, &mut path, &mut syntax, diagnostics)
                }
                _ => diagnostics.push(wrong_type(layer, &path, "a table", value)),
            },
            _ => diagnostics.push(unknown(layer, &path)),
        }
        path.pop();
    }
    // The table is sorted by key; file order is by where the key is.
    syntax.sort_by_key(|(offset, _, _)| *offset);
    // Captures the file doesn't mention stay as fallbacks; the file's own
    // entries follow in file order, replacements of bundled captures included.
    theme
        .syntax
        .retain(|(capture, _)| !syntax.iter().any(|(_, name, _)| name == capture));
    for (_, name, color) in syntax {
        match theme
            .syntax
            .iter_mut()
            .find(|(capture, _)| *capture == name)
        {
            Some((_, slot)) => *slot = color,
            None => theme.syntax.push((name, color)),
        }
    }
    theme
}

/// Sets the colours of a `[ui]` or `[editor]` table, which has `keys`,
/// through `set`.
fn colors(
    layer: &Layer,
    value: &Value,
    path: &mut Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
    keys: &[&str],
    mut set: impl FnMut(&str, Color),
) {
    let Value::Table(table) = value else {
        diagnostics.push(wrong_type(layer, path, "a table", value));
        return;
    };
    for (key, value) in table {
        path.push(key.clone());
        if !keys.contains(&key.as_str()) {
            diagnostics.push(unknown(layer, path));
        } else {
            match color(layer, path, value) {
                Ok(color) => set(key, color),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
        path.pop();
    }
}

/// Collects the `[syntax]` table's colours as (offset, capture name,
/// colour). A nested table, as an unquoted `function.macro = …` makes, is a
/// group of dotted names.
fn syntax_colors(
    layer: &Layer,
    table: &Table,
    path: &mut Vec<String>,
    syntax: &mut Vec<(usize, String, Color)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (key, value) in table {
        path.push(key.clone());
        let name = path[1..].join(".");
        if !is_capture_name(&name) {
            let message =
                "not a capture name: expected dot-separated words of a-z, 0-9 and _, ignored";
            diagnostics.push(layer.key_diagnostic(Severity::Warning, path, message.into()));
        } else if let Value::Table(table) = value {
            syntax_colors(layer, table, path, syntax, diagnostics);
        } else {
            match color(layer, path, value) {
                Ok(color) => syntax.push((layer.key_offset(path).unwrap_or(0), name, color)),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
        path.pop();
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

fn color(layer: &Layer, path: &[String], value: &Value) -> Result<Color, Diagnostic> {
    match value {
        Value::String(text) => text
            .parse()
            .map_err(|message| layer.value_diagnostic(Severity::Error, path, message)),
        _ => Err(wrong_type(layer, path, "a colour string", value)),
    }
}

fn error(layer: &Layer, path: &[String], message: &str) -> Diagnostic {
    layer.value_diagnostic(Severity::Error, path, message.into())
}

fn wrong_type(layer: &Layer, path: &[String], expected: &str, value: &Value) -> Diagnostic {
    let message = format!("expected {expected}, found {}", value.type_str());
    layer.value_diagnostic(Severity::Error, path, message)
}

fn unknown(layer: &Layer, path: &[String]) -> Diagnostic {
    layer.key_diagnostic(Severity::Warning, path, "unknown key, ignored".into())
}

/// The themes to choose from: fastedit Dark and the user's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeSet {
    themes: Vec<Theme>,
}

/// Just the bundled theme.
impl Default for ThemeSet {
    fn default() -> Self {
        Self {
            themes: vec![Theme::fastedit_dark().clone()],
        }
    }
}

impl ThemeSet {
    /// The theme called `name`, ignoring case.
    pub fn get(&self, name: &str) -> Option<&Theme> {
        let name = name.to_lowercase();
        self.themes
            .iter()
            .find(|theme| theme.name.to_lowercase() == name)
    }

    /// Every theme's name: the bundled ones, then the user's by file name.
    /// A user theme that replaces a bundled one takes its place.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.themes.iter().map(|theme| theme.name.as_str())
    }

    /// Adds `theme`, replacing any theme of the same name, ignoring case,
    /// and returning it.
    fn insert(&mut self, theme: Theme) -> Option<Theme> {
        let name = theme.name.to_lowercase();
        match self
            .themes
            .iter_mut()
            .find(|old| old.name.to_lowercase() == name)
        {
            Some(old) => Some(std::mem::replace(old, theme)),
            None => {
                self.themes.push(theme);
                None
            }
        }
    }
}

/// The bundled themes and every `*.toml` theme in `dir` (see
/// [`crate::paths::themes_dir`]), read in file-name order. A user theme
/// replaces a bundled one of the same name, ignoring case; if two of the
/// user's share a name, the later file's wins, with a warning. A missing
/// `dir` is no user themes.
pub fn load_themes(dir: Option<&Path>) -> (ThemeSet, Vec<Diagnostic>) {
    let mut set = ThemeSet::default();
    let mut diagnostics = Vec::new();
    let Some(dir) = dir else {
        return (set, diagnostics);
    };
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (set, diagnostics);
        }
        Err(error) => {
            diagnostics.push(dir_error(
                dir,
                format!("cannot read the directory: {error}"),
            ));
            return (set, diagnostics);
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml") && path.is_file())
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    // Which file each user theme came from, by lowercased name.
    let mut sources: Vec<(String, PathBuf)> = Vec::new();
    for file in files {
        let (theme, layer, more) = load(&file);
        diagnostics.extend(more);
        let Some(theme) = theme else { continue };
        let name = theme.name.to_lowercase();
        if let Some((_, earlier)) = sources.iter_mut().find(|(seen, _)| *seen == name) {
            let message = format!(
                "theme {:?} is also in {}, which this file overrides",
                theme.name,
                earlier.display()
            );
            let name_path = ["name".to_owned()];
            diagnostics.push(layer.value_diagnostic(Severity::Warning, &name_path, message));
            *earlier = file;
        } else {
            sources.push((name, file));
        }
        set.insert(theme);
    }
    (set, diagnostics)
}

/// An error about the directory at `path`.
fn dir_error(path: &Path, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        file: path.to_owned(),
        key: None,
        line: 1,
        column: 1,
        message,
    }
}
