//! Where a window's project is (docs/decisions/j8wvgszah0-remote-development.md
//! § 6): a folder on this machine, or one on a remote host, and the text
//! form a location is written down in.
//!
//! A remote location's text form is `ssh://[user@]host[:port]/abs/path`,
//! the path's bytes percent-encoded where they aren't URL-safe, so any
//! POSIX path, non-UTF-8 ones too, is ASCII text. A local one keeps its
//! plain path, so the files that held paths before there were locations
//! (the recent list, the saved layouts) read as they did: no absolute
//! local path starts `ssh://`.

use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

use crate::HostSet;

/// What a remote location's text form starts with.
pub const SSH_SCHEME: &str = "ssh://";

/// How a remote host is reached.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum HostKind {
    /// Over ssh, as `ssh` itself would reach the destination.
    Ssh,
}

/// A remote host: how it's reached, and its destination as typed (`neon`,
/// `me@box:2222`), which a bare ssh-config alias may be.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostId {
    kind: HostKind,
    user: Option<String>,
    host: String,
    port: Option<u16>,
}

impl HostId {
    /// The ssh host `destination`, `[user@]host[:port]`: a user and host of
    /// letters, digits, `.`, `_`, `~` and `-` (not first, so it's never
    /// taken for an option), or an IPv6 address in brackets, and a port
    /// from 1 to 65535 with no leading zero.
    pub fn ssh(destination: &str) -> Result<Self, LocationError> {
        let (user, rest) = match destination.split_once('@') {
            Some((user, rest)) => (Some(user), rest),
            None => (None, destination),
        };
        let (host, port) = split_port(rest)?;
        if let Some(user) = user
            && !is_name(user)
        {
            return Err(LocationError::Host);
        }
        let named = is_name(host);
        let bracketed = (host.strip_prefix('['))
            .and_then(|host| host.strip_suffix(']'))
            .is_some_and(|address| address.parse::<std::net::Ipv6Addr>().is_ok());
        if !named && !bracketed {
            return Err(LocationError::Host);
        }
        Ok(Self {
            kind: HostKind::Ssh,
            user: user.map(str::to_owned),
            host: host.to_owned(),
            port,
        })
    }

    pub fn kind(&self) -> HostKind {
        self.kind
    }

    /// The user to sign in as, if one was given.
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    /// The host name, ssh-config alias or bracketed address.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// The destination as typed: `[user@]host[:port]`.
    pub fn destination(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(user) = &self.user {
            write!(f, "{user}@")?;
        }
        f.write_str(&self.host)?;
        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }
        Ok(())
    }
}

/// A user or host name: see [`HostId::ssh`].
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && (name.bytes())
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
}

/// `rest` split at its `:port`, if it has one: after a bracketed address's
/// `]`, or at its one colon.
fn split_port(rest: &str) -> Result<(&str, Option<u16>), LocationError> {
    let colon = match rest.strip_prefix('[') {
        Some(_) => {
            let close = rest.find(']').ok_or(LocationError::Host)?;
            match &rest[close + 1..] {
                "" => None,
                after if after.starts_with(':') => Some(close + 1),
                _ => return Err(LocationError::Host),
            }
        }
        None => rest.find(':'),
    };
    let Some(colon) = colon else {
        return Ok((rest, None));
    };
    let digits = &rest[colon + 1..];
    let port = (!digits.is_empty() && !digits.starts_with('0'))
        .then_some(digits)
        .filter(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u16>().ok())
        .ok_or(LocationError::Port)?;
    Ok((&rest[..colon], Some(port)))
}

/// An absolute POSIX path on a remote host, as its bytes: any bytes but
/// NUL, starting with `/`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemotePath(Vec<u8>);

impl RemotePath {
    /// `bytes` as a remote path, if they're absolute and hold no NUL.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Option<Self> {
        let bytes = bytes.into();
        (bytes.first() == Some(&b'/') && !bytes.contains(&0)).then_some(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The last component, trailing slashes aside; `None` for `/`.
    pub fn file_name(&self) -> Option<&[u8]> {
        let path = without_trailing_slashes(&self.0);
        let start = path.iter().rposition(|&b| b == b'/')? + 1;
        Some(&path[start..])
    }

    /// The folder it's in, trailing slashes aside; `None` for `/`.
    pub fn parent(&self) -> Option<RemotePath> {
        let path = without_trailing_slashes(&self.0);
        let slash = path.iter().rposition(|&b| b == b'/')?;
        let parent = without_trailing_slashes(&path[..slash]);
        Some(RemotePath(match parent {
            [] => b"/".to_vec(),
            parent => parent.to_vec(),
        }))
    }
}

fn without_trailing_slashes(path: &[u8]) -> &[u8] {
    let end = path.len() - (path.iter().rev()).take_while(|&&b| b == b'/').count();
    &path[..end]
}

/// Shows the path as text, a byte that isn't UTF-8 as U+FFFD.
impl fmt::Display for RemotePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

/// Where a window's project is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProjectLocation {
    /// A folder on this machine.
    Local(PathBuf),
    /// A folder on a remote host.
    Remote { host: HostId, path: RemotePath },
}

