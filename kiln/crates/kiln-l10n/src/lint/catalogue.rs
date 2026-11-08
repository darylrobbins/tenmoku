// SPDX-License-Identifier: Apache-2.0

//! Layer 3, at a catalogue root (the app's, or kiln's): the checks that need
//! the whole tree, read from disk.
//!
//! - **No key is unused.** A message or term counts as used when a source
//!   file holds it as a string literal, bundled data names it, another
//!   message references it, or it starts with a declared dynamic prefix.
//! - **Every reference resolves, in every branch, in its own locale.** A
//!   static walk of each message's and term's value, attributes and every
//!   variant of every select expression. A reference in a `pl` resource must
//!   name a message or term some `pl` resource defines, and an attribute
//!   accessor an attribute of the `pl` definition: Fluent formats a message
//!   wholly inside its own locale's bundle, so en-US's don't count.
//! - **Every message formats**, once, with dummy arguments, in its locale's
//!   bundle.
//! - **No key is defined twice** in one locale.
//! - **Translations stay in step with en-US**: a key en-US lacks, or an
//!   argument the en-US message doesn't take, is an error. A key a locale
//!   lacks is only counted, because it falls back.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use fluent_syntax::ast::{
    self, Entry, Expression, InlineExpression, Pattern, PatternElement, VariantKey,
};

use super::source::{all_literals, crate_sources, dynamic_prefixes, ftl_files};
use crate::catalogue::{bundle, write_pattern};
use crate::{FALLBACK_LOCALE, public_keys};

/// The root catalogue check:
///
/// ```ignore
/// kiln_l10n::lint::catalogue()
///     .crates_under(root.join("crates"))
///     .crates_under(root.join("kiln/crates"))
///     .data_keys(bundled_title_keys)
///     .check();
/// ```
pub fn catalogue() -> CatalogueCheck {
    CatalogueCheck::default()
}

struct Source {
    locale: String,
    name: String,
    text: String,
}

/// A configured root check; see [`catalogue`].
#[derive(Default)]
pub struct CatalogueCheck {
    sources: Vec<Source>,
    literals: BTreeSet<String>,
    data_keys: BTreeSet<String>,
    prefixes: BTreeSet<String>,
    exempt_public: bool,
    errors: Vec<String>,
}

impl CatalogueCheck {
    /// Every crate directly under `dir`: see [`CatalogueCheck::crate_dir`].
    pub fn crates_under(mut self, dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        let mut crates: Vec<_> = match std::fs::read_dir(dir) {
            Ok(entries) => entries.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
            Err(error) => {
                self.errors.push(format!("{}: {error}", dir.display()));
                return self;
            }
        };
        crates.sort();
        for path in crates
            .into_iter()
            .filter(|p| p.join("Cargo.toml").is_file())
        {
            self = self.crate_dir(path);
        }
        self
    }

    /// One crate: its resources, `l10n/<locale>/*.ftl`; its non-test source,
    /// whose string literals count as uses; and the dynamic prefixes its
    /// `resources!` declares.
    pub fn crate_dir(mut self, dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        let crate_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let l10n = dir.join("l10n");
        for path in ftl_files(&l10n) {
            let rel = path.strip_prefix(&l10n).unwrap_or(&path);
            let Some(locale) = rel.components().next() else {
                continue;
            };
            let locale = locale.as_os_str().to_string_lossy().into_owned();
            match std::fs::read_to_string(&path) {
                Ok(text) => self.sources.push(Source {
                    locale,
                    name: format!("{crate_name}/l10n/{}", rel.display()),
                    text,
                }),
                Err(error) => self.errors.push(format!("{}: {error}", path.display())),
            }
        }
        let (files, mut errors) = crate_sources(dir);
        self.errors.append(&mut errors);
        for file in &files {
            self.literals
                .extend(all_literals(&file.syntax).into_iter().map(|l| l.value));
            self.prefixes.extend(dynamic_prefixes(&file.syntax));
        }
        self
    }

    /// One resource, given directly. `name` labels errors.
    pub fn resource(mut self, locale: &str, name: &str, text: &str) -> Self {
        self.sources.push(Source {
            locale: locale.to_string(),
            name: name.to_string(),
            text: text.to_string(),
        });
        self
    }

    /// Keys bundled data names, such as menu files' `title_key`s.
    pub fn data_keys<S: Into<String>>(mut self, keys: impl IntoIterator<Item = S>) -> Self {
        self.data_keys.extend(keys.into_iter().map(Into::into));
        self
    }

    /// A prefix of keys built at run time, tested elsewhere against what
    /// builds them, such as `command-` against the registry.
    pub fn dynamic_prefix(mut self, prefix: &str) -> Self {
        self.prefixes.insert(prefix.to_string());
        self
    }

