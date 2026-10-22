// SPDX-License-Identifier: LicenseRef-Proprietary

//! The capabilities the user granted each plugin, and whether a plugin may
//! load with what its manifest asks for.
//!
//! The user grants capabilities at install
//! (`docs/decisions/vmtnmkay62-extensions-and-plugins.md` §7). An update
//! that asks for more stops and asks before it loads, while the installed
//! version keeps running; one that asks for the same or less loads
//! silently. A `process.spawn:<program>` grant is bound to the absolute
//! path and file identity the program had when it was granted, and is
//! suspended, so asked again, when the file changes or goes.
//!
//! [`evaluate`] decides, as a pure function of the [`GrantRecord`] and the
//! request; [`GrantStore`] keeps the records in `<state>/plugin-grants.toml`,
//! read and written under a lock so two processes don't lose each other's
//! changes. Prompt text is the app's; enforcing a spawn grant at run time
//! is the host's.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use kiln_command::capability::{Capability, Grants, Target};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// The file in the state directory the grants are kept in.
pub const GRANTS_FILE: &str = "plugin-grants.toml";

/// The first lines of the file, above its `[plugins]` table.
const HEADER: &str = "\
# The capabilities you granted each plugin, and the programs its
# process.spawn grants were bound to. Remove a plugin's table to be asked
# again when it next loads.

";

/// A plugin version, as its manifest gives it.
pub type Version = (u32, u32, u32);

/// What the user decided for one plugin.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantRecord {
    /// The version installed with these grants; `None` if the plugin was
    /// never granted anything (its first request was declined).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<Version>,
    #[serde(default)]
    pub granted: Grants,
    /// What each `process.spawn:<program>` grant in `granted` is bound to,
    /// by program name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub programs: BTreeMap<String, ProgramIdentity>,
    /// Whether the user accepted `granted`'s `net:*` or `process.spawn:*`
    /// on its own warning.
    #[serde(default)]
    pub wildcards_accepted: bool,
    /// The last request the user declined, not asked again until the
    /// request changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declined: Option<Grants>,
    /// Programs whose spawn grant was withdrawn because the file changed or
    /// went, and why: for the prompt that asks again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub suspended: BTreeMap<String, SuspendReason>,
}

impl GrantRecord {
    /// Withdraws `program`'s spawn grant, as [`check_program`] found it
    /// `reason`: [`evaluate`] then gives an [`Decision::Escalation`] for any
    /// request that names it.
    pub fn suspend(&mut self, program: &str, reason: SuspendReason) {
        let capability = Capability::ProcessSpawn(Target::Named(program.to_owned()));
        self.granted = self
            .granted
            .iter()
            .filter(|granted| **granted != capability)
            .cloned()
            .collect();
        self.programs.remove(program);
        self.suspended.insert(program.to_owned(), reason);
    }

    /// The record after `requested`, covered by it, loads at `version`:
    /// only `requested` stays granted. A program named in it that only
    /// `process.spawn:*` covered is bound to what it resolves to on
    /// `path_var`; if one can't be resolved, the request can't load.
    fn narrowed(
        &self,
        version: Version,
        requested: &Grants,
        path_var: &OsStr,
    ) -> Result<Self, GrantStoreError> {
        let mut programs = BTreeMap::new();
        for capability in requested {
            let Capability::ProcessSpawn(Target::Named(program)) = capability else {
                continue;
            };
            let identity = match self.programs.get(program) {
                Some(identity) => identity.clone(),
                None => resolve_program(program, path_var).map_err(|source| {
                    GrantStoreError::Unresolved {
                        program: program.clone(),
                        source,
                    }
                })?,
            };
            programs.insert(program.clone(), identity);
        }
        let suspended = self
            .suspended
            .iter()
            .filter(|(program, _)| {
                requested
                    .iter()
                    .any(|capability| names(capability, program))
            })
            .map(|(program, reason)| (program.clone(), *reason))
            .collect();
        Ok(Self {
            version: Some(version),
            granted: requested.clone(),
            programs,
            wildcards_accepted: self.wildcards_accepted && has_wildcard(requested),
            declined: self.declined.clone(),
            suspended,
        })
    }
}

/// Whether `capability` is `process.spawn:<program>`.
fn names(capability: &Capability, program: &str) -> bool {
    matches!(capability, Capability::ProcessSpawn(Target::Named(name)) if name == program)
}

/// What to do with a plugin asking for a set of capabilities: [`evaluate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Ask the user; nothing loads until they grant.
    Prompt(Prompt),
    /// Load with these grants, which are what was asked for: the same as
    /// the record, or less, in which case the record shrinks to them.
    Load(Grants),
    /// The request has capabilities, or wider ones, than the record
    /// grants: ask about `added`, while `keep_running`, the installed
    /// version, runs on with its own grants.
    Escalation {
        added: Grants,
        keep_running: Option<Version>,
    },
    /// The user declined this same request: don't ask again.
    Declined,
}

