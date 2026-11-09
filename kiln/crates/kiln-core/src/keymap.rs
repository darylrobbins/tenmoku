// SPDX-License-Identifier: Apache-2.0

//! Keymap files and the keymap layered from them.
//!
//! A keymap file is a flat list of bindings:
//!
//! ```toml
//! [[bindings]]
//! keys = "cmd-k cmd-0"
//! command = "editor.fold_all"
//! context = "editor"                                 # optional; omitted = everywhere
//! args = { motion = "word_left", extend = true }     # optional
//!
//! [[bindings]]                                       # unbind
//! keys = "alt-z"
//! context = "editor"
//! command = ""
//! ```

use std::{
    borrow::Borrow,
    fmt,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use crate::{Diagnostic, Layer, Os, Severity, platform::PlatformFilter};
use kiln_command::{
    CommandId, CommandTarget, ContextPredicate, ContextStack, DispatchError, Invocation, Registry,
    Value,
};
use kiln_l10n::tr;
use toml::de::{DeTable, DeValue};

use crate::keystroke::{Key, KeySequence, Keystroke};

/// Where something is in a keymap file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: PathBuf,
    /// 1-based.
    pub line: usize,
    /// 1-based, in chars.
    pub column: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file.display(), self.line, self.column)
    }
}

/// One `[[bindings]]` entry of a keymap file that loaded cleanly.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub keys: KeySequence,
    /// `None` binds them everywhere.
    pub context: Option<ContextPredicate>,
    /// `None` for `command = ""`, which unbinds the keys.
    pub command: Option<CommandId>,
    /// `Null` when the entry has none.
    pub args: Value,
    /// The entry's `[[bindings]]` header, or its inline table.
    pub location: Location,
}

/// A keymap file's entries, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeymapLayer {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
}

/// A command a binding runs, with its args, and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub keys: KeySequence,
    pub context: Option<ContextPredicate>,
    pub command: CommandId,
    /// `Null` when the entry has none.
    pub args: Value,
    /// The index of the layer it came from in [`Keymap::from_layers`]:
    /// 0 for the defaults.
    pub layer: usize,
    pub location: Location,
}

impl Binding {
    /// How deep in `stack` the binding applies, or `None` if its context
    /// doesn't hold there. A binding without a context has depth 0. Any
    /// other's depth is the length of the shortest prefix of the stack, from
    /// the outermost frame inward, that satisfies its predicate: the frame
    /// it needs. So in `[workbench, editor]`, `editor` is at depth 2 and
    /// `workbench` at depth 1. (An identifier holds if any frame has it, so
    /// every match holds on the whole stack; the shortest prefix is what
    /// tells them apart.)
    pub fn depth(&self, stack: &ContextStack) -> Option<usize> {
        let Some(predicate) = &self.context else {
            return Some(0);
        };
        if !predicate.eval(stack) {
            return None;
        }
        let frames = stack.frames();
        let depth = (1..=frames.len()).find(|&len| {
            let prefix: ContextStack = frames[..len].iter().cloned().collect();
            predicate.eval(&prefix)
        });
        Some(depth.unwrap_or(0))
    }

    /// What pressing its keys asks for, in `stack`.
    pub fn invocation(&self, stack: &ContextStack) -> Invocation {
        Invocation::new(self.command.clone())
            .args(self.args.clone())
            .source(kiln_command::Source::Keyboard)
            .context(stack.clone())
    }
}

/// A key that goes on from keystrokes held for a chord
/// ([`Keymap::continuations`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Continuation {
    /// The keystroke after the held ones.
    pub next: Keystroke,
    /// What the held keystrokes and `next` run, if a binding ends there.
    pub command: Option<CommandId>,
    /// Whether longer bindings go on past `next`.
    pub longer: bool,
}

