//! Edits to the user's `keymap.toml` from the Keyboard Shortcuts editor:
//! add a binding, remove one, change its keys, or reset a command to its
//! defaults. The file is edited as text, at the spans `toml_edit` finds,
//! so every byte outside the entries an edit touches is kept: comments,
//! layout, line endings, a byte order mark, a missing final newline.
//!
//! The user's file is read after the defaults (kiln-core's `keymap`
//! module): a later entry with the same keys and context wins, and
//! `command = ""` removes every earlier binding with the same keys and
//! context. So a default is removed by appending such an unbind entry, as
//! VS Code's keybindings.json removes one with a `-` command, and a user
//! entry by deleting it.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use fe_command::{CommandId, ContextPredicate, InvalidCommandId, PredicateError};
use fe_config::Diagnostic;
use kiln_core::Os;
use toml_edit::{ArrayOfTables, Document, DocumentMut, Item, Table, Value};

use crate::{
    DEFAULT, DEFAULT_KEYMAP_PATH, Entry, KEYMAP_FILE, KeySequence, Keymap, KeymapIssue,
    KeymapLayer, KeystrokeError,
};

/// The top of a keymap file [`write_keymap`] creates.
pub const KEYMAP_HEADER: &str = "\
# fastedit keymap.
#
# This file reloads when you save it. Its bindings are read after the
# built-in ones (fe-keymap's keymaps/fastedit.toml): an entry with the same
# keys and context as a built-in binding replaces it, and one with
# `command = \"\"` removes it. The format is described in kiln-core's
# `keymap` module docs.
";

/// A change to the user's keymap. Keys, contexts and commands are as
/// they're written in a keymap file: `cmd-k cmd-0`, `editor && !vim`,
/// `editor.fold_all`. `context` `None` is everywhere.
#[derive(Clone, Debug, PartialEq)]
pub enum KeymapEdit {
    /// Binds `keys` to `command`, after every other entry, so it wins over
    /// any with the same keys and context.
    Add {
        keys: String,
        context: Option<String>,
        command: String,
        /// The command's args: a table, such as `{ motion = "left" }`.
        args: Option<toml::Value>,
    },
    /// Unbinds `keys` in `context`: deletes the user's entries for them,
    /// and appends an unbind entry if a default still binds them.
    Remove {
        keys: String,
        context: Option<String>,
    },
    /// Moves the binding for `old_keys` in `context` to `new_keys`: a
    /// user entry's keys are rewritten; a default is unbound and bound
    /// again at `new_keys`.
    Change {
        old_keys: String,
        context: Option<String>,
        new_keys: String,
    },
    /// Deletes every user entry for `command` and every unbind entry that
    /// removes one of its defaults, so it has its default bindings again.
    ResetCommand { command: String },
}

