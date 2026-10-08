//! Workspace trust (docs/decisions/lsp-client.md Q7): which folders the
//! user trusts to start language servers in, which can run a project's
//! code. Kept in user state (`<state_dir>/trust.toml`, see
//! [`paths::trust_path`](crate::paths::trust_path)), never in a project.
//!
//! Decisions are made per canonical folder and inherited: a folder's trust
//! is that of the nearest of it and its ancestors with a decision. A file
//! that can't be read, or isn't what fastedit writes, loads as empty, so
//! every folder is undecided, and is never overwritten.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Whether a folder is trusted to start language servers in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    Trusted,
    Untrusted,
    /// Neither it nor an ancestor has a decision.
    Undecided,
}

/// The first lines of the file, above its `[folders]` table.
const HEADER: &str = "\
# fastedit writes this file: the folders you trusted (true) or didn't
# (false) to start language servers in. Subfolders follow the nearest
# folder listed. Use File > Trust This Folder or Untrust This Folder to
# change one.

";

/// The trust decisions, by canonical folder, and the file they're kept in.
#[derive(Clone, Debug, Default)]
pub struct TrustStore {
    folders: BTreeMap<PathBuf, bool>,
    /// The file decisions are written to; `None` keeps them in memory.
    path: Option<PathBuf>,
    /// The file was there but couldn't be read or understood: it's never
    /// overwritten.
    bad: bool,
}

impl TrustStore {
    /// A store whose decisions last only as long as it does.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The decisions in the file at `path`. A missing file gives an empty
    /// store, and nothing is created until the first [`set`](Self::set).
    /// One that can't be read, isn't TOML, or holds anything but a bool
    /// for a folder also gives an empty store, so every folder is
    /// undecided, and is left as it is.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let folders = match fs::read_to_string(&path) {
            Ok(text) => parse(&text).or_else(|| {
                log::error!(
                    "{} isn't a trust file fastedit wrote: every folder is untrusted \
                     until it's fixed or removed",
                    path.display()
                );
                None
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some(BTreeMap::new()),
            Err(error) => {
                log::error!(
                    "can't read {}: {error}; every folder is untrusted",
                    path.display()
                );
                None
            }
        };
        Self {
            bad: folders.is_none(),
            folders: folders.unwrap_or_default(),
            path: Some(path),
        }
    }

    /// The trust of canonical folder `folder`: the decision of the nearest
    /// of it and its ancestors that has one, by components (so `/ab` isn't
    /// under `/a`). No I/O.
    pub fn trust_of(&self, folder: &Path) -> Trust {
        folder
            .ancestors()
            .find_map(|ancestor| self.folders.get(ancestor))
            .map_or(Trust::Undecided, |&trusted| {
                if trusted {
                    Trust::Trusted
                } else {
                    Trust::Untrusted
                }
            })
    }

    /// Records whether canonical folder `folder` is trusted, in memory
    /// only. Its subfolders follow it, unless they have decisions of their
    /// own.
    pub fn decide(&mut self, folder: &Path, trusted: bool) {
        self.folders.insert(folder.to_owned(), trusted);
    }

    /// The decision made for canonical folder `folder` itself, not
    /// inherited from an ancestor, if there is one. No I/O.
    pub fn decided(&self, folder: &Path) -> Option<bool> {
        self.folders.get(folder).copied()
    }

    /// [`decide`](Self::decide)s, then [`save`](Self::save)s.
    ///
    /// Errs, with the decision still made in memory, if `folder` isn't
    /// UTF-8 (it can't be a TOML key: the file is left as it was), or if
    /// saving does.
    pub fn set(&mut self, folder: &Path, trusted: bool) -> io::Result<()> {
        self.decide(folder, trusted);
        if self.path.is_some() && folder.to_str().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} isn't UTF-8, so it can't be saved", folder.display()),
            ));
        }
        self.save()
    }

    /// Rewrites the file, if there is one, with every decision whose
    /// folder is UTF-8: through a temporary file in the same directory
    /// renamed over it, creating the directory (mode 0700) and the file
    /// (mode 0600) if need be. Errs, leaving the file as it was, if it
    /// loaded bad or the write fails.
    pub fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if self.bad {
            return Err(io::Error::other(format!(
                "{} isn't a trust file fastedit wrote, so it's left as it is",
                path.display()
            )));
        }
        write(path, self.text().as_bytes())
    }

    /// The file's text: the header, then each decision whose folder is
    /// UTF-8.
    fn text(&self) -> String {
        let folders: toml::Table = self
            .folders
            .iter()
            .filter_map(|(folder, &trusted)| {
                Some((folder.to_str()?.to_owned(), toml::Value::Boolean(trusted)))
            })
            .collect();
        let mut table = toml::Table::new();
        table.insert("folders".into(), toml::Value::Table(folders));
        let body = toml::to_string(&table).unwrap_or_else(|_| "[folders]\n".into());
        format!("{HEADER}{body}")
    }
}

/// `path`'s canonical form ([`fs::canonicalize`]), or `path` as given if
/// it has none (it's missing, say).
pub fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The decisions in `text`, or `None` if it isn't TOML or `[folders]`
/// holds anything but bools.
fn parse(text: &str) -> Option<BTreeMap<PathBuf, bool>> {
    let table: toml::Table = toml::from_str(text).ok()?;
    match table.get("folders") {
        None => Some(BTreeMap::new()),
        Some(toml::Value::Table(folders)) => folders
            .iter()
            .map(|(folder, trusted)| Some((PathBuf::from(folder), trusted.as_bool()?)))
            .collect(),
        Some(_) => None,
    }
}

/// Writes `contents` to `path` through a temporary file beside it, renamed
/// over it, creating the directory (mode 0700) and the file (mode 0600).
fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}
