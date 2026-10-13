// SPDX-License-Identifier: LicenseRef-Proprietary

//! Loading one TOML file as a config layer, checking it against a JSON
//! schema, and merging layers. Nothing here knows about settings, so the
//! keymap can load the same way.

use std::{
    collections::BTreeMap,
    io::{self, Read as _},
    ops::Range,
    path::{Path, PathBuf},
};

use serde::Deserialize as _;
use serde_json::Value as Json;
use toml::{
    Table,
    de::{DeTable, DeValue},
};

use crate::diagnostic::{Diagnostic, LineIndex, Severity};

/// One config file: its text, its parsed table, and what's wrong with it.
#[derive(Clone, Debug, Default)]
pub struct Layer {
    pub path: PathBuf,
    /// The file's text; empty for a missing or unreadable file.
    pub source: String,
    /// The parsed file; empty if it's missing or has a syntax error.
    pub table: Table,
    /// Syntax errors from loading, and whatever later checks add.
    pub diagnostics: Vec<Diagnostic>,
    /// Where each key and its value are in `source`, by key path. Arrays
    /// aren't descended into.
    spans: BTreeMap<Vec<String>, KeySpans>,
}

#[derive(Clone, Debug)]
struct KeySpans {
    key: Range<usize>,
    value: Range<usize>,
}

/// Reads and parses the TOML file at `path`. A missing file is an empty
/// layer with no diagnostics; an unreadable file or a syntax error is an
/// empty layer with an error diagnostic.
pub fn load_layer(path: &Path) -> Layer {
    let mut layer = Layer {
        path: path.to_owned(),
        ..Layer::default()
    };
    match read(path) {
        Ok(Some(source)) => parse_layer(path, source),
        Ok(None) => layer,
        Err(error) => {
            let message = format!("cannot read the file: {error}");
            layer
                .diagnostics
                .push(layer.diagnostic_at(Severity::Error, None, 0, message));
            layer
        }
    }
}

/// Parses `source` as the TOML file at `path`, as [`load_layer`] does once
/// it has read the file: a syntax error is an empty layer with an error
/// diagnostic.
pub fn parse_layer(path: &Path, source: String) -> Layer {
    let mut layer = Layer {
        path: path.to_owned(),
        source,
        ..Layer::default()
    };
    let document = match DeTable::parse(&layer.source) {
        Ok(document) => document,
        Err(error) => {
            let offset = error.span().map_or(0, |span| span.start);
            let message = error.message().trim_end().to_owned();
            layer
                .diagnostics
                .push(layer.diagnostic_at(Severity::Error, None, offset, message));
            return layer;
        }
    };
    index_spans(document.get_ref(), &mut Vec::new(), &mut layer.spans);
    match Table::deserialize(toml::de::Deserializer::from(document)) {
        Ok(table) => layer.table = table,
        // Such as an integer too big for an i64.
        Err(error) => {
            let diagnostic = layer.diagnostic_for(Severity::Error, &error);
            layer.diagnostics.push(diagnostic);
        }
    }
    layer
}

/// Config files bigger than this are rejected rather than read.
const MAX_BYTES: u64 = 1 << 20;

/// The text of the file at `path`, or `None` if there's none. Only regular
/// files of at most [`MAX_BYTES`] are read: a project's config file is
/// untrusted. It's opened without blocking, as opening a FIFO would block,
/// and checked once open, so nothing can be swapped in between.
fn read(path: &Path) -> io::Result<Option<String>> {
    let file = match crate::platform::fs::open_for_reading(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let mut source = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut source)?;
    if source.len() as u64 > MAX_BYTES {
        return Err(io::Error::other(format!(
            "bigger than {} KiB",
            MAX_BYTES / 1024
        )));
    }
    Ok(Some(source))
}