/// Why a [`Decision::Prompt`] asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// Nothing was ever granted to the plugin.
    NewPlugin,
}

/// Whether a plugin with `record` (`None` if it has none) may load with
/// `requested`. Pure: the caller records what follows, as
/// [`GrantStore::decide`] does.
///
/// A request covered by the grants ([`Grants::allows`], so `net:*` covers
/// every named host and `process.spawn:*` every named program) loads,
/// shrinking them to it; one with a capability they don't cover is an
/// escalation naming what it adds, unless it's the request last declined.
pub fn evaluate(record: Option<&GrantRecord>, requested: &Grants) -> Decision {
    let Some(record) = record else {
        return Decision::Prompt(Prompt::NewPlugin);
    };
    let declined = record.declined.as_ref() == Some(requested);
    if record.version.is_none() {
        return if declined {
            Decision::Declined
        } else {
            Decision::Prompt(Prompt::NewPlugin)
        };
    }
    let added = added(&record.granted, requested);
    if added.is_empty() {
        Decision::Load(requested.clone())
    } else if declined {
        Decision::Declined
    } else {
        Decision::Escalation {
            added,
            keep_running: record.version,
        }
    }
}

/// The capabilities in `requested` that `granted` doesn't cover.
fn added(granted: &Grants, requested: &Grants) -> Grants {
    requested
        .iter()
        .filter(|capability| !granted.allows(capability))
        .cloned()
        .collect()
}

/// Whether granting `added` needs the user's separate acceptance: it has
/// `net:*` or `process.spawn:*`.
pub fn needs_wildcard_acceptance(added: &Grants) -> bool {
    has_wildcard(added)
}

fn has_wildcard(grants: &Grants) -> bool {
    grants.iter().any(is_wildcard)
}

fn is_wildcard(capability: &Capability) -> bool {
    matches!(
        capability,
        Capability::Net(Target::Any) | Capability::ProcessSpawn(Target::Any)
    )
}

/// A program a spawn grant is bound to: where it was found on `PATH` and
/// what the file was then.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramIdentity {
    /// The name the grant gives, as in `process.spawn:<name>`.
    pub name: String,
    /// Where it was found: an absolute path, as found (symlinks kept, so a
    /// program that reads its own name runs as itself).
    pub path: PathBuf,
    #[serde(with = "decimal")]
    pub device: u64,
    #[serde(with = "decimal")]
    pub inode: u64,
    #[serde(with = "decimal")]
    pub size: u64,
    /// The modification time, in seconds and nanoseconds since the epoch.
    pub modified: (i64, i64),
    /// The SHA-256 of the content, in lower-case hex.
    pub sha256: String,
}

/// TOML integers are signed 64-bit, and a device or inode number may not
/// fit: they're written as decimal strings.
mod decimal {
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    pub(super) fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("{name:?} is not a program name: it can't be empty, `.`, `..` or hold a `/`")]
    NotAProgramName { name: String },
    #[error("{name} isn't an executable file in any absolute PATH directory")]
    NotFound { name: String },
    #[error("can't read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The first executable file named `name` in an absolute directory of
/// `path_var` (a `PATH` value; relative entries, which follow the working
/// directory, are skipped), with its identity and content hash.
pub fn resolve_program(name: &str, path_var: &OsStr) -> Result<ProgramIdentity, ResolveError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(ResolveError::NotAProgramName {
            name: name.to_owned(),
        });
    }
    for dir in std::env::split_paths(path_var).filter(|dir| dir.is_absolute()) {
        let path = dir.join(name);
        // Anything that isn't an executable file, or can't be opened, is
        // passed over, as a shell would.
        let Ok((mut file, meta)) = open_executable(&path) else {
            continue;
        };
        let sha256 = hash(&mut file).map_err(|source| ResolveError::Io {
            path: path.clone(),
            source,
        })?;
        return Ok(ProgramIdentity {
            name: name.to_owned(),
            path,
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.size(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            sha256,
        });
    }
    Err(ResolveError::NotFound {
        name: name.to_owned(),
    })
}

/// `path` opened for reading without blocking, with its metadata, if it's
/// an executable regular file. What's checked is the file opened, not
/// the path, so nothing can be swapped in between.
fn open_executable(path: &Path) -> io::Result<(File, fs::Metadata)> {
    let file = kiln_core::platform::open_for_reading(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 {
        return Err(io::Error::other("not an executable file"));
    }
    Ok((file, meta))
}

/// The SHA-256 of what's left to read of `file`, in lower-case hex.
fn hash(file: &mut File) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// That a program's file is no longer the one granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the program's grant is suspended: {reason}")]
pub struct Suspended {
    pub reason: SuspendReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuspendReason {
    /// The file is different, or can't be read.
    Changed,
    /// There's no file at the path.
    Missing,
}

impl std::fmt::Display for SuspendReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Changed => "the program changed",
            Self::Missing => "the program is gone",
        })
    }
}

