//! Writes the user guide's reference pages, `docs/user/src/reference/`, from
//! the code they describe, so they can't drift from it:
//!
//! - `settings.md`, every key of `settings.toml` with its type, default,
//!   values and doc comment, by table, from [`fe_config::settings_schema`];
//! - `keyboard-shortcuts.md`, every binding of the built-in keymap
//!   ([`fe_keymap::DEFAULT`]) with its keys on each platform and its
//!   context;
//! - `commands.md`, every command [`fe_workbench::command_catalog`] knows,
//!   with its title and its keys.
//!
//! ```text
//! cargo run -p fastedit --example gen_user_docs              # rewrites the stale pages
//! cargo run -p fastedit --example gen_user_docs -- --check   # writes nothing; exits 1 if one is stale
//! ```
//!
//! `crates/fastedit/tests/user_docs_fresh.rs` runs the check in `cargo
//! test`. The text is the same on every host: each platform's bindings are
//! resolved by name ([`KeymapLayer::parse_for`]), never as the host's;
//! titles and key names are en-US; rows are sorted; lines end in LF; and no
//! path or time is written.
//!
//! The command catalog only checks IDs; it can't list them. So the commands
//! are what it's built from, listed with no app as its own tests build it:
//! the `#[kiln::command]`s `inventory` recorded for
//! [`fe_workbench::RECORDED_COMMAND_CRATES`], and
//! [`fe_workbench::registry_commands`], every registry's.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Path, PathBuf},
    process::ExitCode,
};

use fe_command::{CommandId, Value};
use fe_gpui::fe_keymap::{self, DEFAULT_KEYMAP_PATH, Keymap, KeymapLayer, macos_label, pc_label};
use kiln::__private::{CommandRecord, inventory};
use kiln::core::Os;

/// The command that rewrites the pages.
pub const REGENERATE: &str = "cargo run -p fastedit --example gen_user_docs";

/// Where the pages go, from the repository's root.
pub const DIR: &str = "docs/user/src/reference";

/// The pages' file names, under [`DIR`].
pub const SETTINGS: &str = "settings.md";
pub const SHORTCUTS: &str = "keyboard-shortcuts.md";
pub const COMMANDS: &str = "commands.md";

/// The line every page carries under its H1.
pub const GENERATED: &str = "Generated; do not edit.";

const USAGE: &str = "usage: gen_user_docs [--check]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&args, &repo_root())
}

