// SPDX-License-Identifier: Apache-2.0

//! Plugin API version compatibility: which plugins load on which host.
//!
//! A plugin names the API version it needs; the host serves that version when the major matches
//! and the minor is not above its own. wasmtime's linker already matches semver-compatible
//! interface versions, so an adapter is only needed for a breaking difference, and only for a
//! minor the host still supports. A major is dropped only after a deprecation window.
//!
//! A minor adds exports as well as imports, and an older component has none of the new ones, so
//! the host keeps one generated [`World`] per supported minor and checks a component against the
//! world of the minor it needs ([`world_for`]): a 1.0 plugin is never asked for a 1.1 export.

use kiln_core::ApiVersion;
use kiln_l10n::tr;
use wasmtime::component::{InstancePre, Linker};

use crate::bindings;

/// The plugin API version this host implements.
pub const HOST_API: ApiVersion = ApiVersion { major: 1, minor: 1 };

/// A generated `kiln:extension` world: the one a released or current API minor names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum World {
    /// `kiln:extension@1.0.0`, frozen in `wit/released/1.0`.
    V1_0,
    /// `kiln:extension@1.1.0`, in `wit/`.
    V1_1,
}

impl World {
    /// Every world this build has, oldest first: one per minor of [`HOST_API`]'s major.
    pub const ALL: [World; 2] = [World::V1_0, World::V1_1];

    /// The world of [`HOST_API`], generated from `wit/`.
    pub const CURRENT: World = World::V1_1;

    /// The API version this world is.
    pub const fn api(self) -> ApiVersion {
        let minor = match self {
            Self::V1_0 => 0,
            Self::V1_1 => 1,
        };
        ApiVersion { major: 1, minor }
    }

    /// The version its WIT packages carry, such as `1.1.0`.
    pub fn package_version(self) -> String {
        let ApiVersion { major, minor } = self.api();
        format!("{major}.{minor}.0")
    }

    /// The full name of its `kiln:extension` interface `name`, such as
    /// `kiln:extension/command-handler@1.0.0`.
    pub fn interface(self, name: &str) -> String {
        ["kiln:extension/", name, "@", &self.package_version()].concat()
    }

    /// Checks that `pre`'s component has every export this world asks for, typed as the world
    /// types it. Its imports were checked as `pre` was linked.
    pub fn check<T: 'static>(self, pre: &InstancePre<T>) -> wasmtime::Result<()> {
        match self {
            Self::V1_0 => bindings::v1_0::ExtensionPre::new(pre.clone()).map(drop),
            Self::V1_1 => bindings::ExtensionPre::new(pre.clone()).map(drop),
        }
    }
}

/// The world a plugin needing `needed` runs against on this build's host: the generated world of
/// `needed`'s minor, or why it can't load.
pub fn world_for(needed: ApiVersion) -> Result<World, Incompatible> {
    Compat::CURRENT.world_for(needed)
}

/// Majors older than the host's that still load: each major's last supported host version.
/// A plugin of that major loads while the host is at or below it; none yet.
pub const DEPRECATED: &[(u32, ApiVersion)] = &[];

/// Why a plugin cannot load on this host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Incompatible {
    NeedsNewerHost {
        needed: ApiVersion,
        host: ApiVersion,
    },
    UnsupportedMajor {
        needed: ApiVersion,
        host: ApiVersion,
    },
}

/// In the active locale.
impl std::fmt::Display for Incompatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (key, needed, host) = match self {
            Self::NeedsNewerHost { needed, host } => ("kiln-plugin-needs-newer-host", needed, host),
            Self::UnsupportedMajor { needed, host } => {
                ("kiln-plugin-unsupported-major", needed, host)
            }
        };
        f.write_str(&tr!(
            key,
            needed = needed.to_string(),
            host = host.to_string()
        ))
    }
}

/// A host's API version and its deprecation table.
#[derive(Clone, Copy, Debug)]
pub struct Compat {
    pub host: ApiVersion,
    pub deprecated: &'static [(u32, ApiVersion)],
}

impl Compat {
    /// This build's host.
    pub const CURRENT: Compat = Compat {
        host: HOST_API,
        deprecated: DEPRECATED,
    };

    /// Whether a plugin needing `needed` loads on this host.
    pub fn check(&self, needed: ApiVersion) -> Result<(), Incompatible> {
        let host = self.host;
        if needed.major == host.major {
            return if needed.minor <= host.minor {
                Ok(())
            } else {
                Err(Incompatible::NeedsNewerHost { needed, host })
            };
        }
        let in_window = self
            .deprecated
            .iter()
            .any(|&(major, last)| major == needed.major && host <= last);
        if in_window {
            Ok(())
        } else {
            Err(Incompatible::UnsupportedMajor { needed, host })
        }
    }