/// Why an edit wasn't made. Nothing is written for any of them.
#[derive(Debug, thiserror::Error)]
pub enum KeymapEditError {
    #[error("keys {keys:?}: {error}")]
    Keys { keys: String, error: KeystrokeError },
    #[error("context {context:?}: {error}")]
    Context {
        context: String,
        error: PredicateError,
    },
    #[error(transparent)]
    Command(#[from] InvalidCommandId),
    #[error("args must be a table, such as {{ motion = \"left\" }}")]
    Args,
    #[error("{keys} isn't bound{}", in_context(context))]
    NotBound {
        keys: String,
        context: Option<String>,
    },
    /// The keymap file isn't TOML.
    #[error("the keymap isn't TOML: {0}")]
    NotToml(String),
    /// The file's `bindings` isn't a list of `[[bindings]]` tables, such as
    /// an inline array, which isn't edited.
    #[error("the keymap's `bindings` isn't a list of [[bindings]] tables")]
    Layout,
    /// The edited keymap doesn't load, or drops the entry the edit made:
    /// its diagnostics.
    #[error("the edited keymap doesn't load: {}", first_message(.0))]
    Invalid(Vec<Diagnostic>),
    /// The caller's check of the text to edit refused the edit.
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn in_context(context: &Option<String>) -> String {
    context
        .as_ref()
        .map_or(String::new(), |context| format!(" in {context}"))
}

fn first_message(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.is_error())
        .or(diagnostics.first())
        .map_or_else(|| "an entry was dropped".into(), ToString::to_string)
}

/// A default keymap file the user's sits on: where it's from, for
/// diagnostics, and its text, read for whichever platform an edit needs.
pub type DefaultKeymap<'a> = (&'a Path, &'a str);

/// fastedit's built-in keymap, the one default layer.
fn built_in() -> [DefaultKeymap<'static>; 1] {
    [(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT)]
}

/// The user keymap `source` with `edit` made, on fastedit's built-in
/// keymap on this platform. See [`edit_keymap_on`].
pub fn edit_keymap(source: &str, edit: &KeymapEdit) -> Result<String, KeymapEditError> {
    edit_keymap_on(&built_in(), source, edit, Os::current())
}

/// The user keymap `source`, read after the `defaults` on `os`, with `edit`
/// made. Every byte outside the entries it adds, rewrites or deletes is
/// kept; a deleted entry goes with the comment lines directly above it.
/// What's added uses the file's line endings.
///
/// The edit's keys, context and command are checked first, and the result
/// must load as a [`KeymapLayer`] with every entry the edit wrote in it,
/// or it's an error: an edit that changes nothing included.
pub fn edit_keymap_on(
    defaults: &[DefaultKeymap<'_>],
    source: &str,
    edit: &KeymapEdit,
    os: Os,
) -> Result<String, KeymapEditError> {
    let editor = Editor::new(defaults, os);
    let mut text = source.to_owned();
    // The tables the edit wrote, by index once it's made.
    let mut written = Vec::new();
    match edit {
        KeymapEdit::Add {
            keys,
            context,
            command,
            args,
        } => {
            parse_keys(keys, os)?;
            parse_context(context.as_deref())?;
            CommandId::parse(command)?;
            let args = args.as_ref().map(args_text).transpose()?;
            written.push(append(
                &mut text,
                os,
                keys,
                context.as_deref(),
                command,
                args,
            )?);
        }
        KeymapEdit::Remove { keys, context } => {
            let parsed = (parse_keys(keys, os)?, parse_context(context.as_deref())?);
            let tables = parse_tables(&text, os)?;
            if editor.effective(&tables, &parsed).is_none() {
                return Err(not_bound(keys, context));
            }
            let doomed = tables.iter().enumerate().filter(|(_, table)| {
                table
                    .entry
                    .as_ref()
                    .is_some_and(|entry| entry.command.is_some() && same(entry, &parsed))
            });
            let doomed: Vec<usize> = doomed.map(|(ix, _)| ix).collect();
            delete(&mut text, &doomed, os)?;
            if editor
                .effective(&parse_tables(&text, os)?, &parsed)
                .is_some()
            {
                written.push(append(&mut text, os, keys, context.as_deref(), "", None)?);
            }
        }
        KeymapEdit::Change {
            old_keys,
            context,
            new_keys,
        } => {
            let parsed = (
                parse_keys(old_keys, os)?,
                parse_context(context.as_deref())?,
            );
            parse_keys(new_keys, os)?;
            match editor.effective(&parse_tables(&text, os)?, &parsed) {
                None => return Err(not_bound(old_keys, context)),
                Some(Effective::User(ix)) => {
                    set_keys(&mut text, ix, new_keys, os)?;
                    written.push(ix);
                }
                Some(Effective::Default { command, args }) => {
                    let args = args_from_json(&args)?;
                    let context = context.as_deref();
                    written.push(append(&mut text, os, old_keys, context, "", None)?);
                    let command = command.as_str();
                    written.push(append(&mut text, os, new_keys, context, command, args)?);
                }
            }
        }
        KeymapEdit::ResetCommand { command } => {
            let command = CommandId::parse(command)?;
            let mut defaults_on = DefaultsByOs::default();
            let doomed: Vec<usize> = parse_tables(&text, os)?
                .iter()
                .enumerate()
                .filter(|(_, table)| match table.command.as_deref() {
                    // Bound to it, here or on another platform.
                    Some(text) if text == command.as_str() => true,
                    // An unbind of one of its defaults, on any platform.
                    Some("") => Os::ALL.into_iter().any(|on| {
                        let entry = if on == os {
                            table.entry.clone()
                        } else {
                            entry_on(&table.table, on)
                        };
                        entry.is_some_and(|entry| {
                            defaults_on
                                .keymap(&editor, on)
                                .bindings()
                                .iter()
                                .any(|binding| {
                                    binding.command == command
                                        && binding.keys == entry.keys
                                        && binding.context == entry.context
                                })
                        })
                    }),
                    _ => false,
                })
                .map(|(ix, _)| ix)
                .collect();
            delete(&mut text, &doomed, os)?;
        }
    }
    let (layer, diagnostics) = KeymapLayer::parse_for(Path::new(KEYMAP_FILE), &text, None, os);
    let tables = parse_tables(&text, os)?;
    let dropped = written.iter().any(|&ix| tables[ix].entry.is_none());
    if layer.is_none() || dropped {
        return Err(KeymapEditError::Invalid(diagnostics));
    }
    Ok(text)
}

/// What [`write_keymap`] wrote.
#[derive(Clone, Debug)]
pub struct KeymapWrite {
    /// The file's new text.
    pub source: String,
    /// The defaults with the new file over them.
    pub keymap: Keymap,
    /// The new keymap's issues: bindings that override, shadow or delay
    /// each other ([`Keymap::report`]).
    pub issues: Vec<KeymapIssue>,
    /// The new file's warnings, such as an unknown field.
    pub diagnostics: Vec<Diagnostic>,
}

/// Makes `edit` to the user keymap at `path`, on fastedit's built-in
/// keymap on this platform. See [`write_keymap_on`].
pub fn write_keymap(path: &Path, edit: &KeymapEdit) -> Result<KeymapWrite, KeymapEditError> {
    write_keymap_on(path, &built_in(), edit, Os::current())
}

/// Makes `edit` ([`edit_keymap_on`]) to the user keymap at `path`, read
/// after the `defaults` on `os`, and rebuilds the keymap. A missing file
/// starts as [`KEYMAP_HEADER`], and is created with its directory; an edit
/// that changes nothing writes nothing.
///
/// The new text goes to a temporary file in the same directory, with the
/// old file's permissions, renamed over it, or linked in place of a
/// missing one, so a crash never leaves the file cut short. A symlink is
/// written through to its target. If the file changed since it was read
/// (saved, created, given other permissions, or linked elsewhere), the
/// edit is made again to what's there now, once, so the other change isn't
/// lost; if it changed again, that's an error.
pub fn write_keymap_on(
    path: &Path,
    defaults: &[DefaultKeymap<'_>],
    edit: &KeymapEdit,
    os: Os,
) -> Result<KeymapWrite, KeymapEditError> {
    write_keymap_hooked(path, defaults, edit, os, || {})
}

/// [`write_keymap_on`], running `before_put` once the new text is ready
/// and before it replaces the file: for tests to change the file then.
#[doc(hidden)]
pub fn write_keymap_hooked(
    path: &Path,
    defaults: &[DefaultKeymap<'_>],
    edit: &KeymapEdit,
    os: Os,
    before_put: impl FnMut(),
) -> Result<KeymapWrite, KeymapEditError> {
    write_keymap_checked(path, defaults, edit, os, |_| Ok(()), before_put)
}

/// [`write_keymap_hooked`], first running `check` on the text each round
/// edits, the file as read then, including a second round's after a change
/// under us; an `Err` refuses the edit as [`KeymapEditError::Refused`] and
/// nothing is written.
#[doc(hidden)]
pub fn write_keymap_checked(
    path: &Path,
    defaults: &[DefaultKeymap<'_>],
    edit: &KeymapEdit,
    os: Os,
    mut check: impl FnMut(&str) -> Result<(), String>,
    mut before_put: impl FnMut(),
) -> Result<KeymapWrite, KeymapEditError> {
    // A second round if the file changes under us.
    for _ in 0..2 {
        let target = resolve(path)?;
        let read = match fs::read_to_string(&target) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let source = read.as_deref().unwrap_or(KEYMAP_HEADER);
        check(source).map_err(KeymapEditError::Refused)?;
        let edited = edit_keymap_on(defaults, source, edit, os)?;
        before_put();
        let still = || resolve(path).is_ok_and(|now| now == target);
        let done = if edited == source {
            // Nothing to write, if what was read is still what's there.
            let now = fs::read(&target).ok();
            still() && now.as_deref() == read.as_deref().map(str::as_bytes)
        } else {
            if read.is_none()
                && let Some(dir) = target.parent()
            {
                fs::create_dir_all(dir)?;
            }
            put(&target, edited.as_bytes(), read.as_deref(), still)?
        };
        if !done {
            continue;
        }
        let (layer, diagnostics) = KeymapLayer::parse_for(&target, &edited, None, os);
        let Some(layer) = layer else {
            return Err(KeymapEditError::Invalid(diagnostics));
        };
        let mut layers = Editor::new(defaults, os).layers;
        layers.push(layer);
        let keymap = Keymap::from_layers(&layers);
        let issues = keymap.report();
        return Ok(KeymapWrite {
            source: edited,
            keymap,
            issues,
            diagnostics,
        });
    }
    Err(io::Error::other(format!("{} keeps changing", path.display())).into())
}

/// The defaults an edit is judged on.
struct Editor<'a> {
    defaults: &'a [DefaultKeymap<'a>],
    /// The defaults as on the edit's platform.
    layers: Vec<KeymapLayer>,
}

/// The binding that runs for some keys in a context.
enum Effective {
    /// The user's entry at this index.
    User(usize),
    /// A default.
    Default {
        command: CommandId,
        args: serde_json::Value,
    },
}

type KeysAndContext = (KeySequence, Option<ContextPredicate>);

impl<'a> Editor<'a> {
    fn new(defaults: &'a [DefaultKeymap<'a>], os: Os) -> Self {
        Self {
            defaults,
            layers: layers_on(defaults, os),
        }
    }

    /// The binding for `parsed`'s keys and context with the defaults and
    /// the user's `tables`: the last one read, which wins.
    fn effective(&self, tables: &[TableText], parsed: &KeysAndContext) -> Option<Effective> {
        let user = KeymapLayer {
            path: PathBuf::from(KEYMAP_FILE),
            entries: tables
                .iter()
                .flat_map(|table| table.entry.clone())
                .collect(),
        };
        let keymap = Keymap::from_layers(self.layers.iter().chain([&user]));
        let binding = keymap
            .bindings()
            .iter()
            .rev()
            .find(|binding| binding.keys == parsed.0 && binding.context == parsed.1)?;
        if binding.layer < self.layers.len() {
            return Some(Effective::Default {
                command: binding.command.clone(),
                args: binding.args.clone(),
            });
        }
        // The last of the user's for them: no unbind follows it, or it
        // wouldn't be bound.
        tables
            .iter()
            .rposition(|table| {
                table
                    .entry
                    .as_ref()
                    .is_some_and(|entry| entry.command.is_some() && same(entry, parsed))
            })
            .map(Effective::User)
    }
}

/// The defaults' keymaps by platform, parsed as they're needed.
#[derive(Default)]
struct DefaultsByOs(Vec<(Os, Keymap)>);

impl DefaultsByOs {
    fn keymap(&mut self, editor: &Editor<'_>, os: Os) -> &Keymap {
        let ix = match self.0.iter().position(|(on, _)| *on == os) {
            Some(ix) => ix,
            None => {
                let keymap = Keymap::from_layers(layers_on(editor.defaults, os));
                self.0.push((os, keymap));
                self.0.len() - 1
            }
        };
        &self.0[ix].1
    }
}

/// The defaults as on `os`; one with errors is empty.
fn layers_on(defaults: &[DefaultKeymap<'_>], os: Os) -> Vec<KeymapLayer> {
    defaults
        .iter()
        .map(|(path, source)| {
            KeymapLayer::parse_for(path, source, None, os)
                .0
                .unwrap_or_default()
        })
        .collect()
}

/// One `[[bindings]]` table of the user's file, where it is in the text.
struct TableText {
    /// Its text, in order: under its `[[bindings]]` header, and under each
    /// header of a table of its own, such as `[bindings.args]`, that
    /// another table's comes between.
    sections: Vec<Section>,
    /// It as parsed, for reading as on other platforms.
    table: Table,
    /// Its entry as on the edit's platform: `None` if it's left out there
    /// or has errors.
    entry: Option<Entry>,
    /// Its `command`, if it's a string.
    command: Option<String>,
}

/// The text under a header, up to the next.
#[derive(Clone, Copy)]
struct Section {
    /// Where the blank lines and comments above its header start.
    above: usize,
    /// Its header.
    header: usize,
    /// Its end: the next header's comments, the comments ending the file,
    /// or the end.
    end: usize,
}

/// The `[[bindings]]` tables of `source`, in order.
fn parse_tables(source: &str, os: Os) -> Result<Vec<TableText>, KeymapEditError> {
    let not_toml = |error: toml_edit::TomlError| KeymapEditError::NotToml(error.to_string());
    let document = Document::parse(source).map_err(not_toml)?;
    let editable: DocumentMut = source.parse().map_err(not_toml)?;
    let (Some(spanned), Some(owned)) = (
        document.get("bindings"),
        editable.get("bindings").and_then(Item::as_array_of_tables),
    ) else {
        return match document.get("bindings") {
            None => Ok(Vec::new()),
            Some(_) => Err(KeymapEditError::Layout),
        };
    };
    let spanned = spanned
        .as_array_of_tables()
        .ok_or(KeymapEditError::Layout)?;
    // Where every header and its text start, with the bindings table it's
    // part of, if any.
    let mut starts = Vec::new();
    for (key, item) in document.iter() {
        match item {
            Item::Table(table) => header_starts(table, None, &mut starts),
            Item::ArrayOfTables(tables) => {
                for (ix, table) in tables.iter().enumerate() {
                    let owner = (key == "bindings").then_some(ix);
                    header_starts(table, owner, &mut starts);
                }
            }
            _ => {}
        }
    }
    let trailing = document
        .trailing()
        .span()
        .map_or(source.len(), |span| span.start);
    spanned
        .iter()
        .zip(owned.iter())
        .enumerate()
        .map(|(ix, (spanned, table))| {
            spanned.span().ok_or(KeymapEditError::Layout)?;
            let mut own: Vec<_> = starts
                .iter()
                .filter(|(.., owner)| *owner == Some(ix))
                .collect();
            own.sort();
            let mut sections: Vec<Section> = Vec::new();
            for &&(above, header, _) in &own {
                let end = starts
                    .iter()
                    .map(|&(start, ..)| start)
                    .filter(|&start| start > header)
                    .chain([trailing, source.len()])
                    .min()
                    .unwrap_or(source.len());
                match sections.last_mut() {
                    // Straight after the last: one section.
                    Some(last) if last.end == above => last.end = end,
                    _ => sections.push(Section { above, header, end }),
                }
            }
            Ok(TableText {
                sections,
                entry: entry_on(table, os),
                command: table.get("command").and_then(Item::as_str).map(Into::into),
                table: table.clone(),
            })
        })
        .collect()
}

/// Where the text and header of `table` and of the tables under it with
/// headers start, pushed to `starts` with `owner`.
fn header_starts(
    table: &Table,
    owner: Option<usize>,
    starts: &mut Vec<(usize, usize, Option<usize>)>,
) {
    if !table.is_implicit()
        && !table.is_dotted()
        && let (Some(start), Some(header)) = (text_start(table), table.span())
    {
        starts.push((start, header.start, owner));
    }
    for (_, item) in table.iter() {
        match item {
            Item::Table(table) => header_starts(table, owner, starts),
            Item::ArrayOfTables(tables) => {
                for table in tables.iter() {
                    header_starts(table, owner, starts);
                }
            }
            _ => {}
        }
    }
}

/// Where a table's text starts: the blank lines and comments above its
/// header, or the header.
fn text_start(table: &Table) -> Option<usize> {
    let prefix = table.decor().prefix().and_then(|prefix| prefix.span());
    prefix.or(table.span()).map(|span| span.start)
}

/// A `[[bindings]]` table's entry as on `os`: `None` if it's left out
/// there or has errors.
fn entry_on(table: &Table, os: Os) -> Option<Entry> {
    let mut one = ArrayOfTables::new();
    one.push(table.clone());
    let mut document = DocumentMut::new();
    document.insert("bindings", Item::ArrayOfTables(one));
    let source = document.to_string();
    let (layer, _) = KeymapLayer::parse_for(Path::new(KEYMAP_FILE), &source, None, os);
    layer.and_then(|layer| layer.entries.into_iter().next())
}

fn same(entry: &Entry, (keys, context): &KeysAndContext) -> bool {
    &entry.keys == keys && &entry.context == context
}

fn not_bound(keys: &str, context: &Option<String>) -> KeymapEditError {
    KeymapEditError::NotBound {
        keys: keys.to_owned(),
        context: context.clone(),
    }
}

fn parse_keys(keys: &str, os: Os) -> Result<KeySequence, KeymapEditError> {
    KeySequence::parse_for(keys, os).map_err(|error| KeymapEditError::Keys {
        keys: keys.to_owned(),
        error,
    })
}

fn parse_context(context: Option<&str>) -> Result<Option<ContextPredicate>, KeymapEditError> {
    context
        .map(|context| {
            ContextPredicate::parse(context).map_err(|error| KeymapEditError::Context {
                context: context.to_owned(),
                error,
            })
        })
        .transpose()
}

/// `args` as an inline table, to write.
fn args_text(args: &toml::Value) -> Result<String, KeymapEditError> {
    if !args.is_table() {
        return Err(KeymapEditError::Args);
    }
    let value: Value = args
        .to_string()
        .parse()
        .map_err(|_| KeymapEditError::Args)?;
    Ok(value.to_string().trim().to_owned())
}

/// A binding's args, as kiln-core keeps them, to write again: `None` for
/// none (`Null`).
fn args_from_json(args: &serde_json::Value) -> Result<Option<String>, KeymapEditError> {
    if args.is_null() {
        return Ok(None);
    }
    let args = toml::Value::try_from(args).map_err(|_| KeymapEditError::Args)?;
    args_text(&args).map(Some)
}

/// `text` as a TOML string.
fn quoted(text: &str) -> String {
    Value::from(text).to_string().trim().to_owned()
}

/// The line ending `text` uses: its first one's, `\n` if it has none.
fn line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(end) if text[..end].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// Appends a `[[bindings]]` entry to the end of `text`, its fields in
/// fastedit.toml's order, set apart from what's above by a blank line;
/// `command` `""` unbinds. Comments ending the file stay above it, so
/// they aren't taken for its own when it's deleted. Returns its index.
fn append(
    text: &mut String,
    os: Os,
    keys: &str,
    context: Option<&str>,
    command: &str,
    args: Option<String>,
) -> Result<usize, KeymapEditError> {
    let ix = parse_tables(text, os)?.len();
    let eol = line_ending(text);
    let mut lines = vec![
        "[[bindings]]".to_owned(),
        format!("keys = {}", quoted(keys)),
        format!("command = {}", quoted(command)),
    ];
    if let Some(context) = context {
        lines.push(format!("context = {}", quoted(context)));
    }
    if let Some(args) = args {
        lines.push(format!("args = {args}"));
    }
    if !text.trim_start_matches('\u{feff}').trim().is_empty() {
        if !text.ends_with('\n') {
            text.push_str(eol);
        }
        let last_line = text.strip_suffix('\n').unwrap_or(text);
        let last_line = last_line.strip_suffix('\r').unwrap_or(last_line);
        if !last_line.ends_with('\n') {
            text.push_str(eol);
        }
    }
    for line in lines {
        text.push_str(&line);
        text.push_str(eol);
    }
    Ok(ix)
}

/// Sets the keys of the user's entry at `ix` on `os` to `keys`: the
/// string, or, for keys by platform, this platform's, other platforms'
/// kept.
fn set_keys(text: &mut String, ix: usize, keys: &str, os: Os) -> Result<(), KeymapEditError> {
    let quoted = quoted(keys);
    let (range, new) = {
        let document = Document::parse(text.as_str())
            .map_err(|error| KeymapEditError::NotToml(error.to_string()))?;
        let table = document
            .get("bindings")
            .and_then(Item::as_array_of_tables)
            .and_then(|tables| tables.get(ix))
            .ok_or(KeymapEditError::Layout)?;
        let span = |value: &Value| value.span().ok_or(KeymapEditError::Layout);
        match table.get("keys") {
            Some(Item::Value(Value::InlineTable(by_os))) => {
                if let Some(value) = by_os.get(os.name()) {
                    (span(value)?, quoted)
                } else if let Some(last) = by_os
                    .iter()
                    .map(|(_, value)| value.span())
                    .max_by_key(|span| span.as_ref().map(|span| span.end))
                {
                    let end = last.ok_or(KeymapEditError::Layout)?.end;
                    (end..end, format!(", {} = {quoted}", os.name()))
                } else {
                    let all = by_os.span().ok_or(KeymapEditError::Layout)?;
                    (all, format!("{{ {} = {quoted} }}", os.name()))
                }
            }
            Some(Item::Value(value)) => (span(value)?, quoted),
            Some(Item::Table(by_os)) => {
                if let Some(Item::Value(value)) = by_os.get(os.name()) {
                    (span(value)?, quoted)
                } else {
                    // A line of its own after the last one's.
                    let last = by_os
                        .iter()
                        .filter_map(|(_, item)| item.span())
                        .map(|span| span.end)
                        .max()
                        .ok_or(KeymapEditError::Layout)?;
                    let key = if by_os.is_dotted() {
                        format!("keys.{}", os.name())
                    } else {
                        os.name().to_owned()
                    };
                    let eol = line_ending(text);
                    match text[last..].find('\n') {
                        Some(at) => {
                            let at = last + at + 1;
                            (at..at, format!("{key} = {quoted}{eol}"))
                        }
                        None => (text.len()..text.len(), format!("{eol}{key} = {quoted}")),
                    }
                }
            }
            _ => return Err(KeymapEditError::Layout),
        }
    };
    text.replace_range(range, &new);
    Ok(())
}

/// Deletes the user's tables at `doomed`, in ascending order, each with
/// the comment lines directly above it. What's above those (blank lines, a
/// comment set apart by one) is kept; blank lines after the table go if
/// there's one above it, so the gap isn't doubled.
fn delete(text: &mut String, doomed: &[usize], os: Os) -> Result<(), KeymapEditError> {
    // From the last, so the indices and offsets before it hold.
    for &ix in doomed.iter().rev() {
        let sections = parse_tables(text, os)?.swap_remove(ix).sections;
        for section in sections.iter().rev() {
            delete_section(text, section);
        }
    }
    Ok(())
}

/// Deletes the text under a header with the comment lines directly above
/// it: see [`delete`].
fn delete_section(text: &mut String, section: &Section) {
    let above = &text[section.above..section.header];
    let kept = section.above + kept_above(above);
    let mut start = kept;
    let mut end = section.end;
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    if text[bom..kept].trim().is_empty() {
        // Nothing above it: what follows starts the file.
        start = bom;
        end += blank_lines(&text[end..]);
    } else if end == text.len() {
        // Nor below it: the file ends with what's above.
        start = section.above + content_end(&text[section.above..kept]);
    } else if content_end(&text[section.above..kept]) < kept - section.above {
        // A blank line above it and below: keep the one above.
        end += blank_lines(&text[end..]);
    }
    text.replace_range(start..end, "");
}

/// The length of the part of the text above a header that isn't the
/// comment lines directly above it: up to and including the last blank
/// line. Whatever is before the header on its own line is the header's.
fn kept_above(above: &str) -> usize {
    let mut end = 0;
    let mut start = 0;
    for line in above.split_inclusive('\n') {
        start += line.len();
        if line.ends_with('\n') && line.trim().is_empty() {
            end = start;
        }
    }
    end
}

/// The length of `text` up to the end of its last line that isn't blank.
fn content_end(text: &str) -> usize {
    let mut end = 0;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        start += line.len();
        if !line.trim().is_empty() {
            end = start;
        }
    }
    end
}

/// The length of the blank lines starting `text`.
fn blank_lines(text: &str) -> usize {
    text.split_inclusive('\n')
        .take_while(|line| line.ends_with('\n') && line.trim().is_empty())
        .map(str::len)
        .sum()
}

/// `path` with every symlink at its end followed: the file a write goes
/// to, which may not exist yet.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_owned();
    for _ in 0..32 {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = fs::read_link(&path)?;
                path = match path.parent() {
                    Some(dir) => dir.join(link),
                    None => link,
                };
            }
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "too many levels of symbolic links at {}",
        path.display()
    )))
}