/// Runs the command line `args` on the checkout at `root`.
pub fn run(args: &[&str], root: &Path) -> ExitCode {
    match args {
        [] => match write(root) {
            Ok(written) if written.is_empty() => {
                println!("gen_user_docs: the reference pages are up to date");
                ExitCode::SUCCESS
            }
            Ok(written) => {
                for file in written {
                    println!("gen_user_docs: wrote {DIR}/{file}");
                }
                ExitCode::SUCCESS
            }
            Err(why) => {
                eprintln!("gen_user_docs: {why}");
                ExitCode::FAILURE
            }
        },
        ["--check"] => {
            let stale = stale(root);
            if stale.is_empty() {
                println!("gen_user_docs: the reference pages are up to date");
                return ExitCode::SUCCESS;
            }
            for summary in &stale {
                eprintln!("{summary}");
            }
            eprintln!("gen_user_docs: run `{REGENERATE}`");
            ExitCode::FAILURE
        }
        ["-h" | "--help"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The repository's root, from this package's manifest directory.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Each page's file name, under [`DIR`], and its text.
pub fn pages() -> Vec<(&'static str, String)> {
    english();
    let keymaps = Keymaps::resolve(&[
        (Os::MacOs, fe_keymap::DEFAULT),
        (Os::Linux, fe_keymap::DEFAULT),
        (Os::Windows, fe_keymap::DEFAULT),
    ]);
    let commands = commands();
    vec![
        (SETTINGS, settings_page(&fe_config::settings_schema())),
        (SHORTCUTS, shortcuts_page(&keymaps, &commands)),
        (COMMANDS, commands_page(&commands, &keymaps)),
    ]
}

/// Writes the pages under `root` whose text differs; returns their names.
pub fn write(root: &Path) -> Result<Vec<&'static str>, String> {
    let dir = root.join(DIR);
    let mut written = Vec::new();
    for (file, text) in pages() {
        let path = dir.join(file);
        if std::fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
            continue;
        }
        std::fs::write(&path, text).map_err(|error| format!("{DIR}/{file}: {error}"))?;
        written.push(file);
    }
    Ok(written)
}

/// What [`write`] would change under `root`, one summary a stale page;
/// none when every page is up to date. Writes nothing.
pub fn stale(root: &Path) -> Vec<String> {
    let dir = root.join(DIR);
    let mut stale = Vec::new();
    for (file, text) in pages() {
        match std::fs::read_to_string(dir.join(file)) {
            Ok(found) if found == text => {}
            Ok(found) => stale.push(format!("{DIR}/{file} is stale:\n{}", diff(&found, &text))),
            Err(error) => stale.push(format!("{DIR}/{file} can't be read: {error}")),
        }
    }
    stale
}

/// The lines only `found` has (`-`) and only `wanted` has (`+`), at most
/// ten of each, by line number.
fn diff(found: &str, wanted: &str) -> String {
    const SHOWN: usize = 10;
    let count = |text: &str| {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for line in text.lines() {
            *counts.entry(line.to_owned()).or_default() += 1;
        }
        counts
    };
    // A line is extra on one side as often as it's there more times.
    let only = |text: &str, other: &BTreeMap<String, usize>| {
        let mut left = other.clone();
        let mut lines = Vec::new();
        for (ix, line) in text.lines().enumerate() {
            match left.get_mut(line) {
                Some(n) if *n > 0 => *n -= 1,
                _ => lines.push((ix + 1, line.to_owned())),
            }
        }
        lines
    };
    let removed = only(found, &count(wanted));
    let added = only(wanted, &count(found));
    let mut summary = String::new();
    for (sign, lines) in [('-', &removed), ('+', &added)] {
        for (line, text) in lines.iter().take(SHOWN) {
            let _ = writeln!(summary, "  {sign} {line}: {text}");
        }
        if lines.len() > SHOWN {
            let _ = writeln!(summary, "  {sign} ... and {} more", lines.len() - SHOWN);
        }
    }
    if removed.is_empty() && added.is_empty() {
        // The same lines in another order, or other line endings.
        summary.push_str("  the lines differ in order or line endings\n");
    }
    let lines = |n: usize| {
        if n == 1 {
            "1 line".to_owned()
        } else {
            format!("{n} lines")
        }
    };
    let _ = write!(
        summary,
        "  ({} removed, {} added)",
        lines(removed.len()),
        lines(added.len())
    );
    summary
}

/// Titles and key names in en-US, whatever the host's locale.
fn english() {
    let en_us = "en-US".parse().expect("en-US is a language tag");
    kiln_l10n::set_locales(&[en_us]);
    fe_command::set_label_arg("app", fe_config::APP_NAME);
    fe_keymap::menu::set_label_arg("app", fe_config::APP_NAME);
}

/// A page's H1 and the line saying it's generated.
fn header(title: &str, from: &str) -> String {
    format!("# {title}\n\n> {GENERATED} `{REGENERATE}` writes this page from {from}.\n")
}

// --- Settings ---------------------------------------------------------

/// The settings reference: a section a table, depth first, then a
/// heading a key, with its type, default, values and doc comment.
pub fn settings_page(schema: &Value) -> String {
    let mut page = header(
        "Settings reference",
        "the settings schema (`fe_config::settings_schema()`)",
    );
    page.push_str(
        "\nEvery key `settings.toml` takes, by table: its type, its default, the values it \
         takes and what it does. `<name>` stands for a name you choose, such as a \
         language server's.\n",
    );
    let schema = Schema(schema);
    let mut tables = Vec::new();
    schema.table(
        String::new(),
        TableKind::Plain,
        schema.0,
        None,
        None,
        &mut tables,
    );
    for table in tables {
        page.push('\n');
        if table.path.is_empty() {
            page.push_str("## Top-level keys\n");
        } else {
            let _ = writeln!(page, "## `{}`", table.path);
        }
        if let Some(description) = &table.description {
            page.push('\n');
            page.push_str(&markdown(description));
        }
        match table.kind {
            TableKind::Plain => {}
            TableKind::Map => {
                let parent = table.path.trim_end_matches(".<name>");
                let _ = writeln!(page, "\nA table of these tables, `[{parent}.<name>]`.");
            }
            TableKind::List => {
                let _ = writeln!(page, "\nA list of these tables, `[[{}]]`.", table.path);
            }
        }
        match &table.default {
            Some(default) if default == UNSET || default == UNSET_IMPLICIT => {
                let _ = writeln!(page, "\nDefault: {default}");
            }
            Some(default) => {
                let _ = writeln!(page, "\nDefault: {}", code(default));
            }
            None => {}
        }
        for key in &table.keys {
            let _ = writeln!(page, "\n### `{}`\n", key.path);
            if let Some(description) = &key.description {
                page.push_str(&markdown(description));
                page.push('\n');
            }
            let _ = writeln!(page, "- Type: {}", key.kind);
            match &key.default {
                Some(default) if default == UNSET || default == UNSET_IMPLICIT => {
                    let _ = writeln!(page, "- Default: {default}");
                }
                Some(default) => {
                    let _ = writeln!(page, "- Default: {}", code(default));
                }
                None if key.required => page.push_str("- Required\n"),
                None => {}
            }
            if !key.values.is_empty() {
                page.push_str("- Values:\n");
                for (value, description) in &key.values {
                    let _ = write!(page, "  - {}", code(value));
                    if let Some(description) = description {
                        let _ = write!(page, ": {}", markdown_inline(description));
                    }
                    page.push('\n');
                }
            }
        }
    }
    page
}

/// An explicit null default: the key is unset.
const UNSET: &str = "unset";

/// No default on a key that may be null (an `Option`): unset too, but the
/// schema says so by leaving the default out.
const UNSET_IMPLICIT: &str = "unset (the schema gives no default)";

/// What a table section holds.
#[derive(Clone, Copy, PartialEq)]
enum TableKind {
    /// A table of keys: `[editor]`.
    Plain,
    /// One of a table of tables by name: `[lsp.servers.<name>]`.
    Map,
    /// One of a list of tables: `[[vim.remap]]`.
    List,
}

struct Table {
    /// Dotted, with `<name>` for a map's key; empty for the top level.
    path: String,
    kind: TableKind,
    description: Option<String>,
    /// For a map or a list: the whole thing's default.
    default: Option<String>,
    keys: Vec<Key>,
}

struct Key {
    path: String,
    description: Option<String>,
    kind: String,
    default: Option<String>,
    required: bool,
    /// An enum's values, as TOML, with their doc comments.
    values: Vec<(String, Option<String>)>,
}

/// A table inside the one [`Schema::table`] is adding, to add after it.
struct Nested<'a> {
    path: String,
    kind: TableKind,
    node: &'a Value,
    description: Option<String>,
    default: Option<String>,
    inherited: Option<&'a Value>,
}