/// Whether the file at `identity.path` is still the one recorded: an
/// executable file with the same device, inode, size, modification time
/// and content.
pub fn check_program(identity: &ProgramIdentity) -> Result<(), Suspended> {
    let suspended = |error: io::Error| Suspended {
        reason: match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => SuspendReason::Missing,
            _ => SuspendReason::Changed,
        },
    };
    let (mut file, meta) = open_executable(&identity.path).map_err(suspended)?;
    let changed = Suspended {
        reason: SuspendReason::Changed,
    };
    let same_file = meta.dev() == identity.device
        && meta.ino() == identity.inode
        && meta.size() == identity.size
        && (meta.mtime(), meta.mtime_nsec()) == identity.modified;
    if !same_file || hash(&mut file).map_err(suspended)? != identity.sha256 {
        return Err(changed);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum GrantStoreError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The file isn't one this store writes; it's left as it is.
    #[error("{} isn't a grants file this app understands: {source}", path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("granting {0:?} needs its own acceptance of the wildcard")]
    WildcardsNotAccepted(Grants),
    #[error("process.spawn:{0} has no program resolved for it")]
    ProgramNotResolved(String),
    /// A request narrowing `process.spawn:*` names a program that can't be
    /// bound: it doesn't load, and the record is left as it was.
    #[error("process.spawn:{program} can't be bound to a program: {source}")]
    Unresolved {
        program: String,
        #[source]
        source: ResolveError,
    },
}

/// The file the records are written in.
#[derive(Default, Serialize, Deserialize)]
struct GrantsFile {
    #[serde(default)]
    plugins: BTreeMap<String, GrantRecord>,
}

/// The plugins' [`GrantRecord`]s, by plugin id, in one file. Every call
/// reads the file afresh, and every change is made under an exclusive lock
/// on `<file>.lock` and written to a temporary file renamed over it, so
/// two processes' changes don't lose each other or tear the file. A file
/// that can't be understood is never overwritten.
#[derive(Clone, Debug)]
pub struct GrantStore {
    path: PathBuf,
}

impl GrantStore {
    /// The store in the file at `path`, usually `<state>/`[`GRANTS_FILE`].
    /// Nothing is created until the first change.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `id`'s record, if it has one.
    pub fn get(&self, id: &str) -> Result<Option<GrantRecord>, GrantStoreError> {
        Ok(self.read()?.plugins.remove(id))
    }

    /// Replaces `id`'s record with `record`.
    pub fn set(&self, id: &str, record: GrantRecord) -> Result<(), GrantStoreError> {
        self.update(id, |_| Ok(Some(record)))
    }

    /// Removes `id`'s record, as when it's uninstalled; false if it had
    /// none.
    pub fn forget(&self, id: &str) -> Result<bool, GrantStoreError> {
        let mut had = false;
        self.update(id, |record| {
            had = record.is_some();
            Ok(None)
        })?;
        Ok(had)
    }

    /// [`evaluate`]s `id`'s request, at `version`, against its record, and
    /// on [`Decision::Load`] records `version` and shrinks the grants to
    /// the request, binding a named program that `process.spawn:*` covered
    /// to what it resolves to on `path_var` (a `PATH` value). If one can't
    /// be resolved it errs with [`GrantStoreError::Unresolved`]: nothing
    /// loads, and the record, its version included, is left as it was.
    /// Programs already bound are taken as recorded: run
    /// [`check_programs`](Self::check_programs) first to suspend any that
    /// changed.
    pub fn decide(
        &self,
        id: &str,
        version: Version,
        requested: &Grants,
        path_var: &OsStr,
    ) -> Result<Decision, GrantStoreError> {
        let mut decision = None;
        self.update(id, |record| {
            let made = evaluate(record.as_ref(), requested);
            let record = match (&made, record) {
                (Decision::Load(_), Some(record)) => {
                    Some(record.narrowed(version, requested, path_var)?)
                }
                (_, record) => record,
            };
            decision = Some(made);
            Ok(record)
        })?;
        Ok(decision.expect("update ran"))
    }

