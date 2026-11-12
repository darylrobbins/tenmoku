// SPDX-License-Identifier: Apache-2.0

//! Paths across the plugin boundary: the host's own, and the guest's.
//!
//! A guest's libc and Rust's wasi `Path` split only on `/` and take only `/…` as absolute, so a
//! Windows path can't reach a plugin as it is. On Windows, host `C:\Users\d\work\a.rs` is guest
//! `/C:/Users/d/work/a.rs`, the path part of its `file:///C:/…` URI, so a plugin's paths and URIs
//! agree. The drive letter is upper-cased, as `kiln_core::platform::canonicalize` gives it. On
//! Unix the mapping is the identity.
//!
//! Every path crossing the boundary goes through here, both ways: the folders preopened for a
//! plugin ([`crate::fs::configure`]), the paths in events ([`crate::events`]) and in buffer
//! snapshots. A path that has no guest form never reaches a plugin as a host string. One the
//! guest names maps back through [`to_host_within`], which refuses it unless it lies inside a
//! preopened root: no import takes a path yet, and the first to (`navigation.open`) calls that.
//!
//! A network share (`\\server\share`) has no guest path, and neither has a root still in `\\?\`
//! form, as a root over 260 characters stays: neither is preopened, with a warning, until the
//! plugins stage 3 design's Q7 is settled (`docs/decisions/dqsjfernkt-plugins-stage-3.md`). A
//! file deep under a short root may be over 260 characters, and its `\\?\` is dropped.
//!
//! The functions are pure string code, taking the [`Rules`] to follow, so every OS tests both.

use std::fmt;
use std::path::{Path, PathBuf};

use kiln_core::platform::Os;
use kiln_l10n::tr;

use crate::bindings::Error;

/// How the host spells its paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rules {
    /// `/`-separated and rooted at `/`: guest paths are host paths.
    Unix,
    /// Under a drive letter, `\`- or `/`-separated, with the `\\?\` and `\\server\share`
    /// forms.
    Windows,
}

impl Rules {
    /// This host's.
    pub const HOST: Self = match Os::current() {
        Os::Windows => Self::Windows,
        Os::MacOs | Os::Linux => Self::Unix,
    };
}

/// The longest root, in UTF-16 units, that Windows opens without `\\?\`: `MAX_PATH`.
pub const MAX_ROOT: usize = 260;

/// Why a host path has no guest path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmapped {
    /// It isn't absolute, or on Windows not under a drive letter (as `\work` and `C:work`
    /// aren't).
    NotAbsolute,
    /// A network share: `\\server\share\…`, or `\\?\UNC\server\share\…`.
    Unc,
    /// A device path, `\\.\…`.
    Device,
    /// A root still in `\\?\` form, or over [`MAX_ROOT`].
    LongRoot,
}

impl fmt::Display for Unmapped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Self::NotAbsolute => "not-absolute",
            Self::Unc => "unc",
            Self::Device => "device",
            Self::LongRoot => "long-root",
        };
        write!(f, "{}", tr!("kiln-plugin-path-unmapped", why = why))
    }
}

/// Host path `host` as the guest sees it, under `rules`.
pub fn to_guest(host: &str, rules: Rules) -> Result<String, Unmapped> {
    match rules {
        Rules::Unix if host.starts_with('/') => Ok(host.to_owned()),
        Rules::Unix => Err(Unmapped::NotAbsolute),
        Rules::Windows => windows_guest(host),
    }
}

/// The guest path to preopen root `host` at, under `rules`: as [`to_guest`], but a Windows
/// root still in `\\?\` form, or over [`MAX_ROOT`], has none.
pub fn root_to_guest(host: &str, rules: Rules) -> Result<String, Unmapped> {
    if rules == Rules::Windows
        && !is_unc(host)
        && (host.starts_with(VERBATIM) || host.encode_utf16().count() > MAX_ROOT)
    {
        return Err(Unmapped::LongRoot);
    }
    to_guest(host, rules)
}

/// Guest path `guest` as the host spells it, under `rules`. Under Windows rules it must start
/// `/<letter>:` and hold no `\`; under either it must be absolute. Anything else is
/// `invalid-argument`. Whether it lies inside a root is the caller's to check.
pub fn to_host(guest: &str, rules: Rules) -> Result<String, Error> {
    let path = &format!("{guest:?}");
    let invalid = |why: kiln_l10n::SharedString| Err(Error::InvalidArgument(why.to_string()));
    match rules {
        Rules::Unix if guest.starts_with('/') => Ok(guest.to_owned()),
        Rules::Unix => invalid(tr!("kiln-plugin-path-not-absolute", path = path)),
        Rules::Windows if guest.contains('\\') => {
            invalid(tr!("kiln-plugin-path-backslash", path = path))
        }
        Rules::Windows => {
            let Some((drive, rest)) = guest.strip_prefix('/').and_then(drive) else {
                return invalid(tr!("kiln-plugin-path-no-drive", path = path));
            };
            Ok(format!("{drive}:\\{}", rest.replace('/', "\\")))
        }
    }
}

