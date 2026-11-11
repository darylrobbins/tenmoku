// SPDX-License-Identifier: Apache-2.0

//! The cache of compiled components: wasmtime can serialize a compiled component and load it
//! back, which skips compiling it on every launch.
//!
//! An entry is keyed by the SHA-256 of the component's bytes, wasmtime's version and the
//! engine's config fingerprint. Deserializing is `unsafe`: wasmtime trusts the bytes to be
//! something it serialized. So an entry is loaded only from a directory and file the user owns
//! that no one else can write, and only if wasmtime recognises it as a precompiled component
//! for this engine. Anything else is recompiled and replaced, never an error: the cache only
//! skips compilation, and the grant check stays before any load.

use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::SystemTime,
};

use kiln_core::platform;
use sha2::{Digest, Sha256};
use wasmtime::{Engine, Precompiled, component::Component};

/// How many entries the cache keeps; the oldest go first.
pub const MAX_ENTRIES: usize = 64;

/// The bytes before an entry's payload: its key, then the SHA-256 of the payload.
const HEADER: usize = 64;

/// The extension of an entry's file.
const EXTENSION: &str = "cwasm";

/// Serialized compiled components in one directory.
pub struct ComponentCache {
    dir: PathBuf,
    /// Who owns what we create here, learnt by creating a file. `None` when the directory is
    /// not writable: then nothing in it is trusted.
    owner: Option<platform::Owner>,
    compiled: AtomicUsize,
}

impl ComponentCache {
    /// A cache in `dir`, which is created, readable by the user alone, if it is missing.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let owner = platform::create_private_dir(&dir)
            .and_then(|()| probe(&dir))
            .ok();
        Self {
            dir,
            owner,
            compiled: AtomicUsize::new(0),
        }
    }

    /// How many times [`load`](Self::load) has compiled rather than deserialized.
    pub fn compiled(&self) -> usize {
        self.compiled.load(Ordering::Relaxed)
    }

    /// Where `engine` keeps the entry for `bytes`.
    pub fn entry_path(&self, engine: &Engine, bytes: &[u8]) -> PathBuf {
        let name: String = key(engine, bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.dir.join(name).with_extension(EXTENSION)
    }

    /// The component `bytes` compile to on `engine`: deserialized from its entry if that is
    /// ours and fits `engine`, otherwise compiled and the entry replaced.
    pub fn load(&self, engine: &Engine, bytes: &[u8]) -> wasmtime::Result<Component> {
        let path = self.entry_path(engine, bytes);
        let key = key(engine, bytes);
        if let Some(component) = self.cached(engine, &path, &key) {
            return Ok(component);
        }
        self.compiled.fetch_add(1, Ordering::Relaxed);
        let component = Component::new(engine, bytes)?;
        if let Ok(serialized) = component.serialize()
            && let Err(error) = self.store(&path, &key, &serialized)
        {
            log::debug!("not caching {}: {error}", path.display());
        }
        Ok(component)
    }

    fn cached(&self, engine: &Engine, path: &Path, key: &[u8; 32]) -> Option<Component> {
        let owner = self.owner.as_ref()?;
        if !platform::owned_and_unshared(&platform::open_dir(&self.dir).ok()?, owner) {
            return None;
        }
        // Without blocking: a FIFO put at the path is a miss, not a hang.
        let mut file = platform::open_for_reading(path).ok()?;
        // The opened file, so a swap after the check changes nothing.
        if !file.metadata().ok()?.is_file() || !platform::owned_and_unshared(&file, owner) {
            return None;
        }
        let mut entry = Vec::new();
        file.read_to_end(&mut entry).ok()?;
        // The envelope: this entry's key, then the digest of the payload. Ownership and access
        // say who wrote the file, not that it is whole, and wasmtime's checks on the payload
        // are of its format, not its code.
        let (header, serialized) = entry.split_at_checked(HEADER)?;
        if header[..32] != key[..] || header[32..] != Sha256::digest(serialized)[..] {
            return None;
        }
        if Engine::detect_precompiled(serialized) != Some(Precompiled::Component) {
            return None;
        }
        // SAFETY: the bytes come from a file only the user can have written, in a directory only
        // the user can write to, they match the digest stored with them, and wasmtime has
        // just recognised them as a precompiled component. `deserialize` also checks them against `engine`'s version and config, and
        // fails, which we treat as a miss, for an entry made by another.
        unsafe { Component::deserialize(engine, serialized) }.ok()
    }

    /// Writes `serialized` to `path` through a file of its own, so a reader never sees half an
    /// entry, then prunes the directory.
    fn store(&self, path: &Path, key: &[u8; 32], serialized: &[u8]) -> io::Result<()> {
        let owner = self
            .owner
            .as_ref()
            .ok_or_else(|| io::Error::other("directory not writable"))?;
        if !platform::owned_and_unshared(&platform::open_dir(&self.dir)?, owner) {
            return Err(io::Error::other("directory is writable by others"));
        }
        // A name of its own, removed only once created, so overlapping stores never share or
        // unlink each other's file.
        let (temporary, mut file) = create_unique(&self.dir, ".tmp")?;
        let written = file
            .write_all(key)
            .and_then(|()| file.write_all(&Sha256::digest(serialized)))
            .and_then(|()| file.write_all(serialized))
            .and_then(|()| fs::rename(&temporary, path));
        if written.is_err() {
            drop(fs::remove_file(&temporary));
        }
        written?;
        self.prune();
        Ok(())
    }

    /// Removes the oldest entries beyond [`MAX_ENTRIES`].
    fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut entries: Vec<(SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == EXTENSION))
            .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
            .collect();
        if entries.len() <= MAX_ENTRIES {
            return;
        }
        entries.sort();
        let excess = entries.len() - MAX_ENTRIES;
        for (_, path) in entries.into_iter().take(excess) {
            drop(fs::remove_file(path));
        }
    }
}

/// A file made in `dir`, which says whose files are ours there.
fn probe(dir: &Path) -> io::Result<platform::Owner> {
    let (path, file) = create_unique(dir, ".probe")?;
    let owner = platform::Owner::of(&file);
    drop(file);
    drop(fs::remove_file(path));
    owner
}

/// Creates a new private file in `dir` under a name no other call, in this process or another,
/// is using.
fn create_unique(dir: &Path, prefix: &str) -> io::Result<(PathBuf, File)> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = dir.join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let file = platform::private(OpenOptions::new().write(true).create_new(true)).open(&path)?;
    Ok((path, file))
}

/// The key of `bytes` compiled on `engine`: its content, and wasmtime's version and every
/// setting that changes compiled code, which the compatibility hash covers.
fn key(engine: &Engine, bytes: &[u8]) -> [u8; 32] {
    let mut hasher = DefaultHasher::new();
    engine.precompile_compatibility_hash().hash(&mut hasher);
    Sha256::new()
        .chain_update(Sha256::digest(bytes))
        .chain_update(hasher.finish().to_le_bytes())
        .finalize()
        .into()
}