/// The schema, to resolve `$ref`s against.
struct Schema<'a>(&'a Value);

impl<'a> Schema<'a> {
    /// `node` with its `$ref` followed, as often as it has one.
    fn resolve(&self, mut node: &'a Value) -> &'a Value {
        while let Some(reference) = node.get("$ref").and_then(Value::as_str) {
            let name = reference
                .strip_prefix("#/$defs/")
                .unwrap_or_else(|| panic!("the settings schema refers outside $defs: {reference}"));
            node = self
                .0
                .pointer(&format!("/$defs/{name}"))
                .unwrap_or_else(|| panic!("the settings schema has no {reference}"));
        }
        node
    }

    /// `node`, or the one an `Option`'s `anyOf: [T, null]` holds.
    fn unwrap_null(&self, node: &'a Value) -> &'a Value {
        let resolved = self.resolve(node);
        match resolved
            .get("anyOf")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
        {
            Some([one, null]) if is_null(null) => self.resolve(one),
            Some([null, one]) if is_null(null) => self.resolve(one),
            _ => resolved,
        }
    }

    /// `default` as the page prints it: TOML, empty tables and lists too;
    /// [`UNSET`] for null, [`UNSET_IMPLICIT`] for none on an `Option`;
    /// `None` for none otherwise. Each reads differently, so a change from
    /// one to another changes the page.
    fn default_text(&self, default: Option<&Value>, node: &'a Value) -> Option<String> {
        match default {
            Some(Value::Null) => Some(UNSET.to_owned()),
            Some(value) => Some(toml(value)),
            None if self.nullable(node) => Some(UNSET_IMPLICIT.to_owned()),
            None => None,
        }
    }

    /// Whether `node` may be null: an `Option`.
    fn nullable(&self, node: &'a Value) -> bool {
        let node = self.resolve(node);
        let typed_null = match node.get("type") {
            Some(Value::Array(names)) => names.iter().any(|name| name == "null"),
            _ => false,
        };
        let any_null = (node.get("anyOf").and_then(Value::as_array))
            .is_some_and(|alternatives| alternatives.iter().any(is_null));
        // `Option<serde_json::Value>`: anything, null too.
        let unconstrained = node.as_object().is_some_and(|node| {
            node.keys()
                .all(|key| ["description", "default", "title"].contains(&key.as_str()))
        });
        typed_null || any_null || unconstrained
    }

    /// The table of keys `node` describes, if it's one: one with
    /// `properties`.
    fn struct_of(&self, node: &'a Value) -> Option<&'a Value> {
        let node = self.unwrap_null(node);
        node.get("properties").is_some().then_some(node)
    }

    /// Adds the table at `path` that `node` describes, then its own
    /// tables, to `tables`, keys and tables sorted by name. `inherited` is
    /// the table's default as the tables holding it give it, which wins
    /// over its keys' own: `editor`'s default for `editor.completion`.
    fn table(
        &self,
        path: String,
        kind: TableKind,
        node: &'a Value,
        default: Option<String>,
        inherited: Option<&'a Value>,
        tables: &mut Vec<Table>,
    ) {
        let node = self.unwrap_null(node);
        let properties = node
            .get("properties")
            .and_then(Value::as_object)
            .expect("a table has properties");
        let required: BTreeSet<&str> = (node.get("required").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let at = tables.len();
        tables.push(Table {
            path: path.clone(),
            kind,
            description: None,
            default,
            keys: Vec::new(),
        });
        let mut keys = Vec::new();
        let mut nested = Vec::new();
        let mut names: Vec<&String> = properties.keys().collect();
        names.sort();
        for name in names {
            let property = &properties[name];
            let key = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}.{name}")
            };
            let description = self.description(property);
            // The holding table's default for it, else its own.
            let default = (inherited.and_then(|table| table.get(name.as_str())))
                .or_else(|| property.get("default"));
            // A whole map's or list's default, empty or null as well.
            let whole = || self.default_text(default, property);
            if let Some(table) = self.struct_of(property) {
                nested.push(Nested {
                    path: key,
                    kind: TableKind::Plain,
                    node: table,
                    description,
                    default: None,
                    inherited: default,
                });
            } else if let Some(table) = self.map_of(property) {
                nested.push(Nested {
                    path: format!("{key}.<name>"),
                    kind: TableKind::Map,
                    node: table,
                    description,
                    default: whole(),
                    inherited: None,
                });
            } else if let Some(table) = self.list_of(property) {
                nested.push(Nested {
                    path: key,
                    kind: TableKind::List,
                    node: table,
                    description,
                    default: whole(),
                    inherited: None,
                });
            } else {
                keys.push(Key {
                    path: key,
                    description,
                    kind: self.kind(property),
                    default: self.default_text(default, property),
                    required: required.contains(name.as_str()),
                    values: self.values(property),
                });
            }
        }
        tables[at].keys = keys;
        for nested in nested {
            let start = tables.len();
            let node = nested.node;
            self.table(
                nested.path,
                nested.kind,
                node,
                nested.default,
                nested.inherited,
                tables,
            );
            // The key's own doc comment, else its type's.
            tables[start].description = nested.description.or_else(|| self.description(node));
        }
        if at == 0 && path.is_empty() {
            tables[0].description = None;
            if tables[0].keys.is_empty() {
                tables.remove(0);
            }
        }
    }

    /// The table of tables by name `node` is, if it's one: the table each
    /// holds.
    fn map_of(&self, node: &'a Value) -> Option<&'a Value> {
        let node = self.unwrap_null(node);
        let values = node.get("additionalProperties")?;
        self.struct_of(values)
    }

    /// The list of tables `node` is, if it's one: the table each holds.
    fn list_of(&self, node: &'a Value) -> Option<&'a Value> {
        let node = self.unwrap_null(node);
        if type_names(node) != ["array"] {
            return None;
        }
        self.struct_of(node.get("items")?)
    }

    /// `node`'s doc comment, else that of the type it refers to.
    fn description(&self, node: &'a Value) -> Option<String> {
        let own = node.get("description").and_then(Value::as_str);
        let own =
            own.or_else(|| (self.unwrap_null(node).get("description")).and_then(Value::as_str));
        own.map(str::to_owned)
    }

    /// An enum's values and their doc comments, if `node` is one.
    fn values(&self, node: &'a Value) -> Vec<(String, Option<String>)> {
        let node = self.unwrap_null(node);
        let mut values = Vec::new();
        for alternative in node
            .get("anyOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let alternative = self.resolve(alternative);
            if alternative.get("oneOf").is_some() || alternative.get("enum").is_some() {
                values.extend(self.values(alternative));
            } else if let Some(items) = alternative.get("items") {
                values.extend(self.values(items));
            }
        }
        for one in node
            .get("oneOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(value) = one.get("const") {
                let description = one.get("description").and_then(Value::as_str);
                values.push((toml(value), description.map(str::to_owned)));
            }
        }
        for value in node
            .get("enum")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            values.push((toml(value), None));
        }
        let mut seen = BTreeSet::new();
        values.retain(|(value, _)| seen.insert(value.clone()));
        values
    }

    /// `node`'s type in words, with its bounds: `integer, 1 to 3600`.
    fn kind(&self, node: &'a Value) -> String {
        let node = self.unwrap_null(node);
        if node.get("oneOf").is_some() || node.get("enum").is_some() {
            let types = self.enum_types(node);
            return types.join(" or ");
        }
        if let Some(alternatives) = node.get("anyOf").and_then(Value::as_array) {
            let named = type_names(node);
            if !named.is_empty() {
                // One type, with alternative bounds: `0, or 20 to 500`.
                let bounds: Vec<String> = alternatives.iter().map(bounds).collect();
                return format!("{}, {}", named.join(" or "), bounds.join(", or "));
            }
            let kinds: Vec<String> = alternatives
                .iter()
                .filter(|alternative| !is_null(alternative))
                .map(|alternative| self.kind(alternative))
                .collect();
            return kinds.join(" or ");
        }
        let named = type_names(node);
        let mut kinds = Vec::new();
        for name in &named {
            let kind = match *name {
                "array" => match node.get("items") {
                    Some(items) => format!("array of {}", self.kind(items)),
                    None => "array".to_owned(),
                },
                "object" => match node.get("additionalProperties") {
                    Some(Value::Bool(true)) | None => "table".to_owned(),
                    Some(Value::Bool(false)) => "empty table".to_owned(),
                    Some(values) => format!("table of {}", self.kind(values)),
                },
                name => name.to_owned(),
            };
            kinds.push(kind);
        }
        let mut kind = if kinds.is_empty() {
            "any".to_owned()
        } else {
            kinds.join(" or ")
        };
        let bound = bounds(node);
        if !bound.is_empty() {
            let _ = write!(kind, ", {bound}");
        }
        kind
    }

    /// The JSON types an enum's values have.
    fn enum_types(&self, node: &'a Value) -> Vec<String> {
        let mut types = BTreeSet::new();
        let consts = (node
            .get("oneOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(|one| one.get("const"));
        let listed = node
            .get("enum")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for value in consts.chain(listed) {
            types.insert(json_type(value));
        }
        types.into_iter().map(str::to_owned).collect()
    }
}

/// A node's `type`, one name or a list, less `null`.
fn type_names(node: &Value) -> Vec<&str> {
    match node.get("type") {
        Some(Value::String(name)) if name != "null" => vec![name.as_str()],
        Some(Value::Array(names)) => (names.iter())
            .filter_map(Value::as_str)
            .filter(|name| *name != "null")
            .collect(),
        _ => Vec::new(),
    }
}

/// A node's bounds in words: `1 to 3600`, `at least 1 item`, `matching
/// `^x$``. Empty with none.
fn bounds(node: &Value) -> String {
    let number = |key: &str| {
        node.get(key)
            .filter(|value| value.is_number())
            .map(Value::to_string)
    };
    let mut words = Vec::new();
    if let Some(value) = node.get("const") {
        words.push(code(&toml(value)));
    }
    match (number("minimum"), number("maximum")) {
        (Some(min), Some(max)) => words.push(format!("{min} to {max}")),
        (Some(min), None) => words.push(format!("at least {min}")),
        (None, Some(max)) => words.push(format!("at most {max}")),
        (None, None) => {}
    }
    if let Some(min) = number("minLength") {
        let characters = if min == "1" {
            "character"
        } else {
            "characters"
        };
        words.push(format!("at least {min} {characters}"));
    }
    if let Some(min) = number("minItems") {
        let items = if min == "1" { "item" } else { "items" };
        words.push(format!("at least {min} {items}"));
    }
    if let Some(pattern) = node.get("pattern").and_then(Value::as_str) {
        words.push(format!("matching {}", code(pattern)));
    }
    words.join(", ")
}

fn is_null(node: &Value) -> bool {
    node.get("type").and_then(Value::as_str) == Some("null")
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "table",
    }
}

/// `value` as TOML writes it, tables inline, keys sorted; a null, which
/// TOML has no word for, as `unset`.
fn toml(value: &Value) -> String {
    match value {
        Value::Null => UNSET.to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        // JSON's escapes are TOML's for what serde_json writes.
        Value::String(value) => Value::String(value.clone()).to_string(),
        Value::Array(values) => {
            let values: Vec<String> = values.iter().map(toml).collect();
            format!("[{}]", values.join(", "))
        }
        Value::Object(values) if values.is_empty() => "{}".to_owned(),
        Value::Object(values) => {
            // A null entry stays, as `unset`: dropping it would read as a
            // default without that key.
            let mut entries: Vec<(&String, &Value)> = values.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let entries: Vec<String> = entries
                .into_iter()
                .map(|(key, value)| format!("{} = {}", toml_key(key), toml(value)))
                .collect();
            format!("{{ {} }}", entries.join(", "))
        }
    }
}

fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if bare {
        key.to_owned()
    } else {
        Value::String(key.to_owned()).to_string()
    }
}

// --- Markdown ---------------------------------------------------------

/// A doc comment as Markdown: its paragraphs, each on one line, its list
/// items kept, and outside code spans every character Markdown or HTML
/// would take as syntax escaped.
fn markdown(text: &str) -> String {
    let mut out = String::new();
    let mut line = String::new();
    let flush = |line: &mut String, out: &mut String| {
        if !line.is_empty() {
            out.push_str(&markdown_inline(line));
            out.push('\n');
            line.clear();
        }
    };
    for raw in text.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            flush(&mut line, &mut out);
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push('\n');
            }
            continue;
        }
        if let Some(item) = raw.strip_prefix("- ").or_else(|| raw.strip_prefix("* ")) {
            flush(&mut line, &mut out);
            line.push_str("\u{0}- ");
            line.push_str(item);
            continue;
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(raw);
    }
    flush(&mut line, &mut out);
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// One line of text as inline Markdown: code spans kept, everything else
/// escaped. A leading NUL marks a list item.
fn markdown_inline(text: &str) -> String {
    let text = text.replace('\n', " ");
    let (prefix, text) = match text.strip_prefix("\u{0}- ") {
        Some(item) => ("- ", item.to_owned()),
        None => ("", text),
    };
    let mut out = String::from(prefix);
    let mut rest = text.as_str();
    while let Some(start) = rest.find('`') {
        let Some(len) = rest[start + 1..].find('`') else {
            break;
        };
        out.push_str(&escape(&rest[..start]));
        out.push_str(&rest[start..start + len + 2]);
        rest = &rest[start + len + 2..];
    }
    out.push_str(&escape(rest));
    out
}

/// `text` with Markdown's and HTML's syntax characters escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '|' | '#' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            ch => out.push(ch),
        }
    }
    out
}

