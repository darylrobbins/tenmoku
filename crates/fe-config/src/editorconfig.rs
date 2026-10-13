//! A file's EditorConfig properties: indent style and size, tab width, line
//! ending, final newline and trailing-whitespace trimming. ec4rs does the
//! file parsing and the glob matching; this walks the directories (every
//! `.editorconfig` from the file's directory up, stopping at `root = true`)
//! and reads the values we use.

use std::path::{Component, Path, PathBuf};

use ec4rs::{ConfigParser, Properties, PropertiesSource, Section};

/// `indent_style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentKind {
    Tab,
    Space,
}

/// `indent_size`: a column count, or `tab` when no tab width is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentSize {
    Columns(usize),
    Tab,
}

/// `end_of_line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndOfLine {
    Lf,
    Crlf,
    Cr,
}

/// The EditorConfig properties that apply to one file; `None` for unset,
/// missing or invalid values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorConfig {
    pub indent_style: Option<IndentKind>,
    pub indent_size: Option<IndentSize>,
    pub tab_width: Option<usize>,
    pub end_of_line: Option<EndOfLine>,
    pub insert_final_newline: Option<bool>,
    pub trim_trailing_whitespace: Option<bool>,
}

/// One parsed `.editorconfig`, with the directory its globs are relative to.
struct Parsed {
    dir: PathBuf,
    sections: Vec<Section>,
}

/// The properties `.editorconfig` files set for `path`, with EditorConfig's
/// defaults applied: `indent_size = tab` takes the tab width, `indent_style =
/// tab` without an `indent_size` means `indent_size = tab`, and an
/// `indent_size` without a `tab_width` sets `tab_width` to it. A
/// `.editorconfig` that can't be read or parsed is skipped whole (its `root`
/// marker too) with a warning, and the rest still apply.
pub fn editorconfig_for(path: &Path) -> EditorConfig {
    let path = if path.is_absolute() {
        normalize(path)
    } else {
        match std::env::current_dir() {
            Ok(cwd) => normalize(&cwd.join(path)),
            Err(_) => return EditorConfig::default(),
        }
    };

    let mut files = Vec::new();
    let mut dir = path.parent();
    while let Some(d) = dir {
        if let Some((parsed, root)) = read_file(d) {
            files.push(parsed);
            if root {
                break;
            }
        }
        dir = d.parent();
    }

    let mut props = Properties::new();
    // Farthest first, so nearer files win.
    for file in files.iter().rev() {
        let rel = path.strip_prefix(&file.dir).unwrap_or(&path);
        for section in &file.sections {
            // Infallible for sections.
            let _ = section.apply_to(&mut props, rel);
        }
    }
    from_properties(&props)
}

/// Reads and fully parses `dir/.editorconfig`: the sections and whether it is
/// a root. `None` if there is none, or it was skipped.
fn read_file(dir: &Path) -> Option<(Parsed, bool)> {
    let file = dir.join(".editorconfig");
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // A dangling symlink is a configuration that can't be read.
            if std::fs::symlink_metadata(&file).is_ok() {
                warn_skipped(&file, &e);
            }
            return None;
        }
        Err(e) => {
            warn_skipped(&file, &e);
            return None;
        }
    };
    // ec4rs keeps the byte order mark on a first-line section header and then
    // fails to reparse it, so it never sees the mark.
    let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let mut config =
        match ConfigParser::new_with_path(std::io::Cursor::new(text), Some(file.as_path())) {
            Ok(config) => config,
            Err(e) => {
                warn_skipped(&file, &e);
                return None;
            }
        };
    let root = config.is_root;
    let mut sections = Vec::new();
    for section in &mut config {
        match section {
            Ok(section) => sections.push(section),
            Err(e) => {
                warn_skipped(&file, &e);
                return None;
            }
        }
    }
    let parsed = Parsed {
        dir: dir.to_path_buf(),
        sections,
    };
    Some((parsed, root))
}

fn warn_skipped(file: &Path, error: &dyn std::fmt::Display) {
    log::warn!("skipping {}: {error}", file.display());
}

/// Resolves `.` and `..`, so `a/b/../c` looks in `a`, not `a/b`. A `..` after
/// a symlinked directory goes to the link target's parent, as the OS does.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.is_symlink()
                    && let Ok(real) = out.canonicalize()
                {
                    out = real;
                }
                if !out.pop() {
                    out.push(component);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// A property's value as set: `None` when no file mentions it, `Some("unset")`
/// when one unsets it, else the lowercased text.
fn raw(props: &Properties, key: &str) -> Option<String> {
    props
        .get_raw_for_key(key)
        .into_option()
        .map(str::to_ascii_lowercase)
}

fn flag(props: &Properties, key: &str) -> Option<bool> {
    match raw(props, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn from_properties(props: &Properties) -> EditorConfig {
    let indent_style = match raw(props, "indent_style").as_deref() {
        Some("tab") => Some(IndentKind::Tab),
        Some("space") => Some(IndentKind::Space),
        _ => None,
    };
    let tab_width_raw = raw(props, "tab_width");
    let valid_tab_width = tab_width_raw.as_ref().and_then(|v| v.parse::<usize>().ok());
    // `tab` is a column count once a tab width is known.
    let tab = || match valid_tab_width {
        Some(n) => IndentSize::Columns(n),
        None => IndentSize::Tab,
    };
    let indent_size = match raw(props, "indent_size").as_deref() {
        // Only a missing indent_size takes its default; unset and invalid
        // values stay None.
        None if indent_style == Some(IndentKind::Tab) => Some(tab()),
        None => None,
        Some("tab") => Some(tab()),
        Some(v) => v.parse().ok().map(IndentSize::Columns),
    };
    let tab_width = match (&tab_width_raw, indent_size) {
        (None, Some(IndentSize::Columns(n))) => Some(n),
        _ => valid_tab_width,
    };
    let end_of_line = match raw(props, "end_of_line").as_deref() {
        Some("lf") => Some(EndOfLine::Lf),
        Some("crlf") => Some(EndOfLine::Crlf),
        Some("cr") => Some(EndOfLine::Cr),
        _ => None,
    };
    EditorConfig {
        indent_style,
        indent_size,
        tab_width,
        end_of_line,
        insert_final_newline: flag(props, "insert_final_newline"),
        trim_trailing_whitespace: flag(props, "trim_trailing_whitespace"),
    }
}