/// The effective bindings, in the order they were read: each layer top to
/// bottom, the defaults first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Keymap {
    /// Applies the layers' entries in order, defaults first. An entry with
    /// `command = ""` removes every earlier binding with the same keys and
    /// the same context; others are added. A later binding with the same
    /// keys and context as an earlier one wins over it, but both are kept
    /// for [`Keymap::report`].
    pub fn from_layers<L: Borrow<KeymapLayer>>(layers: impl IntoIterator<Item = L>) -> Self {
        let mut bindings: Vec<Binding> = Vec::new();
        for (layer_ix, layer) in layers.into_iter().enumerate() {
            for entry in &layer.borrow().entries {
                match &entry.command {
                    None => bindings.retain(|binding| {
                        !binding.same_keys_and_context(&entry.keys, &entry.context)
                    }),
                    Some(command) => bindings.push(Binding {
                        keys: entry.keys.clone(),
                        context: entry.context.clone(),
                        command: command.clone(),
                        args: entry.args.clone(),
                        layer: layer_ix,
                        location: entry.location.clone(),
                    }),
                }
            }
        }
        Self { bindings }
    }

    /// Every binding, in the order read, including those a later one
    /// with the same keys and context overrides.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The bindings that run `command`, in the order read, leaving out those
    /// a later binding overrides: for menus and the palette.
    pub fn bindings_for(&self, command: &CommandId) -> Vec<&Binding> {
        self.bindings
            .iter()
            .enumerate()
            .filter(|(ix, binding)| &binding.command == command && !self.is_overridden(*ix))
            .map(|(_, binding)| binding)
            .collect()
    }

    /// The binding that overrides the one at `ix`: the last later one with
    /// the same keys and context.
    pub(crate) fn overrider(&self, ix: usize) -> Option<usize> {
        let binding = &self.bindings[ix];
        (ix + 1..self.bindings.len()).rev().find(|&later| {
            self.bindings[later].same_keys_and_context(&binding.keys, &binding.context)
        })
    }

    pub(crate) fn is_overridden(&self, ix: usize) -> bool {
        self.overrider(ix).is_some()
    }

    /// What `keystroke` pressed alone runs in `stack`: the binding for
    /// exactly that keystroke the [`crate::Matcher`] would dispatch, by its
    /// precedence (the deepest context match, then the later layer, then the
    /// later entry), or `None` if none applies. Longer bindings starting
    /// with it are ignored and no chord is touched: for a click
    /// ([`Key::Click`]), which never starts one. Like a matched key's, the
    /// invocation's source is [`kiln_command::Source::Keyboard`] and its
    /// context `stack`; a host dispatching a click sets the source.
    pub fn lookup(&self, keystroke: &Keystroke, stack: &ContextStack) -> Option<Invocation> {
        self.bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| binding.keys.keystrokes() == [*keystroke])
            .filter_map(|(ix, binding)| Some(((binding.depth(stack)?, ix), binding)))
            .max_by_key(|(rank, _)| *rank)
            .map(|(_, binding)| binding.invocation(stack))
    }

    /// The keys that go on from `held`, the keystrokes of a chord in
    /// progress, in `stack`: one [`Continuation`] per next key of the
    /// bindings whose context holds there and whose keys start with `held`
    /// and are longer, in the order of their first binding. Its command is
    /// the one the [`crate::Matcher`] would dispatch for the keys up to it,
    /// by the same precedence (the deepest context match, then the later
    /// layer, then the later entry).
    pub fn continuations(&self, held: &[Keystroke], stack: &ContextStack) -> Vec<Continuation> {
        // Each next key's continuation, with the rank of the binding that
        // gave its command.
        let mut next: Vec<(Continuation, Option<(usize, usize)>)> = Vec::new();
        for (ix, binding) in self.bindings.iter().enumerate() {
            let keys = binding.keys.keystrokes();
            if keys.len() <= held.len() || !keys.starts_with(held) {
                continue;
            }
            let Some(depth) = binding.depth(stack) else {
                continue;
            };
            let key = keys[held.len()];
            let at = match next
                .iter()
                .position(|(continuation, _)| continuation.next == key)
            {
                Some(at) => at,
                None => {
                    let continuation = Continuation {
                        next: key,
                        command: None,
                        longer: false,
                    };
                    next.push((continuation, None));
                    next.len() - 1
                }
            };
            let (continuation, rank) = &mut next[at];
            if keys.len() > held.len() + 1 {
                continuation.longer = true;
            } else if rank.is_none_or(|best| (depth, ix) > best) {
                *rank = Some((depth, ix));
                continuation.command = Some(binding.command.clone());
            }
        }
        next.into_iter()
            .map(|(continuation, _)| continuation)
            .collect()
    }
}