fn index_spans(
    table: &DeTable<'_>,
    path: &mut Vec<String>,
    spans: &mut BTreeMap<Vec<String>, KeySpans>,
) {
    for (key, value) in table.iter() {
        path.push(key.get_ref().to_string());
        spans.insert(
            path.clone(),
            KeySpans {
                key: key.span(),
                value: value.span(),
            },
        );
        if let DeValue::Table(table) = value.get_ref() {
            index_spans(table, path, spans);
        }
        path.pop();
    }
}

impl Layer {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// A diagnostic at byte `offset` of the source.
    pub fn diagnostic_at(
        &self,
        severity: Severity,
        key: Option<String>,
        offset: usize,
        message: String,
    ) -> Diagnostic {
        self.diagnostic_indexed(
            &LineIndex::new(&self.source),
            severity,
            key,
            offset,
            message,
        )
    }

    /// `diagnostic_at`, with the source's line index already built.
    fn diagnostic_indexed(
        &self,
        index: &LineIndex,
        severity: Severity,
        key: Option<String>,
        offset: usize,
        message: String,
    ) -> Diagnostic {
        let (line, column) = index.line_column(&self.source, offset);
        Diagnostic {
            severity,
            file: self.path.clone(),
            key,
            line,
            column,
            message,
        }
    }

    /// A diagnostic on the key at `path`, such as `["editor", "font_size"]`,
    /// pointing at the key itself.
    pub fn key_diagnostic(
        &self,
        severity: Severity,
        path: &[String],
        message: String,
    ) -> Diagnostic {
        let offset = self.spans.get(path).map_or(0, |spans| spans.key.start);
        self.diagnostic_at(severity, Some(path.join(".")), offset, message)
    }

    /// A diagnostic on the key at `path`, pointing at its value.
    pub fn value_diagnostic(
        &self,
        severity: Severity,
        path: &[String],
        message: String,
    ) -> Diagnostic {
        let offset = self.spans.get(path).map_or(0, |spans| spans.value.start);
        self.diagnostic_at(severity, Some(path.join(".")), offset, message)
    }

    /// The byte offset of the key at `path` in the source, if it's there.
    pub fn key_offset(&self, path: &[String]) -> Option<usize> {
        self.spans.get(path).map(|spans| spans.key.start)
    }

    /// A diagnostic for an error a deserializer reported in this layer's
    /// source, naming the innermost key whose value holds the error.
    pub fn diagnostic_for(&self, severity: Severity, error: &toml::de::Error) -> Diagnostic {
        let offset = error.span().map_or(0, |span| span.start);
        let key = self
            .spans
            .iter()
            .filter(|(_, spans)| spans.value.contains(&offset))
            .max_by_key(|(path, _)| path.len())
            .map(|(path, _)| path.join("."));
        self.diagnostic_at(severity, key, offset, error.message().trim_end().to_owned())
    }

    /// Checks the table against a JSON `schema` (as schemars writes them):
    /// keys the schema doesn't list are warnings at the key, and values not
    /// of the listed `type`, numbers outside `minimum`/`maximum` and strings
    /// shorter than `minLength` are errors at the value.
    pub fn check_schema(&self, schema: &Json) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let index = LineIndex::new(&self.source);
        self.check_table(
            &index,
            &self.table,
            schema,
            schema,
            &mut Vec::new(),
            &mut diagnostics,
        );
        diagnostics
    }

    fn check_table(
        &self,
        index: &LineIndex,
        table: &Table,
        node: &Json,
        root: &Json,
        path: &mut Vec<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let node = resolve(node, root);
        let Some(properties) = node.get("properties").and_then(Json::as_object) else {
            return;
        };
        for (key, value) in table {
            path.push(key.clone());
            match properties.get(key) {
                Some(property) => {
                    let property = resolve(property, root);
                    let problem = wrong_type(value, property);
                    if problem.is_none()
                        && let toml::Value::Table(table) = value
                    {
                        self.check_table(index, table, property, root, path, diagnostics);
                    } else if let Some(message) = problem.or_else(|| out_of_range(value, property))
                    {
                        let offset = self
                            .spans
                            .get(&path[..])
                            .map_or(0, |spans| spans.value.start);
                        diagnostics.push(self.diagnostic_indexed(
                            index,
                            Severity::Error,
                            Some(path.join(".")),
                            offset,
                            message,
                        ));
                    }
                }
                None => {
                    let offset = self.spans.get(&path[..]).map_or(0, |spans| spans.key.start);
                    diagnostics.push(self.diagnostic_indexed(
                        index,
                        Severity::Warning,
                        Some(path.join(".")),
                        offset,
                        "unknown key, ignored".into(),
                    ));
                }
            }
            path.pop();
        }
    }
}

