// SPDX-License-Identifier: Apache-2.0

//! Capabilities a plugin holds, in the strings of the plugin record
//! (`docs/decisions/vmtnmkay62-extensions-and-plugins.md` §7): "Nothing
//! ambient. The manifest lists capabilities (`buffer.read`, `buffer.write`,
//! `commands`, `decorations`, `status`, `fs.read:workspace`,
//! `fs.write:workspace`, `process.spawn:<program>`, `net:<host>`); the user
//! grants them at install, by a prompt that names them." A first-party
//! plugin compiled into the app, such as the built-in chat, holds them the
//! same way, granted by the app instead of a prompt.
//!
//! The plugin host's checks will use [`Grants::allows`]: a host import or a
//! plugin-origin dispatch asks for the [`Capability`] it needs and is refused
//! unless the plugin's grants cover it. This module only names and compares
//! capabilities; enforcing them (resolving a program on `PATH`, checking a
//! host after DNS and on each redirect) is the host's.

use std::{collections::BTreeSet, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// One capability, as a manifest names it. Parses from and prints as the
/// record's strings, and serializes as the string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Capability {
    /// `buffer.read`: snapshot a buffer's text and ranges at a version.
    BufferRead,
    /// `buffer.write`: edit a buffer, as one transaction against the version
    /// read, or propose a change the user accepts.
    BufferWrite,
    /// `commands`: register commands `<extension id>.<name>` and invoke
    /// commands through an `Invocation`, each command's own capabilities
    /// checked as well.
    Commands,
    /// `decorations`: push decoration sets and gutter-column entries for a
    /// buffer.
    Decorations,
    /// `status`: show status items (text, tooltip and a command).
    Status,
    /// `fs.read:<scope>`: read files in the scope.
    FsRead(FsScope),
    /// `fs.write:<scope>`: write files in the scope.
    FsWrite(FsScope),
    /// `process.spawn:<program>`: run the named program, which the host
    /// resolves on `PATH` at grant time; `process.spawn:*` runs any program.
    ProcessSpawn(Target),
    /// `net:<host>`: reach the named host through the host's HTTP and socket
    /// imports; `net:*` reaches any host.
    Net(Target),
}

/// Where an `fs.read` or `fs.write` capability reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FsScope {
    /// `workspace`: the folders open in the workspace.
    Workspace,
}

/// What a `process.spawn` or `net` capability names.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// A program, or a host (lower-case, no scheme, port or path).
    Named(String),
    /// `*`: every program or host. The record calls these extreme: granting
    /// one shows its own warning.
    Any,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid capability {capability:?}: {reason}")]
pub struct CapabilityError {
    pub capability: String,
    pub reason: &'static str,
}

impl Capability {
    pub fn parse(capability: &str) -> Result<Self, CapabilityError> {
        let invalid = |reason| CapabilityError {
            capability: capability.to_string(),
            reason,
        };
        let (name, arg) = match capability.split_once(':') {
            Some((name, arg)) => (name, Some(arg)),
            None => (capability, None),
        };
        match (name, arg) {
            ("buffer.read", None) => Ok(Self::BufferRead),
            ("buffer.write", None) => Ok(Self::BufferWrite),
            ("commands", None) => Ok(Self::Commands),
            ("decorations", None) => Ok(Self::Decorations),
            ("status", None) => Ok(Self::Status),
            ("buffer.read" | "buffer.write" | "commands" | "decorations" | "status", Some(_)) => {
                Err(invalid("this capability takes no argument"))
            }
            ("fs.read", Some(scope)) => Ok(Self::FsRead(parse_scope(scope).map_err(invalid)?)),
            ("fs.write", Some(scope)) => Ok(Self::FsWrite(parse_scope(scope).map_err(invalid)?)),
            ("process.spawn", Some(program)) => {
                Ok(Self::ProcessSpawn(parse_program(program).map_err(invalid)?))
            }
            ("net", Some(host)) => Ok(Self::Net(parse_host(host).map_err(invalid)?)),
            ("fs.read" | "fs.write", None) => {
                Err(invalid("needs a scope, as in `fs.read:workspace`"))
            }
            ("process.spawn", None) => {
                Err(invalid("needs a program, as in `process.spawn:rg`, or `*`"))
            }
            ("net", None) => Err(invalid(
                "needs a host, as in `net:api.anthropic.com`, or `*`",
            )),
            _ => Err(invalid("unknown capability")),
        }
    }
}

fn parse_scope(scope: &str) -> Result<FsScope, &'static str> {
    match scope {
        "workspace" => Ok(FsScope::Workspace),
        _ => Err("the only file scope is `workspace`"),
    }
}

fn parse_program(program: &str) -> Result<Target, &'static str> {
    if program == "*" {
        return Ok(Target::Any);
    }
    if program.is_empty() {
        return Err("the program is empty");
    }
    if program
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("a program name has no spaces or control characters");
    }
    Ok(Target::Named(program.to_string()))
}