    /// Grants `id`, at `version`, exactly `requested`, as the user did at a
    /// prompt. Each `process.spawn:<program>` is bound to its identity in
    /// `programs`, or to the one recorded if it was already granted. Adding
    /// `net:*` or `process.spawn:*` needs `accept_wildcards`, the user's
    /// acceptance of their own warning; without it nothing changes.
    pub fn grant(
        &self,
        id: &str,
        version: Version,
        requested: &Grants,
        programs: &[ProgramIdentity],
        accept_wildcards: bool,
    ) -> Result<GrantRecord, GrantStoreError> {
        let mut granted = None;
        self.update(id, |record| {
            let previous = record.unwrap_or_default();
            let added = added(&previous.granted, requested);
            if needs_wildcard_acceptance(&added) && !accept_wildcards {
                let wildcards = added.iter().filter(|capability| is_wildcard(capability));
                return Err(GrantStoreError::WildcardsNotAccepted(
                    wildcards.cloned().collect(),
                ));
            }
            let mut bound = BTreeMap::new();
            for capability in requested {
                let Capability::ProcessSpawn(Target::Named(program)) = capability else {
                    continue;
                };
                let identity = programs
                    .iter()
                    .find(|identity| identity.name == *program)
                    .or_else(|| previous.programs.get(program))
                    .ok_or_else(|| GrantStoreError::ProgramNotResolved(program.clone()))?;
                bound.insert(program.clone(), identity.clone());
            }
            let record = GrantRecord {
                version: Some(version),
                granted: requested.clone(),
                programs: bound,
                wildcards_accepted: has_wildcard(requested),
                declined: None,
                suspended: BTreeMap::new(),
            };
            granted = Some(record.clone());
            Ok(Some(record))
        })?;
        Ok(granted.expect("update ran"))
    }

    /// Records that the user declined `id`'s `requested`, so
    /// [`evaluate`] gives [`Decision::Declined`] for it until it changes.
    /// What was granted stays granted.
    pub fn decline(&self, id: &str, requested: &Grants) -> Result<(), GrantStoreError> {
        self.update(id, |record| {
            let mut record = record.unwrap_or_default();
            record.declined = Some(requested.clone());
            Ok(Some(record))
        })
    }

    /// [`check_program`]s each of `id`'s spawn grants, suspending those
    /// whose program changed or went, which are returned.
    pub fn check_programs(&self, id: &str) -> Result<Vec<(String, Suspended)>, GrantStoreError> {
        let mut suspended = Vec::new();
        self.update(id, |record| {
            let Some(mut record) = record else {
                return Ok(None);
            };
            for (program, identity) in record.programs.clone() {
                if let Err(found) = check_program(&identity) {
                    record.suspend(&program, found.reason);
                    suspended.push((program, found));
                }
            }
            Ok(Some(record))
        })?;
        Ok(suspended)
    }

    /// Replaces `id`'s record with what `change` makes of it, under the
    /// lock; the file is written only if the record changed.
    fn update(
        &self,
        id: &str,
        change: impl FnOnce(Option<GrantRecord>) -> Result<Option<GrantRecord>, GrantStoreError>,
    ) -> Result<(), GrantStoreError> {
        let io_error = |source| GrantStoreError::Io {
            path: self.path.clone(),
            source,
        };
        locked(&self.path, || {
            let mut file = self.read()?;
            let before = file.plugins.remove(id);
            let after = change(before.clone())?;
            if after == before {
                return Ok(());
            }
            if let Some(record) = after {
                file.plugins.insert(id.to_owned(), record);
            }
            let body = toml::to_string(&file).map_err(|error| io_error(io::Error::other(error)))?;
            write(&self.path, format!("{HEADER}{body}").as_bytes()).map_err(io_error)
        })
        .map_err(io_error)?
    }

    fn read(&self) -> Result<GrantsFile, GrantStoreError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(GrantsFile::default());
            }
            Err(source) => {
                return Err(GrantStoreError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|source| GrantStoreError::Unreadable {
            path: self.path.clone(),
            source,
        })
    }
}

// The lock and write below are `kiln_core::trust`'s, which that crate keeps
// to itself.

/// Runs `f` holding an exclusive lock on `<path>.lock` (created, mode
/// 0600, beside `path` and never removed), blocking while another, in this
/// process or another, holds it. Errs if the lock can't be taken.
fn locked<T>(path: &Path, f: impl FnOnce() -> T) -> io::Result<T> {
    let mut lock_name = path.file_name().unwrap_or_default().to_owned();
    lock_name.push(".lock");
    let lock_path = path.with_file_name(lock_name);
    create_dir(lock_path.parent().unwrap_or(Path::new(".")))?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)?;
    lock.lock()?;
    // The lock is released when `lock` drops, after `f`.
    Ok(f())
}

/// Writes `contents` to `path` through a temporary file beside it, renamed
/// over it, creating the directory (mode 0700) and the file (mode 0600).
fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    create_dir(dir)?;
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

/// Creates `dir` and its missing ancestors, mode 0700.
fn create_dir(dir: &Path) -> io::Result<()> {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}