    /// The world a plugin needing `needed` runs against on this host: the generated world of
    /// `needed`'s minor, once [`check`](Self::check) admits it.
    pub fn world_for(&self, needed: ApiVersion) -> Result<World, Incompatible> {
        self.check(needed)?;
        if let Some(world) = World::ALL.into_iter().find(|world| world.api() == needed) {
            return Ok(world);
        }
        // A version this host admits but this build has no world of: a host said to be newer
        // than this build, or a deprecated major, whose worlds aren't written yet.
        let built = World::CURRENT.api();
        Err(if needed.major == built.major {
            Incompatible::NeedsNewerHost {
                needed,
                host: built,
            }
        } else {
            Incompatible::UnsupportedMajor {
                needed,
                host: self.host,
            }
        })
    }
}

/// Links the interfaces of an older minor that a breaking change left behind.
pub struct Adapter<T: 'static> {
    /// The oldest API version this adapter serves.
    pub serves: ApiVersion,
    pub link: fn(&mut Linker<T>) -> wasmtime::Result<()>,
}

/// Registers `adapters` that serve a version below `compat`'s host and are still in its reach.
pub fn link_adapters<T: 'static>(
    compat: &Compat,
    adapters: &[Adapter<T>],
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    for adapter in adapters {
        if compat.check(adapter.serves).is_ok() && adapter.serves < compat.host {
            (adapter.link)(linker)?;
        }
    }
    Ok(())
}

/// Adds the imports older API versions need to `linker`; none yet.
pub fn link<T: 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    link_adapters(&Compat::CURRENT, &[], linker)
}

/// A released minor's frozen WIT against the current WIT: rule 1 of a minor, checked.
///
/// A minor only adds: new interfaces, new functions and new types. It never adds a case to an
/// existing `variant` or `enum`, a flag or a field to an existing `flags` or `record`, or
/// changes a function: the component model takes those as other types, so a component built
/// for the older minor would no longer link. Each released minor's files are kept as released
/// in `wit/released/<minor>/`, [`digest`](released::digest) records that they stay so, and
/// [`changes`](released::changes) finds what the current files changed of them.
pub mod released {
    use std::{collections::BTreeMap, fs, io, path::Path};

    use sha2::{Digest, Sha256};

    /// WIT files by file name, their text with `\n` line endings.
    pub type Files = BTreeMap<String, String>;