/// Writes `contents` to `path` through a temporary file beside it, if
/// `still` holds once that's synced: renamed over the file if it still
/// holds `read` and has the permissions it had, which it keeps; or, if it
/// was missing (`read` `None`), linked in its place if it still is.
/// `Ok(false)` if not, with nothing written.
fn put(
    path: &Path,
    contents: &[u8],
    read: Option<&str>,
    still: impl FnOnce() -> bool,
) -> io::Result<bool> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let permissions = || {
        fs::metadata(path)
            .ok()
            .map(|metadata| metadata.permissions())
    };
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let kept = read.and_then(|_| permissions());
        if let Some(kept) = &kept {
            file.set_permissions(kept.clone())?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        // Checked last, after the slow sync, so a change in between is rare.
        if !still() {
            return Ok(false);
        }
        let Some(read) = read else {
            return match fs::hard_link(&temporary, path) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
                linked => linked.map(|()| true),
            };
        };
        if fs::read(path).ok().as_deref() != Some(read.as_bytes()) || permissions() != kept {
            return Ok(false);
        }
        fs::rename(&temporary, path).map(|()| true)
    })();
    // Unless it was renamed into place.
    if read.is_none() || !matches!(written, Ok(true)) {
        let _ = fs::remove_file(&temporary);
    }
    written
}