impl Binding {
    fn same_keys_and_context(
        &self,
        keys: &KeySequence,
        context: &Option<ContextPredicate>,
    ) -> bool {
        &self.keys == keys && &self.context == context
    }
}

/// The commands that exist and the args they take, to check bindings
/// against. [`Registry`] is one.
pub trait CommandCatalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError>;
}

/// Why a [`CommandCatalog`] rejected a binding's command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    UnknownCommand,
    BadArgs(String),
}

/// Checks with [`Registry::validate_args`], whatever context the command
/// applies in.
impl<T: CommandTarget> CommandCatalog for Registry<T> {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        match self.validate_args(command, args) {
            Ok(_) => Ok(()),
            Err(DispatchError::UnknownCommand(_)) => Err(CatalogError::UnknownCommand),
            Err(DispatchError::BadArgs { message, .. }) => Err(CatalogError::BadArgs(message)),
            // `validate_args` checks nothing else.
            Err(
                error @ (DispatchError::NotInContext { .. }
                | DispatchError::Failed { .. }
                | DispatchError::PluginDenied(_)),
            ) => Err(CatalogError::BadArgs(error.to_string())),
        }
    }
}

impl<C: CommandCatalog + ?Sized> CommandCatalog for &C {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        (**self).check(command, args)
    }
}

impl<C: CommandCatalog + ?Sized> CommandCatalog for Box<C> {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        (**self).check(command, args)
    }
}

impl<C: CommandCatalog + ?Sized> CommandCatalog for Rc<C> {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        (**self).check(command, args)
    }
}

impl<C: CommandCatalog + ?Sized> CommandCatalog for Arc<C> {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        (**self).check(command, args)
    }
}

impl KeymapLayer {
    /// Parses a keymap file's `source`, read from `path`. Returns the layer
    /// if it has no errors, and the diagnostics either way.
    ///
    /// Errors, each at the offending token: a syntax error, an entry that
    /// isn't a table or misses `keys` or `command`, a value of the wrong
    /// type, a bad keystroke, a bad context predicate, a malformed command
    /// ID, and, with a `catalog`, args it rejects. A command the catalog
    /// doesn't know is a warning, and only that entry is dropped: it may come
    /// from an extension that isn't loaded yet. So is a `click` anywhere but
    /// in a single keystroke with a modifier, such as `cmd-click`: `click`
    /// alone, `cmd-k click` and `click a` are dropped. Unknown keys are
    /// warnings.
    ///
    /// The keymap is this platform's: see [`KeymapLayer::parse_for`].
    pub fn parse(
        path: &Path,
        source: &str,
        catalog: Option<&dyn CommandCatalog>,
    ) -> (Option<Self>, Vec<Diagnostic>) {
        Self::parse_for(path, source, catalog, Os::current())
    }

    /// [`KeymapLayer::parse`] as on `os`, so one file serves every
    /// platform:
    ///
    /// - `secondary` in keys is `cmd` on macOS and `ctrl` elsewhere;
    /// - `keys` may be a table of keys by platform, `{ macos = "alt-left",
    ///   default = "ctrl-left" }`, its `default` for any not named; with no
    ///   entry for `os`, the binding is left out;
    /// - `platform = "macos"` (or `"!macos"`, or a list such as
    ///   `"linux, windows"`) leaves the binding out elsewhere.
    ///
    /// Every platform's keys and filter are checked, so a mistake in one
    /// shows wherever the file is loaded.
    pub fn parse_for(
        path: &Path,
        source: &str,
        catalog: Option<&dyn CommandCatalog>,
        os: Os,
    ) -> (Option<Self>, Vec<Diagnostic>) {
        let mut layer = Layer::default();
        layer.path = path.to_owned();
        layer.source = source.to_owned();
        let mut parser = EntryParser {
            layer,
            catalog,
            os,
            diagnostics: Vec::new(),
        };
        let entries = parser.parse();
        let EntryParser {
            layer, diagnostics, ..
        } = parser;
        let ok = !diagnostics.iter().any(Diagnostic::is_error);
        let parsed = ok.then_some(KeymapLayer {
            path: layer.path,
            entries,
        });
        (parsed, diagnostics)
    }
}