    /// The `.wit` files directly in `dir`.
    pub fn read(dir: &Path) -> io::Result<Files> {
        let mut files = Files::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "wit") {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let text = fs::read_to_string(&path)?.replace("\r\n", "\n");
                files.insert(name.into_owned(), text);
            }
        }
        Ok(files)
    }

    /// A SHA-256 of `files`, names and text, in hex: what a frozen copy is recorded as.
    pub fn digest(files: &Files) -> String {
        let mut hash = Sha256::new();
        for (name, text) in files {
            for part in [name, text] {
                hash.update((part.len() as u64).to_le_bytes());
                hash.update(part.as_bytes());
            }
        }
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// A released definition the current WIT doesn't serve as released. A definition is named
    /// by its container (`interface types`, `world extension`, or `package` for what is outside
    /// both) and its item in it (`variant event`, `log`, `export event-handler`, or
    /// `use types . { range }`), empty for the container itself; its text is its tokens.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Change {
        /// The current WIT has no such definition.
        Removed { container: String, item: String },
        /// The current WIT defines it otherwise.
        Changed {
            container: String,
            item: String,
            was: String,
            now: String,
        },
        /// A file isn't a sequence of WIT items: one of the released files, or a current one.
        Unparsed { released: bool, file: String },
    }

    /// What `current` dropped or changed of `released`'s definitions: none when it only adds.
    /// Comments, layout, the files a definition is in and the package versions are ignored;
    /// each name a `use` brings in counts on its own.
    pub fn changes(released: &Files, current: &Files) -> Vec<Change> {
        let (released, current) = match (definitions(released), definitions(current)) {
            (Ok(released), Ok(current)) => (released, current),
            (Err(file), _) => {
                return vec![Change::Unparsed {
                    released: true,
                    file,
                }];
            }
            (_, Err(file)) => {
                return vec![Change::Unparsed {
                    released: false,
                    file,
                }];
            }
        };
        released
            .into_iter()
            .filter_map(|(key, was)| {
                let now = current.get(&key).cloned();
                let (container, item) = key;
                match now {
                    None => Some(Change::Removed { container, item }),
                    Some(now) if now != was => Some(Change::Changed {
                        container,
                        item,
                        was,
                        now,
                    }),
                    Some(_) => None,
                }
            })
            .collect()
    }

    /// Each definition of `files`, by its container and its item in it (see [`Change`]), as its
    /// tokens; or the name of a file that doesn't parse.
    fn definitions(files: &Files) -> Result<BTreeMap<(String, String), String>, String> {
        let mut out = BTreeMap::new();
        for (name, text) in files {
            let tokens = tokens(text);
            let mut at = 0;
            while at < tokens.len() {
                at = match tokens[at].as_str() {
                    "interface" | "world" => {
                        let unparsed = || name.clone();
                        let container = [&tokens[at], tokens.get(at + 1).ok_or_else(unparsed)?]
                            .map(String::as_str)
                            .join(" ");
                        if tokens.get(at + 2).map(String::as_str) != Some("{") {
                            return Err(unparsed());
                        }
                        out.insert((container.clone(), String::new()), container.clone());
                        let end = block(&tokens[at + 3..], |item| {
                            for (key, text) in items(item) {
                                out.insert((container.clone(), key), text);
                            }
                        })
                        .ok_or_else(unparsed)?;
                        at + 3 + end
                    }
                    _ => {
                        let end = item_end(&tokens[at..]).ok_or_else(|| name.clone())?;
                        for (key, text) in items(&tokens[at..at + end]) {
                            out.insert(("package".to_owned(), key), text);
                        }
                        at + end
                    }
                };
            }
        }
        Ok(out)
    }

    /// Hands `each` the items of the block `tokens` starts inside, and returns where the block
    /// ends, past its `}`.
    fn block(tokens: &[String], mut each: impl FnMut(&[String])) -> Option<usize> {
        let mut at = 0;
        loop {
            match tokens.get(at)?.as_str() {
                "}" => return Some(at + 1),
                _ => {
                    let end = item_end(&tokens[at..])?;
                    each(&tokens[at..at + end]);
                    at += end;
                }
            }
        }
    }

    /// Where the item `tokens` starts with ends: past its `;`, or past the `}` of its body when
    /// no `;` follows it.
    fn item_end(tokens: &[String]) -> Option<usize> {
        let mut depth = 0usize;
        for (at, token) in tokens.iter().enumerate() {
            match token.as_str() {
                ";" if depth == 0 => return Some(at + 1),
                "{" => depth += 1,
                "}" => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 && tokens.get(at + 1).map(String::as_str) != Some(";") {
                        return Some(at + 1);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// An item's definitions by name: a `use` gives one for each name it brings in.
    fn items(item: &[String]) -> Vec<(String, String)> {
        let text = |tokens: &[String]| tokens.join(" ");
        let token = |at: usize| item.get(at).map(String::as_str).unwrap_or_default();
        // What it says, without its `;`: `import log`.
        let said = text(
            item.strip_suffix([";".to_owned()].as_slice())
                .unwrap_or(item),
        );
        match token(0) {
            "use" => {
                let Some(open) = item.iter().position(|token| token == "{") else {
                    return vec![(said, text(item))];
                };
                let path = &item[..open];
                let close = item
                    .iter()
                    .rposition(|token| token == "}")
                    .filter(|&close| close > open)
                    .unwrap_or(item.len());
                item[open + 1..close]
                    .split(|token| token == ",")
                    .filter(|name| !name.is_empty())
                    .map(|name| {
                        let one = [text(path), text(name)].join(" { ") + " }";
                        (one.clone(), one)
                    })
                    .collect()
            }
            "record" | "variant" | "enum" | "flags" | "resource" | "type" => {
                vec![(text(&item[..item.len().min(2)]), text(item))]
            }
            "import" | "export" if token(2) == ":" => {
                vec![(text(&item[..item.len().min(2)]), text(item))]
            }
            _ if token(1) == ":" => vec![(token(0).to_owned(), text(item))],
            // `import log;`, `include other;`.
            _ => vec![(said, text(item))],
        }
    }

    /// `text`'s tokens, without comments, each package version as a bare `@`.
    fn tokens(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let word = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '%');
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(&c) = chars.get(at) {
            let next = chars.get(at + 1).copied();
            if c.is_whitespace() {
                at += 1;
            } else if c == '/' && next == Some('/') {
                while chars.get(at).is_some_and(|&c| c != '\n') {
                    at += 1;
                }
            } else if c == '/' && next == Some('*') {
                at += 2;
                while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                    at += 1;
                }
                at += 2;
            } else if c == '@' && next.is_some_and(|c| c.is_ascii_digit()) {
                // `@1.0.0`, but not the `.` of the `.{` after it.
                at += 1;
                while let Some(&c) = chars.get(at) {
                    let continues = c.is_ascii_alphanumeric()
                        || matches!(c, '-' | '+')
                        || (c == '.' && chars.get(at + 1).is_some_and(char::is_ascii_alphanumeric));
                    if !continues {
                        break;
                    }
                    at += 1;
                }
                out.push("@".to_owned());
            } else if word(c) {
                let start = at;
                while chars.get(at).is_some_and(|&c| word(c)) {
                    at += 1;
                }
                out.push(chars[start..at].iter().collect());
            } else {
                out.push(c.to_string());
                at += 1;
            }
        }
        out
    }
}