/// `text` as a code span, fenced with more backticks than it holds.
fn code(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        run = if ch == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// `text` as a code span in a table cell, where `|` ends the cell unless
/// escaped.
fn cell_code(text: &str) -> String {
    code(text).replace('|', "\\|")
}

/// A key label as `<kbd>` text: each character Markdown or HTML would read
/// as syntax, or a table as the end of a cell, as an entity.
fn kbd(label: &str) -> String {
    let mut out = String::from("<kbd>");
    for ch in label.chars() {
        if "|<>&`\\*_[]~#!".contains(ch) {
            let _ = write!(out, "&#{};", ch as u32);
        } else {
            out.push(ch);
        }
    }
    out.push_str("</kbd>");
    out
}

// --- Keymap -----------------------------------------------------------

/// The platforms, in the order the pages list them.
const OSES: [Os; 3] = [Os::MacOs, Os::Linux, Os::Windows];

fn os_title(os: Os) -> &'static str {
    match os {
        Os::MacOs => "macOS",
        Os::Linux => "Linux",
        Os::Windows => "Windows",
    }
}

/// A key label as the platform prints it: macOS symbols there, spelled
/// out elsewhere; never as the host prints it.
fn label(os: Os, keys: &fe_keymap::KeySequence) -> String {
    match os {
        Os::MacOs => macos_label(keys),
        Os::Linux | Os::Windows => pc_label(keys),
    }
}

