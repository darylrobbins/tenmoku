//! A reply's code block read as an edit (the decision record's § 8): the
//! fence's info string names the file, ` ```rust path=src/lib.rs `, and the
//! body is the whole file, or the selection the turn was asked about when
//! there's no path, or `<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE`
//! sections, each marker exactly so on a line of its own.

use kiln_l10n::tr;
use std::{fmt, path::Component};

/// The line opening a section's search text.
pub const SEARCH_MARKER: &str = "<<<<<<< SEARCH";
/// The line between a section's search text and its replacement.
pub const DIVIDER_MARKER: &str = "=======";
/// The line closing a section.
pub const REPLACE_MARKER: &str = ">>>>>>> REPLACE";

/// A code block as an edit: see [`parse_edit_block`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditBlock {
    /// The file it's for, relative to the project's root, as written.
    pub path: Option<String>,
    pub body: EditBody,
}

/// What a block does to its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditBody {
    /// Replaces the whole file, or with no path the selection.
    Replace(String),
    /// Replaces each search text, the first of each pair, with the second,
    /// in the order written. Each line keeps its `\n`.
    Sections(Vec<(String, String)>),
}

/// Why a code block isn't an edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditBlockError {
    /// A marker out of place, one left open, or text between sections, at
    /// `line` of the code, 1-based.
    Malformed { line: usize },
    /// The path is absolute or leaves the root with `..`.
    Path(String),
}

impl fmt::Display for EditBlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditBlockError::Malformed { line } => {
                f.write_str(&tr!("chat-edit-markers-malformed", line = line.to_string()))
            }
            EditBlockError::Path(path) => f.write_str(&tr!("chat-edit-path-outside", path = path)),
        }
    }
}

impl std::error::Error for EditBlockError {}

/// The edit a fence with info string `info` holding `code` makes.
///
/// # Errors
/// [`EditBlockError::Path`] if its `path=` is absolute or has a `..`;
/// [`EditBlockError::Malformed`] if its markers don't make whole sections
/// with nothing but blank lines between them.
pub fn parse_edit_block(info: &str, code: &str) -> Result<EditBlock, EditBlockError> {
    let path = info_path(info).map(str::to_owned);
    if let Some(path) = &path
        && !is_relative_within(path)
    {
        return Err(EditBlockError::Path(path.clone()));
    }
    let body = if (code.split('\n')).any(|line| marker(line).is_some()) {
        EditBody::Sections(sections(code)?)
    } else {
        EditBody::Replace(code.to_owned())
    };
    Ok(EditBlock { path, body })
}

/// The `path=` among a fence's info string's words, its value bare or in
/// double quotes, as the block's header shows it.
pub fn info_path(info: &str) -> Option<&str> {
    let (at, _) = (info.match_indices("path=")).find(|(at, _)| {
        info[..*at]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
    })?;
    let value = &info[at + "path=".len()..];
    let path = match value.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next(),
        None => value.split_whitespace().next(),
    };
    path.filter(|path| !path.is_empty())
}

/// Whether `path` stays under the root it's joined to: not absolute, no
/// drive, and no `..`, split at either slash.
fn is_relative_within(path: &str) -> bool {
    let absolute = path.starts_with(['/', '\\'])
        || std::path::Path::new(path).is_absolute()
        || (path.split(['/', '\\']).next()).is_some_and(|first| first.contains(':'));
    let parent = (path.split(['/', '\\'])).any(|part| part == "..")
        || (std::path::Path::new(path).components())
            .any(|part| matches!(part, Component::ParentDir));
    !absolute && !parent
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    Search,
    Divider,
    Replace,
}

/// The marker `line` is, a `\r` ending it aside.
fn marker(line: &str) -> Option<Marker> {
    match line.strip_suffix('\r').unwrap_or(line) {
        SEARCH_MARKER => Some(Marker::Search),
        DIVIDER_MARKER => Some(Marker::Divider),
        REPLACE_MARKER => Some(Marker::Replace),
        _ => None,
    }
}

/// `code`'s sections, in order.
fn sections(code: &str) -> Result<Vec<(String, String)>, EditBlockError> {
    enum State {
        Between,
        /// In the search text of a section opened at this line.
        Search(usize),
        Replace(usize),
    }
    let mut state = State::Between;
    let (mut search, mut replace) = (String::new(), String::new());
    let mut sections = Vec::new();
    for (ix, line) in code.split_inclusive('\n').enumerate() {
        let number = ix + 1;
        let text = line.strip_suffix('\n').unwrap_or(line);
        state = match (state, marker(text)) {
            (State::Between, Some(Marker::Search)) => State::Search(number),
            (State::Between, None) if text.trim().is_empty() => State::Between,
            (State::Search(_) | State::Replace(_), Some(Marker::Search))
            | (State::Between | State::Replace(_), Some(Marker::Divider))
            | (State::Between | State::Search(_), Some(Marker::Replace))
            | (State::Between, None) => return Err(EditBlockError::Malformed { line: number }),
            (State::Search(open), Some(Marker::Divider)) => State::Replace(open),
            (State::Replace(_), Some(Marker::Replace)) => {
                let section = (std::mem::take(&mut search), std::mem::take(&mut replace));
                sections.push(section);
                State::Between
            }
            (State::Search(open), None) => {
                search.push_str(line);
                State::Search(open)
            }
            (State::Replace(open), None) => {
                replace.push_str(line);
                State::Replace(open)
            }
        };
    }
    match state {
        State::Between => Ok(sections),
        // Never closed.
        State::Search(open) | State::Replace(open) => Err(EditBlockError::Malformed { line: open }),
    }
}
