// SPDX-License-Identifier: Apache-2.0

//! The host's secret-storage imports for plugins.
//!
//! A plugin's secrets are its own: `get`, `set` and `delete` reach only the entry
//! `ext.<plugin id>.<name>` in the app's service of the [`SecretStore`], and need no capability.
//! A name is `[a-z0-9_-]{1,64}`, so it has no dot and `a` with `b.c` can never collide with
//! `a.b` with `c`.
//!
//! The store can't list its entries, so an index file records each plugin's names, written
//! under a lock and atomically, and before the store is touched: a crash between the two leaves
//! a name with no entry, which [`PluginSecrets::delete_all`] deletes harmlessly, never an entry
//! nothing remembers. Uninstalling a plugin calls `delete_all`.
//!
//! A value is held as a [`Secret`] wherever it's held, so no `Debug`, log line or error carries
//! one. It reaches the plugin only as the return of `get`, never through the WASI environment,
//! which stays empty.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use kiln_core::secrets::{Secret, SecretError, SecretStore};
use kiln_l10n::tr;
use serde::{Deserialize, Serialize};

use crate::bindings::kiln::extension::secrets::Host;
use crate::bindings::{Error, HostState, unimplemented};

/// The longest name a plugin may give a secret.
pub const MAX_NAME_LEN: usize = 64;

/// The secrets slice of [`HostState`]: the plugin's [`PluginSecrets`]. The default has none, and
/// every import is unimplemented.
#[derive(Debug, Default)]
pub struct State {
    secrets: Option<PluginSecrets>,
}

impl State {
    pub fn new(secrets: PluginSecrets) -> Self {
        Self {
            secrets: Some(secrets),
        }
    }
}

/// What the index file holds: each plugin's secret names.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    plugins: BTreeMap<String, Vec<String>>,
}

/// One plugin's view of the secret store, and the index that lets all of them be deleted.
#[derive(Clone)]
pub struct PluginSecrets {
    store: Arc<dyn SecretStore>,
    service: String,
    plugin_id: String,
    index_path: PathBuf,
}

impl fmt::Debug for PluginSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginSecrets")
            .field("service", &self.service)
            .field("plugin_id", &self.plugin_id)
            .field("index_path", &self.index_path)
            .finish_non_exhaustive()
    }
}

impl PluginSecrets {
    /// Plugin `plugin_id`'s secrets in `store` under `service` (the app id), with the names
    /// of every plugin's secrets kept in the file `index_path`.
    pub fn new(
        store: Arc<dyn SecretStore>,
        service: impl Into<String>,
        plugin_id: impl Into<String>,
        index_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            store,
            service: service.into(),
            plugin_id: plugin_id.into(),
            index_path: index_path.into(),
        }
    }

    fn account(&self, name: &str) -> String {
        format!("ext.{}.{name}", self.plugin_id)
    }

    /// The secret named `name`, or `None`.
    pub fn get(&self, name: &str) -> Result<Option<String>, Error> {
        check_name(name)?;
        let secret = self
            .store
            .get(&self.service, &self.account(name))
            .map_err(store_error)?;
        Ok(secret.map(Secret::into_string))
    }

    /// Sets the secret `name`, recording the name first. The lock is held until the store has
    /// the value, so a delete can't slip between the two.
    pub fn set(&self, name: &str, value: &str) -> Result<(), Error> {
        check_name(name)?;
        let secret = Secret::new(value);
        let _lock = self.lock()?;
        let mut index = self.read_index()?;
        let names = index.plugins.entry(self.plugin_id.clone()).or_default();
        if names.iter().all(|known| known != name) {
            names.push(name.to_owned());
        }
        // Written even when the name is already there: a name visible in the index file is no
        // proof the write that put it there was synced, if that write failed after its rename.
        self.write_index(&index)?;
        self.store
            .set(&self.service, &self.account(name), secret.expose())
            .map_err(store_error)
    }

    /// Deletes the secret `name`; one that doesn't exist is fine.
    pub fn delete(&self, name: &str) -> Result<(), Error> {
        check_name(name)?;
        let _lock = self.lock()?;
        self.delete_locked(name)
    }

    /// [`delete`](Self::delete) with the lock held: the entry first, so a failure keeps the name.
    fn delete_locked(&self, name: &str) -> Result<(), Error> {
        self.store
            .delete(&self.service, &self.account(name))
            .map_err(store_error)?;
        let mut index = self.read_index()?;
        let Some(names) = index.plugins.get_mut(&self.plugin_id) else {
            return Ok(());
        };
        names.retain(|known| known != name);
        if names.is_empty() {
            index.plugins.remove(&self.plugin_id);
        }
        self.write_index(&index)
    }

    /// The names of the plugin's secrets, as the index has them.
    pub fn names(&self) -> Result<Vec<String>, Error> {
        let _lock = self.lock()?;
        let mut index = self.read_index()?;
        Ok(index.plugins.remove(&self.plugin_id).unwrap_or_default())
    }

    /// Deletes every secret of the plugin and its index row, all under one hold of the lock. On
    /// a failure the index keeps the names not yet deleted, so a retry finishes the job.
    pub fn delete_all(&self) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut index = self.read_index()?;
        for name in index
            .plugins
            .get(&self.plugin_id)
            .cloned()
            .unwrap_or_default()
        {
            self.delete_locked(&name)?;
        }
        index = self.read_index()?;
        if index.plugins.remove(&self.plugin_id).is_some() {
            self.write_index(&index)?;
        }
        Ok(())
    }

    /// Holds the index's lock file locked until the returned file is dropped.
    fn lock(&self) -> Result<File, Error> {
        let path = self.index_path.with_extension("lock");
        let open = || {
            if let Some(dir) = path.parent() {
                create_dir_durably(dir)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)?;
            file.lock()?;
            Ok::<_, io::Error>(file)
        };
        open().map_err(|error| index_error("lock", &path, &error))
    }

    fn read_index(&self) -> Result<Index, Error> {
        match fs::read_to_string(&self.index_path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|error| index_error("parse", &self.index_path, &error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Index::default()),
            Err(error) => Err(index_error("read", &self.index_path, &error)),
        }
    }

    fn write_index(&self, index: &Index) -> Result<(), Error> {
        let text = toml::to_string(index)
            .map_err(|error| index_error("serialize", &self.index_path, &error))?;
        write_atomically(&self.index_path, text.as_bytes())
            .map_err(|error| index_error("write", &self.index_path, &error))
    }
}

