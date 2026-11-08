// SPDX-License-Identifier: Apache-2.0

//! The catalogue files each crate embeds with [`resources!`](crate::resources).

use std::collections::BTreeSet;

use fluent_syntax::ast::Entry;

/// One embedded `.ftl` file.
#[derive(Debug)]
pub struct ResourceFile {
    /// Its locale's BCP 47 tag, such as `en-US`.
    pub locale: &'static str,
    /// The path `resources!` was given, relative to the file that called it.
    pub path: &'static str,
    /// The file's text.
    pub source: &'static str,
}

/// A crate's catalogue files, as [`resources!`](crate::resources) records
/// them.
#[derive(Debug)]
pub struct Resources {
    /// The crate that owns these messages (its `CARGO_PKG_NAME`).
    pub crate_name: &'static str,
    /// The crate's directory when it was built.
    pub manifest_dir: &'static str,
    /// `file!()` of the call, which `path`s are relative to.
    pub declared_in: &'static str,
    pub files: &'static [ResourceFile],
    /// Prefixes of keys the crate builds at run time, such as `command-`.
    pub dynamic_prefixes: &'static [&'static str],
}

impl Resources {
    /// Every crate's resources linked into this binary.
    pub fn registered() -> Vec<&'static Resources> {
        let mut all: Vec<&'static Resources> = inventory::iter::<Registered>
            .into_iter()
            .map(|r| r.0)
            .collect();
        // Link order isn't stable; the catalogue's "first one wins" for a
        // duplicate key should be.
        all.sort_by_key(|r| r.crate_name);
        all
    }

    /// The locales this crate ships.
    pub fn locales(&self) -> BTreeSet<&'static str> {
        self.files.iter().map(|f| f.locale).collect()
    }
}

/// What [`resources!`](crate::resources) submits to `inventory`.
pub struct Registered(pub &'static Resources);

inventory::collect!(Registered);

/// The messages and terms (`-name`) in a file's `## public` groups, which
/// other crates may use. A group runs from its `## …` comment to the next
/// group comment; it is public when the comment's first word is `public`.
pub fn public_keys(source: &str) -> Vec<String> {
    let resource = match fluent_syntax::parser::parse(source) {
        Ok(resource) => resource,
        Err((resource, _)) => resource,
    };
    let mut public = false;
    let mut keys = Vec::new();
    for entry in &resource.body {
        match entry {
            Entry::GroupComment(comment) => {
                public = comment
                    .content
                    .iter()
                    .find(|line| !line.trim().is_empty())
                    .and_then(|line| line.split_whitespace().next())
                    .is_some_and(|word| word == "public");
            }
            Entry::Message(message) if public => keys.push(message.id.name.to_string()),
            Entry::Term(term) if public => keys.push(format!("-{}", term.id.name)),
            _ => {}
        }
    }
    keys
}