    /// Doesn't report unused keys in `## public` groups: for a library's
    /// root, whose public keys apps outside it may be the only users of.
    pub fn exempt_public(mut self) -> Self {
        self.exempt_public = true;
        self
    }

    pub fn report(&self) -> CatalogueReport {
        let mut report = CatalogueReport {
            parse: self.errors.clone(),
            ..Default::default()
        };
        let mut parsed: Vec<(&Source, ast::Resource<&str>)> = Vec::new();
        for source in &self.sources {
            let resource = match fluent_syntax::parser::parse(source.text.as_str()) {
                Ok(resource) => resource,
                Err((resource, errors)) => {
                    for error in errors {
                        report
                            .parse
                            .push(format!("{}: {}: {error:?}", source.locale, source.name));
                    }
                    resource
                }
            };
            parsed.push((source, resource));
        }

        // Every definition, by locale.
        let mut locales: BTreeMap<&str, Defs> = BTreeMap::new();
        for (source, resource) in &parsed {
            let defs = locales.entry(source.locale.as_str()).or_default();
            for entry in &resource.body {
                match entry {
                    Entry::Message(message) => defs
                        .messages
                        .entry(message.id.name.to_string())
                        .or_default()
                        .push(Def::new(
                            &source.name,
                            message.value.as_ref(),
                            &message.attributes,
                        )),
                    Entry::Term(term) => defs
                        .terms
                        .entry(term.id.name.to_string())
                        .or_default()
                        .push(Def::new(&source.name, Some(&term.value), &term.attributes)),
                    _ => {}
                }
            }
        }
        let empty = Defs::default();
        let english = locales.get(FALLBACK_LOCALE).unwrap_or(&empty);

        for (locale, defs) in &locales {
            for (kind, map) in [("message", &defs.messages), ("term", &defs.terms)] {
                for (id, all) in map {
                    if all.len() > 1 {
                        let files: Vec<&str> = all.iter().map(|d| d.file.as_str()).collect();
                        let sigil = if kind == "term" { "-" } else { "" };
                        report.duplicates.push(format!(
                            "{locale}: {kind} `{sigil}{id}` is defined in {}",
                            files.join(" and ")
                        ));
                    }
                }
            }
        }

        // The reference walk, and every reference for the unused check.
        let mut referenced_messages = BTreeSet::new();
        let mut referenced_terms = BTreeSet::new();
        for (source, resource) in &parsed {
            let locale = source.locale.as_str();
            let defs = &locales[locale];
            for entry in &resource.body {
                let (label, refs) = match entry {
                    Entry::Message(message) => (
                        message.id.name.to_string(),
                        entry_refs(message.value.as_ref(), &message.attributes),
                    ),
                    Entry::Term(term) => (
                        format!("-{}", term.id.name),
                        entry_refs(Some(&term.value), &term.attributes),
                    ),
                    _ => continue,
                };
                for found in refs {
                    let path = if found.path.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", found.path.join(" "))
                    };
                    let at = format!("{locale}: {}: {label}{path}", source.name);
                    match &found.reference {
                        Reference::Message(id, attribute) => {
                            referenced_messages.insert(id.clone());
                            check_reference(
                                &mut report.walk,
                                &at,
                                locale,
                                id,
                                attribute.as_deref(),
                                "",
                                &defs.messages,
                                &english.messages,
                            );
                        }
                        Reference::Term(id, attribute) => {
                            referenced_terms.insert(id.clone());
                            check_reference(
                                &mut report.walk,
                                &at,
                                locale,
                                id,
                                attribute.as_deref(),
                                "-",
                                &defs.terms,
                                &english.terms,
                            );
                        }
                        Reference::Variable(_) => {}
                    }
                }
            }
        }

        // Stale and missing translations.
        for (locale, defs) in locales.iter().filter(|(l, _)| **l != FALLBACK_LOCALE) {
            for (id, all) in &defs.messages {
                let def = &all[0];
                match english.messages.get(id) {
                    None => report
                        .stale
                        .push(format!("{locale}: {}: `{id}` isn't in en-US", def.file)),
                    Some(en) => {
                        for variable in def.variables.difference(&en[0].variables) {
                            report.stale.push(format!(
                                "{locale}: {}: `{id}` takes `${variable}`, which en-US's doesn't",
                                def.file
                            ));
                        }
                    }
                }
            }
            let missing = english
                .messages
                .keys()
                .filter(|id| !defs.messages.contains_key(*id))
                .count();
            if missing > 0 {
                report.missing.insert(locale.to_string(), missing);
            }
        }