/// Guest path `guest` as the host spells it, if it lies inside one of `roots`, host paths as
/// preopened: the root context rule, for an import that takes a path. [`to_host`]'s checks
/// come first; then a path with a `.` or `..` part is `invalid-argument`, and one outside every
/// root with a guest path is `denied`, never passed through. The check reads the words alone:
/// a caller that opens the file still resolves its links inside the root.
pub fn to_host_within<S: AsRef<str>>(
    guest: &str,
    rules: Rules,
    roots: impl IntoIterator<Item = S>,
) -> Result<String, Error> {
    let host = to_host(guest, rules)?;
    let path = &format!("{guest:?}");
    if guest.split('/').any(|part| part == "." || part == "..") {
        let why = tr!("kiln-plugin-path-dot", path = path);
        return Err(Error::InvalidArgument(why.to_string()));
    }
    // With its drive letter upper-cased, as the roots' are.
    let normal = to_guest(&host, rules).unwrap_or_else(|_| guest.to_owned());
    let inside = (roots.into_iter())
        .filter_map(|root| root_to_guest(root.as_ref(), rules).ok())
        .any(|root| {
            let root = root.trim_end_matches('/');
            (normal.strip_prefix(root)).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        });
    if !inside {
        let why = tr!("kiln-plugin-path-outside-roots", path = path);
        return Err(Error::Denied(why.to_string()));
    }
    Ok(host)
}

/// [`to_guest`] for a buffer's or a file's path: none for one that has no guest path, with a
/// debug log, as such a path shows up on every event of its buffer.
pub fn guest(host: &str, rules: Rules) -> Option<String> {
    to_guest(host, rules)
        .inspect_err(|why| log::debug!("not giving plugins the path {host}: {why}"))
        .ok()
}

/// [`root_to_guest`] for a root to preopen: none, with a warning, for one that isn't UTF-8 or
/// has no guest path.
pub fn root(host: &Path, rules: Rules) -> Option<String> {
    let Some(text) = host.to_str() else {
        log::warn!(
            "not preopening {} for plugins: not a UTF-8 path",
            host.display()
        );
        return None;
    };
    root_to_guest(text, rules)
        .inspect_err(|why| log::warn!("not preopening {text} for plugins: {why}"))
        .ok()
}

/// [`to_host`] under this host's rules.
pub fn host(guest: &str) -> Result<PathBuf, Error> {
    to_host(guest, Rules::HOST).map(PathBuf::from)
}

const VERBATIM: &str = r"\\?\";

/// Whether `host` names a network share, in either form.
fn is_unc(host: &str) -> bool {
    match host.strip_prefix(VERBATIM) {
        Some(rest) => rest
            .get(..4)
            .is_some_and(|unc| unc.eq_ignore_ascii_case(r"UNC\")),
        None => {
            let mut chars = host.chars();
            let separator = |c: Option<char>| matches!(c, Some('\\' | '/'));
            separator(chars.next())
                && separator(chars.next())
                && !host.starts_with(r"\\.\")
                && !host.starts_with("//./")
        }
    }
}

fn windows_guest(host: &str) -> Result<String, Unmapped> {
    if is_unc(host) {
        return Err(Unmapped::Unc);
    }
    if host.starts_with(r"\\.\") || host.starts_with("//./") {
        return Err(Unmapped::Device);
    }
    let path = host.strip_prefix(VERBATIM).unwrap_or(host);
    let (drive, rest) = drive(path).ok_or(Unmapped::NotAbsolute)?;
    Ok(format!("/{drive}:/{}", rest.replace('\\', "/")))
}

/// `path`'s drive letter, upper-cased, and what follows its separator, if it starts with a
/// letter, `:` and a separator. A bare `D:` is not a root: on Windows it names the current
/// directory on that drive.
fn drive(path: &str) -> Option<(char, &str)> {
    let mut chars = path.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str();
    let rest = rest.strip_prefix(['\\', '/'])?;
    Some((letter.to_ascii_uppercase(), rest))
}