/// A host name: `.`-separated labels of ASCII letters, digits, `-` and `_`,
/// lower-cased. A URL, a port or a path is refused rather than cut down to a
/// host, so a manifest says exactly what it reaches.
fn parse_host(host: &str) -> Result<Target, &'static str> {
    if host == "*" {
        return Ok(Target::Any);
    }
    if host.is_empty() {
        return Err("the host is empty");
    }
    if host.contains("://") {
        return Err("a host has no scheme");
    }
    if host.contains(':') {
        return Err("a host has no port");
    }
    if host.contains(['/', '?', '#']) {
        return Err("a host has no path");
    }
    if host.contains('@') {
        return Err("a host has no user name");
    }
    if host.contains('*') {
        return Err("`*` stands alone, as `net:*`; there are no partial wildcards");
    }
    if !host.is_ascii() {
        return Err("an international host is written in its ASCII (punycode) form");
    }
    if host
        .split('.')
        .any(|label| label.is_empty() || !label.bytes().all(is_host_byte))
    {
        return Err("a host is `.`-separated labels of letters, digits, `-` and `_`");
    }
    Ok(Target::Named(host.to_ascii_lowercase()))
}

fn is_host_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferRead => f.write_str("buffer.read"),
            Self::BufferWrite => f.write_str("buffer.write"),
            Self::Commands => f.write_str("commands"),
            Self::Decorations => f.write_str("decorations"),
            Self::Status => f.write_str("status"),
            Self::FsRead(scope) => write!(f, "fs.read:{scope}"),
            Self::FsWrite(scope) => write!(f, "fs.write:{scope}"),
            Self::ProcessSpawn(program) => write!(f, "process.spawn:{program}"),
            Self::Net(host) => write!(f, "net:{host}"),
        }
    }
}

impl fmt::Display for FsScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workspace => f.write_str("workspace"),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => f.write_str(name),
            Self::Any => f.write_str("*"),
        }
    }
}

impl FromStr for Capability {
    type Err = CapabilityError;

    fn from_str(capability: &str) -> Result<Self, Self::Err> {
        Self::parse(capability)
    }
}

impl TryFrom<String> for Capability {
    type Error = CapabilityError;

    fn try_from(capability: String) -> Result<Self, Self::Error> {
        Self::parse(&capability)
    }
}

impl From<Capability> for String {
    fn from(capability: Capability) -> Self {
        capability.to_string()
    }
}

/// The capabilities granted to one plugin. Serializes as a list of the
/// capability strings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Grants(BTreeSet<Capability>);

/// The hosts a plugin's grants reach: [`Grants::net_hosts`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetHosts<'a> {
    /// The hosts named by `net:<host>` grants, sorted.
    pub hosts: Vec<&'a str>,
    /// Whether `net:*` is granted, which reaches every host.
    pub any: bool,
}

impl Grants {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a grant; false if it was already granted.
    pub fn insert(&mut self, capability: Capability) -> bool {
        self.0.insert(capability)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether these grants cover `capability`: it is granted itself, or it
    /// names a host and `net:*` is granted, or a program and
    /// `process.spawn:*` is. Hosts compare without case.
    pub fn allows(&self, capability: &Capability) -> bool {
        match capability {
            Capability::Net(Target::Named(host)) => {
                self.0.contains(&Capability::Net(Target::Any))
                    || self
                        .0
                        .contains(&Capability::Net(Target::Named(host.to_ascii_lowercase())))
            }
            Capability::ProcessSpawn(Target::Named(_)) => {
                self.0.contains(&Capability::ProcessSpawn(Target::Any))
                    || self.0.contains(capability)
            }
            _ => self.0.contains(capability),
        }
    }

    /// The hosts named by `net:<host>` grants, and whether `net:*` is
    /// granted.
    pub fn net_hosts(&self) -> NetHosts<'_> {
        let mut net_hosts = NetHosts::default();
        for capability in &self.0 {
            match capability {
                Capability::Net(Target::Named(host)) => net_hosts.hosts.push(host),
                Capability::Net(Target::Any) => net_hosts.any = true,
                _ => {}
            }
        }
        net_hosts
    }
}

impl FromIterator<Capability> for Grants {
    fn from_iter<I: IntoIterator<Item = Capability>>(capabilities: I) -> Self {
        Self(capabilities.into_iter().collect())
    }
}

impl Extend<Capability> for Grants {
    fn extend<I: IntoIterator<Item = Capability>>(&mut self, capabilities: I) {
        self.0.extend(capabilities);
    }
}

impl<'a> IntoIterator for &'a Grants {
    type Item = &'a Capability;
    type IntoIter = std::collections::btree_set::Iter<'a, Capability>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