/// Writes `contents` to `path` through a file of its own, so a reader never sees half of it.
fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        sync_dir(path.parent().unwrap_or(Path::new(".")))
    })();
    if written.is_err() {
        drop(fs::remove_file(&temporary));
    }
    written
}

/// Flushes `dir`'s entries, so a rename or a new file in it survives a crash. Nothing to do on
/// Windows, where a folder doesn't open as a `File` and NTFS journals its entries itself.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(test)]
    if tests::take_fault(dir) {
        return Err(io::Error::other("injected sync failure"));
    }
    kiln_core::platform::sync_dir(dir)
}

/// Creates `dir` and its missing ancestors, flushing each entry into its parent. Every ancestor
/// is flushed on every call, existing or not: a directory that is visible may be one whose
/// earlier flush failed.
fn create_dir_durably(dir: &Path) -> io::Result<()> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    let Some(parent) = dir.parent() else {
        return Ok(());
    };
    create_dir_durably(parent)?;
    match fs::create_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => {}
        Err(error) => return Err(error),
    }
    sync_dir(parent)
}

fn check_name(name: &str) -> Result<(), Error> {
    let valid = (1..=MAX_NAME_LEN).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidArgument(
            tr!("kiln-plugin-secret-name", max = MAX_NAME_LEN).to_string(),
        ))
    }
}

fn store_error(error: SecretError) -> Error {
    match error {
        SecretError::Denied => Error::Denied(error.to_string()),
        error => Error::Failed(error.to_string()),
    }
}

/// `action` is what failed: `lock`, `parse`, `read`, `serialize` or `write`.
fn index_error(action: &'static str, path: &Path, error: &dyn fmt::Display) -> Error {
    Error::Failed(
        tr!(
            "kiln-plugin-secrets-index",
            action = action,
            path = path.display().to_string(),
            reason = error.to_string()
        )
        .to_string(),
    )
}

impl State {
    fn secrets(&self, import: &str) -> Result<&PluginSecrets, Error> {
        self.secrets
            .as_ref()
            .map_or_else(|| unimplemented(import), Ok)
    }
}

impl Host for HostState {
    fn get(&mut self, name: String) -> Result<Option<String>, Error> {
        self.secrets
            .secrets("kiln:extension/secrets.get")?
            .get(&name)
    }

    fn set(&mut self, name: String, value: String) -> Result<(), Error> {
        self.secrets
            .secrets("kiln:extension/secrets.set")?
            .set(&name, &value)
    }

    fn delete(&mut self, name: String) -> Result<(), Error> {
        self.secrets
            .secrets("kiln:extension/secrets.delete")?
            .delete(&name)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use kiln_core::secrets::MemoryStore;

    use super::*;

    thread_local! {
        static FAULT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    /// Whether the armed fault is for `dir`; it fires once.
    pub(super) fn take_fault(dir: &Path) -> bool {
        FAULT.with_borrow_mut(|fault| {
            if fault.as_deref() == Some(dir) {
                *fault = None;
                true
            } else {
                false
            }
        })
    }

    fn fail_next_sync_of(dir: &Path) {
        FAULT.with_borrow_mut(|fault| *fault = Some(dir.to_owned()));
    }

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("kiln-secrets-unit-{}-{name}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        root
    }

    #[test]
    fn a_retry_after_a_failed_sync_of_the_renamed_index_syncs_again() {
        let root = root("rename");
        fs::create_dir_all(&root).unwrap();
        let store = Arc::new(MemoryStore::default());
        let secrets = PluginSecrets::new(store.clone(), "svc", "p", root.join("index.toml"));
        fail_next_sync_of(&root);
        assert!(secrets.set("key", "v").is_err());
        assert_eq!(
            secrets.get("key").unwrap(),
            None,
            "no entry before the index is durable"
        );
        // The name is visible now; the retry must still write and sync the index.
        fail_next_sync_of(&root);
        assert!(
            secrets.set("key", "v").is_err(),
            "the retry synced the index again"
        );
        assert_eq!(secrets.get("key").unwrap(), None);
        secrets.set("key", "v").unwrap();
        assert_eq!(secrets.get("key").unwrap().as_deref(), Some("v"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_retry_after_a_failed_sync_of_a_new_directory_syncs_its_parent_again() {
        let root = root("dir");
        fs::create_dir_all(&root).unwrap();
        let store = Arc::new(MemoryStore::default());
        let secrets = PluginSecrets::new(store, "svc", "p", root.join("sub/index.toml"));
        fail_next_sync_of(&root);
        assert!(secrets.set("key", "v").is_err());
        assert!(root.join("sub").is_dir(), "the directory is visible");
        fail_next_sync_of(&root);
        assert!(
            secrets.set("key", "v").is_err(),
            "the retry synced the parent again"
        );
        assert_eq!(secrets.get("key").unwrap(), None);
        secrets.set("key", "v").unwrap();
        fs::remove_dir_all(&root).ok();
    }
}