/// A row's key labels, one list a platform in [`OSES`]'s order.
pub type Labels = [Vec<String>; 3];

/// A binding as the shortcuts page lists it, whatever its keys.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bound {
    pub command: String,
    /// Compact JSON; empty with none.
    pub args: String,
    /// Empty for everywhere.
    pub context: String,
}

/// The built-in keymap as resolved for each platform: each binding's key
/// labels, by platform in [`OSES`]'s order, in the keymap's order.
pub struct Keymaps {
    pub bindings: BTreeMap<Bound, Labels>,
}

impl Keymaps {
    /// The keymaps `sources` give, each the text of a keymap resolved for
    /// its platform as [`KeymapLayer::parse_for`] does. Panics on an error
    /// in one: they're the built-in keymap.
    pub fn resolve(sources: &[(Os, &str)]) -> Self {
        let mut bindings: BTreeMap<Bound, Labels> = BTreeMap::new();
        for (os, source) in sources {
            let column = OSES.iter().position(|one| one == os).expect("a known OS");
            let (layer, diagnostics) =
                KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, *os);
            let layer = layer.unwrap_or_else(|| {
                panic!(
                    "the built-in keymap doesn't load on {}: {diagnostics:?}",
                    os.name()
                )
            });
            // As the app applies it: `command = ""` unbinds the keys bound
            // before it, and a later binding of the same keys and context
            // overrides an earlier one.
            let keymap = Keymap::from_layers([layer]);
            let mut commands: Vec<&CommandId> = Vec::new();
            for binding in keymap.bindings() {
                if !commands.contains(&&binding.command) {
                    commands.push(&binding.command);
                }
            }
            for command in commands {
                for binding in keymap.bindings_for(command) {
                    let bound = Bound {
                        command: command.to_string(),
                        args: if binding.args.is_null() {
                            String::new()
                        } else {
                            binding.args.to_string()
                        },
                        context: (binding.context.as_ref())
                            .map(|context| context.to_string())
                            .unwrap_or_default(),
                    };
                    let labels = &mut bindings.entry(bound).or_default()[column];
                    let label = label(*os, &binding.keys);
                    if !labels.contains(&label) {
                        labels.push(label);
                    }
                }
            }
        }
        Self { bindings }
    }

    /// Each command's key labels, by platform, over every binding of it.
    fn by_command(&self) -> BTreeMap<&str, Labels> {
        let mut by: BTreeMap<&str, Labels> = BTreeMap::new();
        for (bound, labels) in &self.bindings {
            let all = by.entry(&bound.command).or_default();
            for (all, labels) in all.iter_mut().zip(labels) {
                for label in labels {
                    if !all.contains(label) {
                        all.push(label.clone());
                    }
                }
            }
        }
        by
    }
}

