//! The generated catalogue files, for the goldens: `settings.ftl` from the
//! settings schema, the text the Settings editor shows, whose doc comments
//! are its en-US source; and `settings-template.ftl` from the settings
//! template's prose.

use std::collections::BTreeSet;

use serde_json::Value;

use super::template::{Line, Template, Toml};
use super::{KeyKind, ftl, setting_key, variant_attribute};
use crate::settings_model::{self, Row, SettingKind, title};

/// Top-level tables that would make a key of another kind's.
const RESERVED: [&str; 3] = ["template", "group", "intro"];

/// `settings.ftl` for `schema`: each table's title by its name, then each
/// table's and setting's description by its path, in schema order, with
/// an attribute for each described value ([`variant_attribute`]). Errors
/// for a name [`setting_key`] can't map, or a value no attribute can name.
pub(crate) fn settings_ftl(schema: &Value) -> Result<String, Vec<String>> {
    let walk = settings_model::rows(schema);
    let mut errors = Vec::new();
    for name in schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|properties| properties.keys())
    {
        if RESERVED.contains(&name.as_str()) {
            errors.push(format!("a top-level table can't be named `{name}`"));
        }
    }

    let mut out = String::from(HEADER);
    out.push_str("\n## The Settings editor's group titles, by table name.\n\n");
    let mut names = BTreeSet::new();
    for group in &walk.groups {
        for name in &group.key {
            if names.insert(name.as_str()) {
                ftl::message(
                    &mut out,
                    &setting_key(KeyKind::Group, name),
                    &title(name),
                    &[],
                );
            }
        }
    }

    out.push_str(
        "\n## Each table's and setting's description, by its dotted path, with an\n\
         ## attribute for each value that has one of its own.\n",
    );
    for row in &walk.order {
        let (key, description, variants) = match *row {
            Row::Group(index) => {
                let group = &walk.groups[index];
                (&group.key, &group.description, Vec::new())
            }
            Row::Entry(index) => {
                let entry = &walk.entries[index];
                let variants = match &entry.kind {
                    SettingKind::Enum(variants) => variants
                        .iter()
                        .filter(|variant| !variant.description.is_empty())
                        .map(|variant| (variant.value.as_str(), variant.description.clone()))
                        .collect(),
                    _ => Vec::new(),
                };
                (&entry.key, &entry.description, variants)
            }
        };
        if description.is_empty() && variants.is_empty() {
            continue;
        }
        let path = key.join(".");
        if let Some(name) = key.iter().find(|name| !ftl::is_key_segment(name)) {
            errors.push(format!(
                "{path}: `{name}` isn't letters, digits and `_`, so it has no key"
            ));
            continue;
        }
        let mut attributes: Vec<(String, String)> = Vec::new();
        for (value, text) in variants {
            let name = variant_attribute(value);
            if !ftl::is_identifier(&name) {
                errors.push(format!("{path}: no attribute can name the value {value:?}"));
            } else if attributes.iter().any(|(other, _)| *other == name) {
                errors.push(format!("{path}: two values are the attribute `{name}`"));
            } else {
                attributes.push((name, text));
            }
        }
        out.push('\n');
        ftl::message(
            &mut out,
            &setting_key(KeyKind::Description, &path),
            description,
            &attributes,
        );
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

const HEADER: &str = "\
### The Settings editor's text: generated from fe-config's settings schema,
### whose doc comments are the en-US source. Don't edit it by hand: change
### the doc comment, then rerun fe-config's tests with UPDATE_GOLDEN=1.
";

/// `settings-template.ftl` for `text`, the settings template: one message
/// per block, in order. Errors for a comment line that's neither `#` nor
/// `# …`, two blocks with one key, a key part [`setting_key`] can't map,
/// or a block keyed by a setting or table `schema` doesn't have.
pub(crate) fn template_ftl(text: &str, schema: &Value) -> Result<String, Vec<String>> {
    let template = Template::parse(text);
    let mut errors = check_template(&template, schema);
    let mut out = String::from(TEMPLATE_HEADER);
    let mut keys = BTreeSet::new();
    for block in &template.blocks {
        if !keys.insert(block.key.as_str()) {
            errors.push(format!(
                "line {}: a second block keyed {}",
                block.start + 1,
                block.key
            ));
        }
        out.push('\n');
        ftl::message(&mut out, &block.key, &template.text(block), &[]);
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// A fake locale's `settings-template.ftl` for `text`: every block's
/// message replaced by one line naming its key, long enough to wrap.
pub(crate) fn fake_template_ftl(text: &str) -> String {
    let template = Template::parse(text);
    let mut out = String::from(FAKE_HEADER);
    for block in &template.blocks {
        out.push('\n');
        let marker = format!("{} {FAKE_FILLER}", fake_marker(&block.key));
        ftl::message(&mut out, &block.key, &marker, &[]);
    }
    out
}

/// What the fake locale's message for `key` starts with.
pub(crate) fn fake_marker(key: &str) -> String {
    format!("⟦{key}⟧")
}

/// What follows each fake marker, so that every message wraps.
const FAKE_FILLER: &str =
    "Ŧĥîś ŀîñê ĩś ŀōñĝ ēñōūĝĥ ţō ŵŕāƥ āţ ēîĝĥţŷ čōŀūmñś, şō ţĥē ŵŕāƥƥîñĝ îś ţēśţēđ ţōō.";

fn check_template(template: &Template<'_>, schema: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    for (at, line) in template.lines.iter().enumerate() {
        if template.kinds[at] == Line::Prose && !line.starts_with("# ") {
            errors.push(format!(
                "line {}: a comment starts `# ` or is `#` alone",
                at + 1
            ));
        }
        if let Line::Active(None) = template.kinds[at]
            && !line.trim().is_empty()
        {
            errors.push(format!("line {}: neither TOML nor a comment", at + 1));
        }
    }
    let mut table = String::new();
    let mut paths: Vec<(usize, String)> = Vec::new();
    for (at, kind) in template.kinds.iter().enumerate() {
        match kind {
            Line::Active(Some(Toml::Header(path))) | Line::Commented(Toml::Header(path)) => {
                table = path.clone();
                paths.push((at, path.clone()));
            }
            Line::Active(Some(Toml::Key(key))) | Line::Commented(Toml::Key(key)) => {
                let path = if table.is_empty() {
                    key.clone()
                } else {
                    format!("{table}.{key}")
                };
                paths.push((at, path));
            }
            _ => {}
        }
    }
    for (at, path) in &paths {
        if let Some(part) = path.split('.').find(|part| !ftl::is_key_segment(part)) {
            errors.push(format!(
                "line {}: `{part}` of {path} isn't letters, digits and `_`, so it has no key",
                at + 1
            ));
        }
    }
    for block in &template.blocks {
        let Some(path) = block.key.strip_prefix("settings-template-") else {
            continue;
        };
        if path == "intro" || is_note(path) {
            continue;
        }
        let named = paths
            .iter()
            .find(|(_, candidate)| setting_key(KeyKind::Template, candidate) == block.key);
        match named {
            Some((_, path)) if has_path(schema, path) => {}
            Some((_, path)) => errors.push(format!(
                "line {}: {} names {path}, which the schema hasn't",
                block.start + 1,
                block.key
            )),
            None => errors.push(format!(
                "line {}: {} names no line of the template",
                block.start + 1,
                block.key
            )),
        }
    }
    errors
}

/// Whether `path` ends in `note-<n>`.
fn is_note(path: &str) -> bool {
    path.rsplit_once("-note-")
        .map_or(path.strip_prefix("note-"), |(_, n)| Some(n))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `schema` has a setting or table at the dotted `path`, through
/// arrays' items and maps' entries (`formatters.typescript`).
pub(crate) fn has_path(schema: &Value, path: &str) -> bool {
    let defs = schema.get("$defs").unwrap_or(&Value::Null);
    let mut at = resolve(defs, schema);
    for name in path.split('.') {
        let mut options = vec![at];
        for key in ["anyOf", "oneOf"] {
            if let Some(list) = at.get(key).and_then(Value::as_array) {
                options.extend(list.iter().map(|option| resolve(defs, option)));
            }
        }
        let next = options.into_iter().find_map(|option| {
            let option = match option.get("items") {
                Some(items) if option.get("properties").is_none() => resolve(defs, items),
                _ => option,
            };
            if let Some(property) = option.get("properties").and_then(|p| p.get(name)) {
                return Some(resolve(defs, property));
            }
            option
                .get("additionalProperties")
                .filter(|entry| entry.is_object())
                .map(|entry| resolve(defs, entry))
        });
        match next {
            Some(next) => at = next,
            None => return false,
        }
    }
    true
}

/// `schema` with a `$ref` to `defs` followed.
fn resolve<'a>(defs: &'a Value, schema: &'a Value) -> &'a Value {
    let target = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|path| path.strip_prefix("#/$defs/"))
        .and_then(|name| defs.get(name));
    match target {
        Some(target) => resolve(defs, target),
        None => schema,
    }
}

const TEMPLATE_HEADER: &str = "\
### The prose of settings-template.toml: generated from it, one message per
### block of comment lines that aren't commented TOML. The template is the
### en-US source, so don't edit this by hand: change the template, then
### rerun fe-config's tests with UPDATE_GOLDEN=1.
###
### A translation's lines are written after `# `, wrapped at 80 columns;
### an empty line is `#` alone. Commented TOML around a block, such as
### `# font_size = 13.0`, is never translated.
";

const FAKE_HEADER: &str = "\
### A fake locale for tests: every message of settings-template.ftl as a
### marker naming its key. Generated by fe-config's tests; rerun them with
### UPDATE_GOLDEN=1 after the template changes.
";
