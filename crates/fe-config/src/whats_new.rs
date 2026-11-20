//! Which version's What's New this machine has seen, for the What's New tab
//! that opens once on the first launch of a new version.
//!
//! The marker is `<state_dir>/whats_new.toml`, `seen = "X.Y.Z"`: state, not
//! settings, as the setup wizard's `setup.toml` is ([`crate::first_run`]).
//! [`decide`] reads only; [`record`] writes the marker atomically. Whether
//! the machine is new, which a missing marker alone doesn't say, is the
//! caller's to know.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

/// The marker's file name, in the state directory.
pub const MARKER_FILE: &str = "whats_new.toml";

/// The marker's one key: the version whose notes were seen.
const SEEN_KEY: &str = "seen";

/// The extension of the temporary file [`record`] writes first.
const TMP_EXTENSION: &str = "tmp";

/// What to do about the What's New tab at launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhatsNew {
    /// The marker is older than the running version, or missing or
    /// unreadable: show it (on a machine that isn't new).
    Show,
    /// The marker is the running version or newer (a downgrade): show
    /// nothing, and keep the marker as it is.
    Seen,
}

/// Decides whether `running`'s What's New is still to show, by the marker
/// in `state_dir`. Reads only. A marker that can't be read or doesn't hold a
/// version counts as none.
pub fn decide(state_dir: &Path, running: &str) -> WhatsNew {
    let marker = state_dir.join(MARKER_FILE);
    let text = match fs::read_to_string(&marker) {
        Ok(text) => text,
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                log::warn!("{}: {error}", marker.display());
            }
            return WhatsNew::Show;
        }
    };
    let seen = (text.parse::<toml::Table>().ok())
        .and_then(|table| table.get(SEEN_KEY)?.as_str().and_then(parse_version));
    let Some(seen) = seen else {
        log::warn!("{} holds no version", marker.display());
        return WhatsNew::Show;
    };
    match parse_version(running) {
        Some(running) if seen < running => WhatsNew::Show,
        Some(_) => WhatsNew::Seen,
        // Not a release's version: nothing to compare it with.
        None => WhatsNew::Seen,
    }
}

/// `MAJOR.MINOR.PATCH` as numbers, which compare as versions do: each part is
/// plain digits, with no sign or leading zero, and fits. The rules of Check
/// for Updates' (fe-workbench's `update_check`).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let mut part = || {
        let part = parts.next()?;
        let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
        let plain = part == "0" || !part.starts_with('0');
        (digits && plain).then(|| part.parse().ok()).flatten()
    };
    let version = (part()?, part()?, part()?);
    parts.next().is_none().then_some(version)
}

/// A release's version: `X.Y.Z`, or `X.Y.Z-preview.N`, which is older than
/// its own `X.Y.Z` and newer than the stable before it. The derived order
/// follows from `pre` being compared last, with a preview below no preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The preview's number, or `None` for a stable release.
    pub pre: Option<u64>,
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let core = |v: &Self| (v.major, v.minor, v.patch);
        core(self)
            .cmp(&core(other))
            .then_with(|| match (self.pre, other.pre) {
                (None, None) => std::cmp::Ordering::Equal,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.cmp(&b),
            })
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// `X.Y.Z` or `X.Y.Z-preview.N`, each number by [`parse_version`]'s rules.
pub fn parse_release_version(text: &str) -> Option<ReleaseVersion> {
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre.strip_prefix("preview.")?)),
        None => (text, None),
    };
    let (major, minor, patch) = parse_version(core)?;
    // `parse_version` on "N.0.0" holds N to plain digits.
    let pre = match pre {
        Some(n) => Some(parse_version(&format!("{n}.0.0"))?.0),
        None => None,
    };
    Some(ReleaseVersion {
        major,
        minor,
        patch,
        pre,
    })
}

/// Writes the marker, `seen = "<version>"`, creating the state directory;
/// a later call replaces it.
pub fn record(state_dir: &Path, version: &str) -> io::Result<()> {
    fs::create_dir_all(state_dir)?;
    let mut table = toml::Table::new();
    table.insert(SEEN_KEY.to_owned(), version.into());
    let text = toml::to_string(&table).map_err(io::Error::other)?;
    // A unique name made exclusively (`create_new` does not follow a planted
    // symlink), so concurrent records never share a temp file.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (tmp, mut file) = loop {
        let tmp = state_dir.join(format!(
            "{MARKER_FILE}.{}.{}.{TMP_EXTENSION}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let written = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, state_dir.join(MARKER_FILE))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}
