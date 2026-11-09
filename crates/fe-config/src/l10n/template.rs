//! The settings template in the active locale.
//!
//! Each `#` line of `settings-template.toml` is one of two kinds:
//!
//! - **Commented TOML**: after `# `, a table header, an array-table header
//!   or a `key = value` line, such as `# font_size = 13.0` or
//!   `# [[vim.remap]]`. It is never translated.
//! - **Prose**: any other comment line. A run of prose lines, with the `#`
//!   lines inside it, is a **block**, one message of
//!   `settings-template.ftl`. A `#` line at either end of a run stays as
//!   it is.
//!
//! A block's key comes from what follows it, past `#` lines: a table
//! header, active or commented, is that table's
//! (`settings-template-vim-remap`); a `key = value` line, active or
//! commented, is that key's in the table it is in, which a commented
//! header opens until the next header of either kind
//! (`settings-template-vim-remap-mode`). Otherwise it is
//! `settings-template-intro` before the first table, then
//! `settings-template-<table>-note-<n>`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use super::{KeyKind, setting_key};
use crate::USER_SETTINGS_TEMPLATE;

/// How wide a rendered comment line is, its `# ` included, before it wraps.
const WIDTH: usize = 80;

/// What a line of the template is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A line that isn't a comment: a table header, a setting, or blank.
    Active(Option<Toml>),
    /// A comment holding a line of TOML, after `# `.
    Commented(Toml),
    /// `#` alone.
    Blank,
    /// Any other comment.
    Prose,
}

/// A line of TOML the template's keys come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Toml {
    /// `[path]` or `[[path]]`, by its dotted path.
    Header(String),
    /// `key = value`, by its key.
    Key(String),
}

/// One block of prose: lines `start..end`, keyed `key`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) key: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// A template, split into lines and blocks.
pub(crate) struct Template<'a> {
    pub(crate) lines: Vec<&'a str>,
    #[cfg(test)]
    pub(crate) kinds: Vec<Line>,
    pub(crate) blocks: Vec<Block>,
    newline: bool,
}

impl<'a> Template<'a> {
    pub(crate) fn parse(text: &'a str) -> Self {
        let mut lines: Vec<&str> = text.split('\n').collect();
        let newline = lines.last() == Some(&"");
        if newline {
            lines.pop();
        }
        let kinds: Vec<Line> = lines.iter().map(|line| classify(line)).collect();
        let mut blocks = Vec::new();
        let mut table = String::new();
        let mut notes: HashMap<String, usize> = HashMap::new();
        let mut at = 0;
        while at < lines.len() {
            match &kinds[at] {
                Line::Active(Some(Toml::Header(path))) | Line::Commented(Toml::Header(path)) => {
                    table = path.clone();
                }
                Line::Prose => {
                    let start = at;
                    let mut end = at;
                    while end < lines.len() && matches!(kinds[end], Line::Prose | Line::Blank) {
                        end += 1;
                    }
                    while kinds[end - 1] == Line::Blank {
                        end -= 1;
                    }
                    let next = (end..lines.len()).find(|&i| kinds[i] != Line::Blank);
                    let path = match next.map(|i| &kinds[i]) {
                        Some(
                            Line::Active(Some(Toml::Header(path)))
                            | Line::Commented(Toml::Header(path)),
                        ) => path.clone(),
                        Some(
                            Line::Active(Some(Toml::Key(key))) | Line::Commented(Toml::Key(key)),
                        ) => join(&table, key),
                        _ if table.is_empty() && !notes.contains_key("") => {
                            notes.insert(String::new(), 0);
                            "intro".to_owned()
                        }
                        _ => {
                            let n = notes.entry(table.clone()).or_default();
                            *n += 1;
                            join(&table, &("note-".to_owned() + &n.to_string()))
                        }
                    };
                    blocks.push(Block {
                        key: setting_key(KeyKind::Template, &path),
                        start,
                        end,
                    });
                    at = end;
                    continue;
                }
                _ => {}
            }
            at += 1;
        }
        Self {
            lines,
            #[cfg(test)]
            kinds,
            blocks,
            newline,
        }
    }

    /// `block`'s text, its message in en-US: each line after `# `, and the
    /// `#` lines empty.
    #[cfg(test)]
    pub(crate) fn text(&self, block: &Block) -> String {
        let lines: Vec<&str> = self.lines[block.start..block.end]
            .iter()
            .map(|line| line.strip_prefix("# ").unwrap_or(""))
            .collect();
        lines.join("\n")
    }