        // Formatting, each locale in a bundle of its own resources, as at run time.
        let mut by_locale: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
        for source in &self.sources {
            by_locale
                .entry(source.locale.as_str())
                .or_default()
                .push((source.name.clone(), source.text.clone()));
        }
        for (locale, files) in &by_locale {
            let Ok(langid) = locale.parse() else {
                report
                    .parse
                    .push(format!("`{locale}` is not a BCP 47 language tag"));
                continue;
            };
            let mut ignored = Vec::new();
            let built = bundle(&langid, files, &mut ignored);
            for (name, text) in files {
                let resource = match FluentResource::try_new(text.clone()) {
                    Ok(resource) | Err((resource, _)) => Arc::new(resource),
                };
                for entry in resource.entries() {
                    let Entry::Message(message) = entry else {
                        continue;
                    };
                    let id = message.id.name;
                    let Some(formatted) = built.get_message(id) else {
                        continue;
                    };
                    let mut variables = BTreeSet::new();
                    for found in entry_refs(message.value.as_ref(), &message.attributes) {
                        if let Reference::Variable(name) = found.reference {
                            variables.insert(name);
                        }
                    }
                    let mut args = FluentArgs::new();
                    for name in &variables {
                        args.set(name.clone(), FluentValue::from(1));
                    }
                    let patterns = formatted
                        .value()
                        .map(|p| (String::new(), p))
                        .into_iter()
                        .chain(
                            formatted
                                .attributes()
                                .map(|a| (format!(".{}", a.id()), a.value())),
                        );
                    for (attribute, pattern) in patterns {
                        let mut errors = Vec::new();
                        write_pattern(&built, pattern, Some(&args), &mut errors);
                        for error in errors {
                            report
                                .format
                                .push(format!("{locale}: {name}: {id}{attribute}: {error}"));
                        }
                    }
                }
            }
        }

        // Unused keys.
        let public: BTreeSet<String> = if self.exempt_public {
            self.sources
                .iter()
                .filter(|s| s.locale == FALLBACK_LOCALE)
                .flat_map(|s| public_keys(&s.text))
                .collect()
        } else {
            BTreeSet::new()
        };
        let used = |key: &str| {
            self.literals.contains(key)
                || self.data_keys.contains(key)
                || self.prefixes.iter().any(|p| key.starts_with(p.as_str()))
                || public.contains(key)
        };
        for (id, all) in &english.messages {
            if !referenced_messages.contains(id) && !used(id) {
                report
                    .unused
                    .push(format!("en-US: {}: `{id}` is unused", all[0].file));
            }
        }
        for (id, all) in &english.terms {
            let key = format!("-{id}");
            if !referenced_terms.contains(id) && !used(&key) {
                report
                    .unused
                    .push(format!("en-US: {}: `{key}` is unused", all[0].file));
            }
        }
        report
    }

    #[track_caller]
    pub fn check(&self) {
        let report = self.report();
        assert!(report.is_clean(), "{report}");
    }
}

/// What the root check found, by check.
#[derive(Clone, Debug, Default)]
pub struct CatalogueReport {
    /// Files that don't parse, or can't be read.
    pub parse: Vec<String>,
    /// References that don't resolve in their own locale.
    pub walk: Vec<String>,
    /// Errors formatting a message with dummy arguments.
    pub format: Vec<String>,
    pub duplicates: Vec<String>,
    /// Translations of keys en-US lacks, or with arguments en-US's lack.
    pub stale: Vec<String>,
    pub unused: Vec<String>,
    /// How many en-US keys each other locale lacks. Not an error.
    pub missing: BTreeMap<String, usize>,
}

impl CatalogueReport {
    pub fn is_clean(&self) -> bool {
        [
            &self.parse,
            &self.walk,
            &self.format,
            &self.duplicates,
            &self.stale,
            &self.unused,
        ]
        .iter()
        .all(|list| list.is_empty())
    }
}

