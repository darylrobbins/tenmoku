//! Plugins in the signed index (plugins stage 3, section 4.6,
//! `docs/decisions/dqsjfernkt-plugins-stage-3.md`).
//!
//! A plugin is one archive, listed in [`crate::grammars::Index::plugins`]
//! with its id, version, the extension API minor it needs, the archive's
//! SHA-256, size and entry count, and a summary of the capabilities its
//! manifest asks for. The index is signed whole, as grammars' is, so the
//! entry is only as good as the signature: [`verified_entry`] checks the
//! signature, refuses a replay, and only then reads the entry. An install
//! then goes through [`PluginEntry::verify_archive`] (the archive's bytes
//! are the entry's hash) and [`PluginEntry::verify_capabilities`] (its
//! manifest asks for what the entry says) before anything is unpacked.
//!
//! There are no per-author keys: the registry's signature vouches for the
//! bytes it reviewed.

use serde::{Deserialize, Serialize};

use crate::{
    grammars::{Index, is_sha256, sha256_hex},
    update_signature::PublicKey,
};

/// The most bytes a listed archive may have. An entry above it is not an
/// index the app takes, so a signed index can't make the app download more.
pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// The most entries a listed archive may have, folders included.
pub const MAX_ARCHIVE_ENTRIES: u32 = 10_000;

/// The most capabilities an entry may summarise.
pub const MAX_CAPABILITIES: usize = 256;

/// One plugin in the index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEntry {
    /// The extension's id, which is also its key.
    pub id: String,
    pub name: String,
    pub version: String,
    /// The minor of the kiln extension API 1 the plugin needs.
    pub api_minor: u32,
    /// The extension's SPDX licence.
    pub license: String,
    /// The archive's file name, plain, ending in `.zip` or `.tar.gz`.
    pub file: String,
    /// The archive's SHA-256, in lowercase hex.
    pub sha256: String,
    /// The archive's size in bytes, at most [`MAX_ARCHIVE_BYTES`].
    pub size: u64,
    /// How many entries the archive holds, at most [`MAX_ARCHIVE_ENTRIES`].
    pub entries: u32,
    /// The manifest's `[contributes.plugin] capabilities`, as written.
    pub capabilities: Vec<String>,
}

/// Why an install was refused before it was unpacked.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("the index's signature: {0}")]
    Signature(String),
    #[error("the index: {0}")]
    Index(String),
    #[error("the index's serial {found} is lower than the cached {cached}")]
    Rollback { found: u64, cached: u64 },
    #[error("the index lists no plugin {0:?}")]
    NotListed(String),
    #[error("the archive has {found} bytes; the index lists {listed}")]
    Size { listed: u64, found: u64 },
    #[error("the archive's SHA-256 is {found}; the index lists {listed}")]
    Hash { listed: String, found: String },
    #[error("the manifest asks for {0:?}, which the index doesn't list")]
    UnlistedCapability(String),
}

impl PluginEntry {
    /// Checks the entry holds together, under its key `key`: that id, its
    /// hash, its file name and the caps on size, entries and capabilities.
    pub fn check(&self, key: &str) -> Result<(), String> {
        let bad = |message: &str| Err(format!("the plugin {key:?}: {message}"));
        if self.id != key || self.id.is_empty() {
            return bad("its id isn't its key");
        }
        if !is_sha256(&self.sha256) {
            return bad("its sha256 isn't SHA-256 hex");
        }
        let plain = !self.file.is_empty()
            && !self.file.starts_with('.')
            && !self.file.contains(['/', '\\']);
        if !plain || !(self.file.ends_with(".zip") || self.file.ends_with(".tar.gz")) {
            return bad("its file isn't a plain .zip or .tar.gz name");
        }
        self.check_limits().or_else(|message| bad(&message))?;
        if self.capabilities.len() > MAX_CAPABILITIES {
            return bad("it lists too many capabilities");
        }
        Ok(())
    }

    /// The caps on the archive: what an app checks before it downloads, so
    /// it reads at most [`PluginEntry::size`] bytes.
    pub fn check_limits(&self) -> Result<(), String> {
        if self.size == 0 || self.size > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "its size {} isn't 1 to {MAX_ARCHIVE_BYTES} bytes",
                self.size
            ));
        }
        if self.entries == 0 || self.entries > MAX_ARCHIVE_ENTRIES {
            return Err(format!(
                "its {} entries aren't 1 to {MAX_ARCHIVE_ENTRIES}",
                self.entries
            ));
        }
        Ok(())
    }

    /// Checks `archive`, as downloaded, is the one listed: its size, then
    /// its hash. Nothing is unpacked before this passes.
    pub fn verify_archive(&self, archive: &[u8]) -> Result<(), Refused> {
        self.check_limits().map_err(Refused::Index)?;
        let found = archive.len() as u64;
        if found != self.size {
            return Err(Refused::Size {
                listed: self.size,
                found,
            });
        }
        let hash = sha256_hex(archive);
        if hash != self.sha256 {
            return Err(Refused::Hash {
                listed: self.sha256.clone(),
                found: hash,
            });
        }
        Ok(())
    }

    /// Checks the capabilities the archive's manifest asks for are all in
    /// the entry's list, which is what the install prompt shows. It may ask
    /// for fewer.
    pub fn verify_capabilities<S: AsRef<str>>(&self, manifest: &[S]) -> Result<(), Refused> {
        match manifest
            .iter()
            .find(|asked| !self.capabilities.iter().any(|c| c == asked.as_ref()))
        {
            Some(asked) => Err(Refused::UnlistedCapability(asked.as_ref().to_owned())),
            None => Ok(()),
        }
    }
}

/// The entry for `id` in `index`, which is `signature`'s (the base64 of the
/// signature) exact bytes under `public_key` (the base64 of the key).
///
/// The signature is checked before the index is parsed, so a tampered index
/// is never read; an index whose serial is below `cached_serial`, a replay
/// of an older signed one, is refused. Returns the entry with the index's
/// serial, to cache.
pub fn verified_entry(
    index: &[u8],
    signature: &str,
    public_key: &str,
    cached_serial: Option<u64>,
    id: &str,
) -> Result<(PluginEntry, u64), Refused> {
    let key = PublicKey::from_base64(public_key)
        .map_err(|error| Refused::Signature(error.to_string()))?;
    key.verify(index, signature)
        .map_err(|error| Refused::Signature(error.to_string()))?;
    let text = std::str::from_utf8(index).map_err(|_| Refused::Index("it isn't UTF-8".into()))?;
    let index = Index::parse(text).map_err(|error| Refused::Index(error.to_string()))?;
    if let Some(cached) = cached_serial
        && index.serial < cached
    {
        return Err(Refused::Rollback {
            found: index.serial,
            cached,
        });
    }
    let entry = index
        .plugins
        .get(id)
        .ok_or_else(|| Refused::NotListed(id.to_owned()))?;
    Ok((entry.clone(), index.serial))
}

/// Everything an install checks before it unpacks: [`verified_entry`], then
/// that `archive` is the entry's ([`PluginEntry::verify_archive`]). Returns
/// the entry, whose capabilities the install then holds the manifest to
/// ([`PluginEntry::verify_capabilities`]), and the index's serial, to
/// cache.
pub fn verified_install(
    index: &[u8],
    signature: &str,
    public_key: &str,
    cached_serial: Option<u64>,
    id: &str,
    archive: &[u8],
) -> Result<(PluginEntry, u64), Refused> {
    let (entry, serial) = verified_entry(index, signature, public_key, cached_serial, id)?;
    entry.verify_archive(archive)?;
    Ok((entry, serial))
}