/// The key columns for `rows`: one a platform, but one for Linux and
/// Windows when every row's keys are the same on both.
fn key_columns<'a>(rows: impl IntoIterator<Item = &'a Labels>) -> Vec<(String, usize)> {
    let same = rows.into_iter().all(|labels| labels[1] == labels[2]);
    if same {
        vec![
            (os_title(OSES[0]).to_owned(), 0),
            ("Linux and Windows".to_owned(), 1),
        ]
    } else {
        (OSES.iter().enumerate())
            .map(|(ix, os)| (os_title(*os).to_owned(), ix))
            .collect()
    }
}

fn keys_cell(labels: &[String]) -> String {
    if labels.is_empty() {
        return "—".to_owned();
    }
    let labels: Vec<String> = labels.iter().map(|label| kbd(label)).collect();
    labels.join(", ")
}

/// A command's namespace: `editor` for `editor.move_left`.
fn namespace(id: &str) -> &str {
    id.split_once('.').map_or(id, |(namespace, _)| namespace)
}

/// The keyboard shortcuts page: a table a command namespace, a row a
/// binding (command, args and context), with the command's title from
/// `titles` and its keys on each platform.
pub fn shortcuts_page(keymaps: &Keymaps, titles: &BTreeMap<String, String>) -> String {
    let mut page = header(
        "Keyboard shortcuts",
        "the built-in keymap (`crates/fe-keymap/keymaps/fastedit.toml`)",
    );
    page.push_str(
        "\nThe default key bindings, by command, with the keys on each platform and the \
         context they apply in. Your `keymap.toml` overrides them.\n",
    );
    let columns = key_columns(keymaps.bindings.values());
    let mut by_namespace: BTreeMap<&str, Vec<(&Bound, &Labels)>> = BTreeMap::new();
    for (bound, labels) in &keymaps.bindings {
        by_namespace
            .entry(namespace(&bound.command))
            .or_default()
            .push((bound, labels));
    }
    for (namespace, rows) in by_namespace {
        let _ = writeln!(page, "\n## `{namespace}`\n");
        page.push_str("| Command | Title |");
        for (title, _) in &columns {
            let _ = write!(page, " {title} |");
        }
        page.push_str(" Context |\n|---|---|");
        for _ in &columns {
            page.push_str("---|");
        }
        page.push_str("---|\n");
        for (bound, labels) in rows {
            let mut command = cell_code(&bound.command);
            if !bound.args.is_empty() {
                let _ = write!(command, " {}", cell_code(&bound.args));
            }
            let title = titles.get(&bound.command).map_or("", String::as_str);
            let _ = write!(page, "| {command} | {} |", escape(title));
            for (_, column) in &columns {
                let _ = write!(page, " {} |", keys_cell(&labels[*column]));
            }
            let context = if bound.context.is_empty() {
                "everywhere".to_owned()
            } else {
                cell_code(&bound.context)
            };
            let _ = writeln!(page, " {context} |");
        }
    }
    page
}