/// Whether `keys` either has no click or is one keystroke of a click with
/// at least one modifier.
fn click_is_bindable(keys: &KeySequence) -> bool {
    match keys.keystrokes() {
        [keystroke] if keystroke.key == Key::Click => keystroke.modifiers != Default::default(),
        keystrokes => keystrokes
            .iter()
            .all(|keystroke| keystroke.key != Key::Click),
    }
}

struct EntryParser<'a> {
    /// The file's path and source, for diagnostics.
    layer: Layer,
    catalog: Option<&'a dyn CommandCatalog>,
    /// The platform the keymap is for.
    os: Os,
    diagnostics: Vec<Diagnostic>,
}

/// One `[[bindings]]` entry's fields and their spans.
struct Fields<'a, 'i> {
    table: &'a DeTable<'i>,
    values: &'a toml::Table,
    span: Range<usize>,
}

impl EntryParser<'_> {
    fn parse(&mut self) -> Vec<Entry> {
        let source = self.layer.source.clone();
        let document = match DeTable::parse(&source) {
            Ok(document) => document,
            Err(error) => {
                let offset = error.span().map_or(0, |span| span.start);
                self.error(None, offset, error.message().trim_end().to_owned());
                return Vec::new();
            }
        };
        let values: toml::Table = match source.parse() {
            Ok(values) => values,
            // Such as an integer too big for an i64.
            Err(error) => {
                let offset = error.span().map_or(0, |span| span.start);
                self.error(None, offset, error.message().trim_end().to_owned());
                return Vec::new();
            }
        };
        let document = document.get_ref();
        self.unknown_keys(document, &["bindings"], "");
        let Some(bindings) = document.get("bindings") else {
            return Vec::new();
        };
        let (DeValue::Array(items), Some(toml::Value::Array(item_values))) =
            (bindings.get_ref(), values.get("bindings"))
        else {
            let message = tr!(
                "kiln-core-config-expected-tables",
                found = bindings.get_ref().type_str()
            )
            .into();
            self.error(Some("bindings"), bindings.span().start, message);
            return Vec::new();
        };
        let mut entries = Vec::new();
        for (ix, (item, item_values)) in items.iter().zip(item_values).enumerate() {
            let key = format!("bindings[{ix}]");
            match (item.get_ref(), item_values) {
                (DeValue::Table(table), toml::Value::Table(values)) => {
                    let fields = Fields {
                        table,
                        values,
                        span: item.span(),
                    };
                    entries.extend(self.entry(&key, &fields));
                }
                _ => {
                    let message = tr!(
                        "kiln-core-config-expected-table",
                        found = item.get_ref().type_str()
                    )
                    .into();
                    self.error(Some(&key), item.span().start, message);
                }
            }
        }
        entries
    }

    /// The entry, or `None` if it has errors or is dropped.
    fn entry(&mut self, key: &str, fields: &Fields<'_, '_>) -> Option<Entry> {
        self.unknown_keys(
            fields.table,
            &["keys", "command", "context", "args", "platform"],
            key,
        );
        let errors = self.error_count();
        let included = self.platform(key, fields);
        let keys = self.keys(key, fields);
        let command = self
            .string(key, fields, "command", true)
            .and_then(|(text, span)| {
                if text.is_empty() {
                    return Some(None);
                }
                CommandId::parse(&text)
                    .map_err(|error| {
                        let message = format!("{}: {}", error.reason, error.id);
                        self.error(Some(&format!("{key}.command")), span.start, message);
                    })
                    .ok()
                    .map(Some)
            });
        let context = match self.string(key, fields, "context", false) {
            None => None,
            Some((text, span)) => match ContextPredicate::parse(&text) {
                Ok(predicate) => Some(predicate),
                Err(error) => {
                    let offset = self.inner_offset(&span, &text, error.offset);
                    self.error(Some(&format!("{key}.context")), offset, error.to_string());
                    None
                }
            },
        };
        let args = match (fields.table.get("args"), fields.values.get("args")) {
            (Some(spanned), Some(value)) => match value {
                toml::Value::Table(_) => match serde_json::to_value(value) {
                    Ok(args) => Some((args, spanned.span())),
                    Err(error) => {
                        self.error(
                            Some(&format!("{key}.args")),
                            spanned.span().start,
                            error.to_string(),
                        );
                        None
                    }
                },
                value => {
                    let message =
                        tr!("kiln-core-config-expected-table", found = value.type_str()).into();
                    self.error(Some(&format!("{key}.args")), spanned.span().start, message);
                    None
                }
            },
            _ => None,
        };
        if self.error_count() > errors {
            return None;
        }
        // Left out here, though checked for every platform.
        if !included? {
            return None;
        }
        let (keys, command) = (keys??, command?);
        if !click_is_bindable(&keys) {
            let offset = fields
                .table
                .get("keys")
                .map_or(fields.span.start, |value| value.span().start);
            let message = tr!("kiln-core-keymap-click-alone", keys = keys.to_string()).into();
            self.push(
                Severity::Warning,
                Some(&format!("{key}.keys")),
                offset,
                message,
            );
            return None;
        }
        let (args, args_span) = match args {
            Some((args, span)) => (args, Some(span)),
            None => (Value::Null, None),
        };
        if let (Some(command), Some(catalog)) = (&command, self.catalog) {
            let command_span = fields.table.get("command").map(|value| value.span());
            match catalog.check(command, &args) {
                Ok(()) => {}
                Err(CatalogError::UnknownCommand) => {
                    let offset = command_span.map_or(fields.span.start, |span| span.start);
                    let message = tr!(
                        "kiln-core-keymap-unknown-command",
                        command = command.to_string()
                    )
                    .into();
                    self.push(
                        Severity::Warning,
                        Some(&format!("{key}.command")),
                        offset,
                        message,
                    );
                    return None;
                }
                Err(CatalogError::BadArgs(message)) => {
                    let (field, span) = match args_span {
                        Some(span) => ("args", Some(span)),
                        None => ("command", command_span),
                    };
                    let offset = span.map_or(fields.span.start, |span| span.start);
                    let message = tr!(
                        "kiln-core-keymap-bad-args",
                        command = command.to_string(),
                        reason = message
                    )
                    .into();
                    self.error(Some(&format!("{key}.{field}")), offset, message);
                    return None;
                }
            }
        }
        Some(Entry {
            keys,
            context,
            command,
            args,
            location: self.location(fields.span.start),
        })
    }

    /// Whether the entry's `platform` filter includes this keymap's
    /// platform: `Some(true)` with none, `None` if it's malformed.
    fn platform(&mut self, key: &str, fields: &Fields<'_, '_>) -> Option<bool> {
        let Some((text, span)) = self.string(key, fields, "platform", false) else {
            return Some(fields.table.get("platform").is_none());
        };
        match PlatformFilter::parse(&text) {
            Ok(filter) => Some(filter.matches(self.os)),
            Err(message) => {
                self.error(Some(&format!("{key}.platform")), span.start, message);
                None
            }
        }
    }

    /// The entry's keys on this platform: `Some(None)` if it has none here,
    /// `None` if any platform's are malformed.
    fn keys(&mut self, key: &str, fields: &Fields<'_, '_>) -> Option<Option<KeySequence>> {
        let field = format!("{key}.keys");
        let (Some(spanned), Some(value)) = (fields.table.get("keys"), fields.values.get("keys"))
        else {
            let message = tr!("kiln-core-config-missing", name = "keys").into();
            self.error(Some(key), fields.span.start, message);
            return None;
        };
        // Each platform's keys, parsed as on that platform.
        let variants: Vec<(String, String, Range<usize>)> = match (spanned.get_ref(), value) {
            (_, toml::Value::String(text)) => {
                vec![("".into(), text.clone(), spanned.span())]
            }
            (DeValue::Table(table), toml::Value::Table(values)) => {
                let mut variants = Vec::new();
                for (name, value) in values {
                    let span = table
                        .get(name.as_str())
                        .map_or(spanned.span(), |value| value.span());
                    if name != "default" && Os::from_name(name).is_none() {
                        let quoted = format!("{name:?}");
                        let message =
                            tr!("kiln-core-keymap-platform-unknown", name = quoted).into();
                        self.error(Some(&format!("{field}.{name}")), span.start, message);
                        continue;
                    }
                    match value {
                        toml::Value::String(text) => {
                            variants.push((name.clone(), text.clone(), span));
                        }
                        value => {
                            let message =
                                tr!("kiln-core-config-expected-string", found = value.type_str())
                                    .into();
                            self.error(Some(&format!("{field}.{name}")), span.start, message);
                        }
                    }
                }
                variants
            }
            (_, value) => {
                let message =
                    tr!("kiln-core-keymap-expected-keys", found = value.type_str()).into();
                self.error(Some(&field), spanned.span().start, message);
                return None;
            }
        };
        let errors = self.error_count();
        let mut here = None;
        let mut fallback = None;
        for (name, text, span) in variants {
            // A shared or default entry is checked as on this platform.
            let os = Os::from_name(&name).unwrap_or(self.os);
            match KeySequence::parse_for(&text, os) {
                Ok(keys) if name.is_empty() || os == self.os && name != "default" => {
                    here = Some(keys);
                }
                Ok(keys) if name == "default" => fallback = Some(keys),
                Ok(_) => {}
                Err(error) => {
                    let offset = self.inner_offset(&span, &text, error.offset);
                    self.error(Some(&field), offset, error.message);
                }
            }
        }
        (self.error_count() == errors).then(|| here.or(fallback))
    }

    /// The string `name` of the entry and its span. Missing, it's an error
    /// if `required`; of another type, it's an error.
    fn string(
        &mut self,
        key: &str,
        fields: &Fields<'_, '_>,
        name: &str,
        required: bool,
    ) -> Option<(String, Range<usize>)> {
        let Some(spanned) = fields.table.get(name) else {
            if required {
                let message = tr!("kiln-core-config-missing", name = name).into();
                self.error(Some(key), fields.span.start, message);
            }
            return None;
        };
        match fields.values.get(name) {
            Some(toml::Value::String(text)) => Some((text.clone(), spanned.span())),
            value => {
                let found = value.map_or_else(
                    || tr!("kiln-core-config-found-nothing"),
                    |value| value.type_str().into(),
                );
                let message = tr!("kiln-core-config-expected-string", found = found).into();
                self.error(
                    Some(&format!("{key}.{name}")),
                    spanned.span().start,
                    message,
                );
                None
            }
        }
    }

    /// Warns about each key of `table` not in `known`, at the key.
    fn unknown_keys(&mut self, table: &DeTable<'_>, known: &[&str], prefix: &str) {
        let mut unknown: Vec<_> = table
            .iter()
            .filter(|(key, _)| !known.contains(&key.get_ref().as_ref()))
            .map(|(key, _)| (key.span().start, key.get_ref().to_string()))
            .collect();
        unknown.sort();
        for (offset, name) in unknown {
            let key = match prefix {
                "" => name,
                prefix => format!("{prefix}.{name}"),
            };
            self.push(
                Severity::Warning,
                Some(&key),
                offset,
                tr!("kiln-core-config-unknown-key").into(),
            );
        }
    }

    /// The offset in the file of byte `offset` of the string `text`, whose
    /// value is at `span`: inside the quotes if the string is written
    /// without escapes, else the value's start.
    fn inner_offset(&self, span: &Range<usize>, text: &str, offset: usize) -> usize {
        let written = &self.layer.source[span.clone()];
        let plain = ['"', '\''].iter().any(|&quote| {
            written.len() == text.len() + 2
                && written.starts_with(quote)
                && written.ends_with(quote)
                && &written[1..written.len() - 1] == text
        });
        if plain {
            span.start + 1 + offset
        } else {
            span.start
        }
    }

    fn location(&self, offset: usize) -> Location {
        let at = self
            .layer
            .diagnostic_at(Severity::Warning, None, offset, String::new());
        Location {
            file: at.file,
            line: at.line,
            column: at.column,
        }
    }

    fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }

    fn error(&mut self, key: Option<&str>, offset: usize, message: String) {
        self.push(Severity::Error, key, offset, message);
    }

    fn push(&mut self, severity: Severity, key: Option<&str>, offset: usize, message: String) {
        let diagnostic =
            self.layer
                .diagnostic_at(severity, key.map(str::to_owned), offset, message);
        self.diagnostics.push(diagnostic);
    }
}