/// `node`, or the `#/$defs/…` definition it refers to.
fn resolve<'a>(node: &'a Json, root: &'a Json) -> &'a Json {
    node.get("$ref")
        .and_then(Json::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| root.get("$defs")?.get(name))
        .unwrap_or(node)
}

/// Why `value` isn't of a type `schema` lists, if it isn't. Serde would
/// take an array for a struct, for one.
fn wrong_type(value: &toml::Value, schema: &Json) -> Option<String> {
    let types: Vec<&str> = match schema.get("type")? {
        Json::String(name) => vec![name],
        Json::Array(names) => names.iter().filter_map(Json::as_str).collect(),
        _ => return None,
    };
    let fits = |name: &str| match name {
        "object" => value.is_table(),
        "array" => value.is_array(),
        "string" => value.is_str(),
        "integer" => value.is_integer(),
        "number" => value.is_integer() || value.is_float(),
        "boolean" => value.is_bool(),
        // TOML has no null.
        "null" => false,
        _ => true,
    };
    if types.iter().any(|name| fits(name)) {
        return None;
    }
    let expected: Vec<&str> = types
        .iter()
        .filter_map(|name| match *name {
            "object" => Some("a table"),
            "array" => Some("an array"),
            "string" => Some("a string"),
            "integer" => Some("an integer"),
            "number" => Some("a number"),
            "boolean" => Some("a boolean"),
            _ => None,
        })
        .collect();
    Some(format!(
        "expected {}, found {}",
        expected.join(" or "),
        value.type_str()
    ))
}

/// Why `value` breaks `schema`'s bounds, if it does.
fn out_of_range(value: &toml::Value, schema: &Json) -> Option<String> {
    let bound = |name| schema.get(name).and_then(Json::as_f64);
    match value {
        toml::Value::Integer(_) | toml::Value::Float(_) => {
            let number = value
                .as_float()
                .or_else(|| value.as_integer().map(|n| n as f64))?;
            let (min, max) = (bound("minimum"), bound("maximum"));
            // Written so NaN is out of every range.
            let fits = min.is_none_or(|min| number >= min) && max.is_none_or(|max| number <= max);
            let show = |name: &str| schema[name].to_string();
            (!fits).then(|| match (min, max) {
                (Some(_), Some(_)) => format!(
                    "{value} is out of range: must be from {} to {}",
                    show("minimum"),
                    show("maximum")
                ),
                (Some(_), None) => {
                    format!("{value} is too small: must be at least {}", show("minimum"))
                }
                _ => format!("{value} is too large: must be at most {}", show("maximum")),
            })
        }
        toml::Value::String(string) => {
            let min = bound("minLength")?;
            match string.chars().count() as f64 >= min {
                true => None,
                false if min <= 1.0 => Some("must not be empty".into()),
                false => Some(format!("must be at least {min} characters long")),
            }
        }
        _ => None,
    }
}

/// Merges `over` into `base` key by key: tables in both are merged, and any
/// other value in `over` replaces `base`'s.
pub fn merge(base: &mut Table, over: &Table) {
    for (key, value) in over {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(base)), toml::Value::Table(over)) => merge(base, over),
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}