// --- Commands ---------------------------------------------------------

/// Every command [`fe_workbench::command_catalog`] knows, by ID, with its
/// en-US title: the `#[kiln::command]`s of the crates it checks them for,
/// as `inventory` recorded them, and its registries' commands.
pub fn commands() -> BTreeMap<String, String> {
    let mut named: Vec<(String, String)> = Vec::new();
    for record in inventory::iter::<CommandRecord>() {
        if fe_workbench::RECORDED_COMMAND_CRATES.contains(&record.krate) {
            named.push((record.id.to_owned(), record.title.to_owned()));
        }
    }
    for (id, title) in fe_workbench::registry_commands() {
        named.push((id.to_string(), title));
    }
    let mut commands: BTreeMap<String, String> = BTreeMap::new();
    for (id, title) in named {
        // The first of an ID, as the catalog checks it.
        commands
            .entry(id)
            .or_insert_with_key(|id| fe_command::command_label(id, &title).to_string());
    }
    commands
}

/// The commands page: a table a command namespace, a row a command, with
/// its title and every key the built-in keymap binds it to, on each
/// platform.
pub fn commands_page(commands: &BTreeMap<String, String>, keymaps: &Keymaps) -> String {
    let mut page = header(
        "Commands",
        "the command catalog (`fe_workbench::command_catalog()`) and the built-in keymap",
    );
    page.push_str(
        "\nEvery command by ID, the name a keymap, a menu or a plugin runs it by, with its \
         title and its default keys in any context.\n",
    );
    let keys = keymaps.by_command();
    let none: Labels = Default::default();
    let rows: Vec<(&String, &String, &Labels)> = commands
        .iter()
        .map(|(id, title)| (id, title, keys.get(id.as_str()).unwrap_or(&none)))
        .collect();
    let columns = key_columns(rows.iter().map(|(_, _, labels)| *labels));
    let mut by_namespace: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for row in rows {
        by_namespace.entry(namespace(row.0)).or_default().push(row);
    }
    for (namespace, rows) in by_namespace {
        let _ = writeln!(page, "\n## `{namespace}`\n");
        page.push_str("| ID | Title |");
        for (title, _) in &columns {
            let _ = write!(page, " {title} |");
        }
        page.push_str("\n|---|---|");
        for _ in &columns {
            page.push_str("---|");
        }
        page.push('\n');
        for (id, title, labels) in rows {
            let _ = write!(page, "| {} | {} |", cell_code(id), escape(title));
            for (_, column) in &columns {
                let _ = write!(page, " {} |", keys_cell(&labels[*column]));
            }
            page.push('\n');
        }
    }
    page
}
