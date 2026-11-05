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
            self.write_index(&index)?;
        }
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

/// Flushes `dir`'s entries, so a rename or a new file in it survives a crash.
fn sync_dir(dir: &Path) -> io::Result<()> {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    File::open(dir)?.sync_all()
}

/// Creates `dir` and its missing ancestors, flushing each new entry into its parent.
fn create_dir_durably(dir: &Path) -> io::Result<()> {
    if dir.as_os_str().is_empty() || dir.is_dir() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        create_dir_durably(parent)?;
    }
    match fs::create_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    sync_dir(dir.parent().unwrap_or(Path::new(".")))
}

fn check_name(name: &str) -> Result<(), Error> {
    let valid = (1..=MAX_NAME_LEN).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidArgument(format!(
            "a secret's name is 1 to {MAX_NAME_LEN} characters of a-z, 0-9, _ and -"
        )))
    }
}

fn store_error(error: SecretError) -> Error {
    match error {
        SecretError::Denied => Error::Denied(error.to_string()),
        error => Error::Failed(error.to_string()),
    }
}

fn index_error(action: &str, path: &Path, error: &dyn fmt::Display) -> Error {
    Error::Failed(format!(
        "couldn't {action} the secrets index {}: {error}",
        path.display()
    ))
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