impl fmt::Display for CatalogueReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sections = [
            ("doesn't parse", &self.parse),
            ("unresolved reference", &self.walk),
            ("doesn't format", &self.format),
            ("duplicate key", &self.duplicates),
            ("stale translation", &self.stale),
            ("unused key", &self.unused),
        ];
        for (what, list) in sections {
            for line in list {
                writeln!(f, "{what}: {line}")?;
            }
        }
        for (locale, count) in &self.missing {
            writeln!(
                f,
                "note: {locale} lacks {count} en-US key(s), which fall back"
            )?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Defs {
    messages: BTreeMap<String, Vec<Def>>,
    terms: BTreeMap<String, Vec<Def>>,
}

struct Def {
    file: String,
    attributes: BTreeSet<String>,
    variables: BTreeSet<String>,
}

impl Def {
    fn new(file: &str, value: Option<&Pattern<&str>>, attributes: &[ast::Attribute<&str>]) -> Self {
        let mut variables = BTreeSet::new();
        for found in entry_refs(value, attributes) {
            if let Reference::Variable(name) = found.reference {
                variables.insert(name);
            }
        }
        Self {
            file: file.to_string(),
            attributes: attributes.iter().map(|a| a.id.name.to_string()).collect(),
            variables,
        }
    }
}

enum Reference {
    Message(String, Option<String>),
    Term(String, Option<String>),
    Variable(String),
}

struct Found {
    reference: Reference,
    /// Where in the entry: `.attribute`, then `[variant]` keys.
    path: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn check_reference(
    out: &mut Vec<String>,
    at: &str,
    locale: &str,
    id: &str,
    attribute: Option<&str>,
    sigil: &str,
    defs: &BTreeMap<String, Vec<Def>>,
    english: &BTreeMap<String, Vec<Def>>,
) {
    let only_english = |has: bool| {
        if has && locale != FALLBACK_LOCALE {
            " (defined only in en-US)"
        } else {
            ""
        }
    };
    match (defs.get(id), attribute) {
        (None, _) => {
            let note = only_english(english.contains_key(id));
            out.push(format!(
                "{at}: `{sigil}{id}` isn't defined in {locale}{note}"
            ));
        }
        (Some(all), Some(attribute)) if !all[0].attributes.contains(attribute) => {
            let in_english = english
                .get(id)
                .is_some_and(|en| en[0].attributes.contains(attribute));
            let note = only_english(in_english);
            out.push(format!(
                "{at}: `{sigil}{id}.{attribute}`: {locale}'s `{sigil}{id}` has no `.{attribute}`{note}"
            ));
        }
        _ => {}
    }
}

fn entry_refs(value: Option<&Pattern<&str>>, attributes: &[ast::Attribute<&str>]) -> Vec<Found> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    if let Some(value) = value {
        walk_pattern(value, &mut path, &mut out);
    }
    for attribute in attributes {
        path.push(format!(".{}", attribute.id.name));
        walk_pattern(&attribute.value, &mut path, &mut out);
        path.pop();
    }
    out
}

fn walk_pattern(pattern: &Pattern<&str>, path: &mut Vec<String>, out: &mut Vec<Found>) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            walk_expression(expression, path, out);
        }
    }
}

fn walk_expression(expression: &Expression<&str>, path: &mut Vec<String>, out: &mut Vec<Found>) {
    match expression {
        Expression::Select { selector, variants } => {
            walk_inline(selector, path, out);
            for variant in variants {
                let key = match &variant.key {
                    VariantKey::Identifier { name } => name.to_string(),
                    VariantKey::NumberLiteral { value } => value.to_string(),
                };
                path.push(if variant.default {
                    format!("*[{key}]")
                } else {
                    format!("[{key}]")
                });
                walk_pattern(&variant.value, path, out);
                path.pop();
            }
        }
        Expression::Inline(inline) => walk_inline(inline, path, out),
    }
}

fn walk_inline(inline: &InlineExpression<&str>, path: &mut Vec<String>, out: &mut Vec<Found>) {
    let mut push = |reference| {
        out.push(Found {
            reference,
            path: path.clone(),
        })
    };
    match inline {
        InlineExpression::MessageReference { id, attribute } => push(Reference::Message(
            id.name.to_string(),
            attribute.as_ref().map(|a| a.name.to_string()),
        )),
        InlineExpression::TermReference {
            id,
            attribute,
            arguments,
        } => {
            push(Reference::Term(
                id.name.to_string(),
                attribute.as_ref().map(|a| a.name.to_string()),
            ));
            if let Some(arguments) = arguments {
                walk_arguments(arguments, path, out);
            }
        }
        InlineExpression::VariableReference { id } => {
            push(Reference::Variable(id.name.to_string()))
        }
        InlineExpression::FunctionReference { arguments, .. } => {
            walk_arguments(arguments, path, out)
        }
        InlineExpression::Placeable { expression } => walk_expression(expression, path, out),
        InlineExpression::StringLiteral { .. } | InlineExpression::NumberLiteral { .. } => {}
    }
}

fn walk_arguments(
    arguments: &ast::CallArguments<&str>,
    path: &mut Vec<String>,
    out: &mut Vec<Found>,
) {
    for positional in &arguments.positional {
        walk_inline(positional, path, out);
    }
    for named in &arguments.named {
        walk_inline(&named.value, path, out);
    }
}