impl ProjectLocation {
    /// The remote location `text` writes, which must be exactly
    /// `ssh://[user@]host[:port]/abs/path` (see [`HostId::ssh`]), with no
    /// query or fragment; `%` and two hex digits in the path is the byte
    /// they spell.
    pub fn parse(text: &str) -> Result<Self, LocationError> {
        let rest = text.strip_prefix(SSH_SCHEME).ok_or(LocationError::NotSsh)?;
        if rest.contains(['?', '#']) {
            return Err(LocationError::QueryOrFragment);
        }
        let slash = rest.find('/').ok_or(LocationError::Path)?;
        let host = HostId::ssh(&rest[..slash])?;
        let path = RemotePath::new(decode(&rest[slash..])?).ok_or(LocationError::Path)?;
        Ok(ProjectLocation::Remote { host, path })
    }

    /// The location a text form, as [`ProjectLocation::text`] writes it,
    /// names: one that starts `ssh://` and parses is remote, and anything
    /// else is a local path.
    pub fn from_text(text: &OsStr) -> Self {
        let remote = (text.to_str())
            .filter(|text| text.starts_with(SSH_SCHEME))
            .and_then(|text| Self::parse(text).ok());
        remote.unwrap_or_else(|| ProjectLocation::Local(PathBuf::from(text)))
    }

    /// Whether `text` is written as a remote location, parsing or not: what
    /// a command-line argument that's no local path looks like.
    pub fn is_remote_text(text: &OsStr) -> bool {
        text.as_encoded_bytes().starts_with(SSH_SCHEME.as_bytes())
    }

    /// The text form: a local location's path, unchanged, or a remote one's
    /// `ssh://` URI.
    pub fn text(&self) -> OsString {
        match self {
            ProjectLocation::Local(path) => path.clone().into_os_string(),
            remote => remote.to_string().into(),
        }
    }

    /// The folder, if it's on this machine.
    pub fn local(&self) -> Option<&Path> {
        match self {
            ProjectLocation::Local(path) => Some(path),
            ProjectLocation::Remote { .. } => None,
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, ProjectLocation::Remote { .. })
    }

    /// The hosts a window on this location works through: this machine's
    /// for a local one. There's no remote host yet, so a remote location
    /// can't be opened.
    pub fn hosts(&self) -> Result<HostSet, RemoteUnsupported> {
        match self {
            ProjectLocation::Local(_) => Ok(HostSet::local()),
            ProjectLocation::Remote { .. } => Err(RemoteUnsupported),
        }
    }
}

/// A local location shows its path, and a remote one its `ssh://` URI,
/// which [`ProjectLocation::parse`] reads back.
impl fmt::Display for ProjectLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectLocation::Local(path) => path.display().fmt(f),
            ProjectLocation::Remote { host, path } => {
                write!(f, "{SSH_SCHEME}{host}")?;
                encode(path.as_bytes(), f)
            }
        }
    }
}

impl From<PathBuf> for ProjectLocation {
    fn from(path: PathBuf) -> Self {
        ProjectLocation::Local(path)
    }
}

/// A local location is the path it holds.
impl PartialEq<Path> for ProjectLocation {
    fn eq(&self, other: &Path) -> bool {
        self.local() == Some(other)
    }
}

impl PartialEq<&Path> for ProjectLocation {
    fn eq(&self, other: &&Path) -> bool {
        *self == **other
    }
}

impl PartialEq<PathBuf> for ProjectLocation {
    fn eq(&self, other: &PathBuf) -> bool {
        *self == *other.as_path()
    }
}

/// Whether `byte` is written as itself in a path: RFC 3986's unreserved
/// characters, its sub-delims, `:`, `@` and `/`.
fn url_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&byte)
}

fn encode(bytes: &[u8], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for &byte in bytes {
        match url_safe(byte) {
            true => fmt::Write::write_char(f, char::from(byte))?,
            false => write!(f, "%{byte:02X}")?,
        }
    }
    Ok(())
}

fn decode(text: &str) -> Result<Vec<u8>, LocationError> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'%' {
            let hex = (bytes.get(at + 1..at + 3))
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or(LocationError::Escape)?;
            decoded.push(u8::from_str_radix(hex, 16).map_err(|_| LocationError::Escape)?);
            at += 3;
        } else if url_safe(byte) {
            decoded.push(byte);
            at += 1;
        } else {
            return Err(LocationError::Unescaped);
        }
    }
    Ok(decoded)
}

/// Why [`ProjectLocation::parse`] refused a text.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LocationError {
    /// It doesn't start `ssh://`.
    NotSsh,
    /// The user or host isn't one ssh can be given.
    Host,
    /// The port isn't a number from 1 to 65535.
    Port,
    /// There's no path after the host, or it holds a NUL.
    Path,
    /// A `%` not followed by two hex digits.
    Escape,
    /// A byte in the path that should have been percent-encoded.
    Unescaped,
    /// A `?` or `#`, which a location has no use for.
    QueryOrFragment,
}

impl fmt::Display for LocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LocationError::NotSsh => "not an ssh:// location",
            LocationError::Host => "not a host ssh can reach",
            LocationError::Port => "not a port number",
            LocationError::Path => "no absolute path after the host",
            LocationError::Escape => "a % not followed by two hex digits",
            LocationError::Unescaped => "a character in the path that should be percent-encoded",
            LocationError::QueryOrFragment => "a ? or # after the path",
        })
    }
}

impl std::error::Error for LocationError {}

/// Why a remote location didn't open: there's no remote host support yet.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RemoteUnsupported;

impl fmt::Display for RemoteUnsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("remote folders aren't supported yet")
    }
}

impl std::error::Error for RemoteUnsupported {}

#[cfg(test)]
#[path = "location_tests.rs"]
mod tests;