    /// The template with each block whose key `message` gives replaced by
    /// that text, wrapped at [`WIDTH`]; a block it gives nothing for stays
    /// as it is.
    pub(crate) fn render(&self, message: impl Fn(&str) -> Option<String>) -> String {
        let mut out = String::new();
        let mut blocks = self.blocks.iter().peekable();
        let mut at = 0;
        while at < self.lines.len() {
            if let Some(block) = blocks.next_if(|block| block.start == at) {
                match message(&block.key) {
                    Some(text) => comment(&mut out, &text),
                    None => {
                        for line in &self.lines[block.start..block.end] {
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                }
                at = block.end;
                continue;
            }
            out.push_str(self.lines[at]);
            out.push('\n');
            at += 1;
        }
        if !self.newline {
            out.pop();
        }
        out
    }
}

/// `table.key`, or `key` at the top.
fn join(table: &str, key: &str) -> String {
    if table.is_empty() {
        key.to_owned()
    } else {
        format!("{table}.{key}")
    }
}

fn classify(line: &str) -> Line {
    match line.strip_prefix('#') {
        Some("") => Line::Blank,
        Some(rest) => match rest.strip_prefix(' ').and_then(toml_line) {
            Some(toml) => Line::Commented(toml),
            None => Line::Prose,
        },
        None => Line::Active(toml_line(line.trim())),
    }
}

/// What `text` is, if it's a table header or a `key = value` line of TOML.
pub(crate) fn toml_line(text: &str) -> Option<Toml> {
    let toml = if text.starts_with('[') {
        let inner = (text
            .strip_prefix("[[")
            .and_then(|rest| rest.strip_suffix("]]")))
        .or_else(|| {
            text.strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
        })?;
        Toml::Header(dotted(inner)?)
    } else {
        let (key, _) = text.split_once('=')?;
        Toml::Key(dotted(key)?)
    };
    text.parse::<toml::Table>().ok()?;
    Some(toml)
}

/// `text` as a dotted key of bare keys, spaces around each part dropped.
fn dotted(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('.').map(str::trim).collect();
    let bare = |part: &&str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    parts.iter().all(bare).then(|| parts.join("."))
}

/// Appends `text` as comment lines: each line after `# `, wrapped at
/// [`WIDTH`] keeping its indent, and empty lines as `#`.
fn comment(out: &mut String, text: &str) {
    for line in text.trim_matches('\n').split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            out.push_str("#\n");
            continue;
        }
        if 2 + line.chars().count() <= WIDTH {
            out.push_str("# ");
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let indent = &line[..line.len() - line.trim_start().len()];
        let mut row = String::new();
        let mut width = 0;
        for word in line.split_whitespace() {
            let len = word.chars().count();
            if width > 0 && 2 + indent.len() + width + 1 + len > WIDTH {
                push_row(out, indent, &row);
                row.clear();
                width = 0;
            }
            if width > 0 {
                row.push(' ');
                width += 1;
            }
            row.push_str(word);
            width += len;
        }
        push_row(out, indent, &row);
    }
}

fn push_row(out: &mut String, indent: &str, row: &str) {
    out.push_str("# ");
    out.push_str(indent);
    out.push_str(row);
    out.push('\n');
}

/// The `settings.toml` a new user starts from, in the active locale: every
/// key commented out at its default, so it changes nothing until edited.
///
/// Each prose block of [`USER_SETTINGS_TEMPLATE`] is its message in the
/// first locale of the chain that translates it, wrapped at 80 columns;
/// commented TOML and every other line stay byte for byte. A block only
/// en-US has stays as written, so in en-US this is the constant itself.
/// Rendered once per locale chain, and kept.
///
/// Reads only `kiln_l10n`'s process catalogue, so it may run on any
/// thread, at the moment a file is created.
pub fn user_settings_template() -> &'static str {
    let fallback: kiln_l10n::LanguageIdentifier = kiln_l10n::FALLBACK_LOCALE
        .parse()
        .expect("en-US is a language tag");
    let chain = kiln_l10n::locales();
    if chain.first().is_none_or(|first| *first == fallback) {
        return USER_SETTINGS_TEMPLATE;
    }
    let tags: Vec<String> = chain.iter().map(ToString::to_string).collect();
    let tags = tags.join(",");
    static RENDERED: LazyLock<Mutex<HashMap<String, &'static str>>> = LazyLock::new(Mutex::default);
    if let Some(text) = RENDERED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&tags)
    {
        return text;
    }
    static TEMPLATE: LazyLock<Template<'static>> =
        LazyLock::new(|| Template::parse(USER_SETTINGS_TEMPLATE));
    let args = kiln_l10n::Args::new();
    let rendered = TEMPLATE.render(|key| {
        let formatted = kiln_l10n::try_format(key, &args)?;
        (formatted.locale != fallback)
            .then(|| kiln_l10n::strip_isolates(&formatted.text).into_owned())
    });
    let mut rendered_by_chain = RENDERED.lock().unwrap_or_else(PoisonError::into_inner);
    // A handful of chains at most, in a process's life.
    rendered_by_chain
        .entry(tags)
        .or_insert_with(|| Box::leak(rendered.into_boxed_str()))
}
