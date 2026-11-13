// SPDX-License-Identifier: Apache-2.0

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
//! is the host's. A plugin's thread never writes the file: a suspension
//! found during a call goes to the store's [`SuspensionWriter`], which
//! retries it until it is written.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kiln_command::capability::{Capability, Grants, Target};
use kiln_l10n::tr;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::limits::BackgroundLimits;

/// The file in the state directory the grants are kept in.
pub const GRANTS_FILE: &str = "plugin-grants.toml";

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
    /// What marks each program's binding as made by one grant and no other. A program can be
    /// granted again with its file's identity unchanged, even after its record was removed, so
    /// this is what tells a suspension found under an earlier grant from one that still applies.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grant_tokens: BTreeMap<String, String>,
    /// The background limits the user accepted, or `None` for the defaults: a record written
    /// before there were any, or never granted any. See [`evaluate_background`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<BackgroundLimits>,
    /// The background limits of the request last declined, with `declined`; `None` for the
    /// defaults, as in a record written before they were kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declined_background: Option<BackgroundLimits>,
}

impl GrantRecord {
    /// The token of the grant that bound `program`, if it has one: a record written before there
    /// were tokens has none.
    pub(crate) fn grant_token(&self, program: &str) -> Option<&str> {
        self.grant_tokens.get(program).map(String::as_str)
    }

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
        let mut grant_tokens = self.grant_tokens.clone();
        for capability in requested {
            let Capability::ProcessSpawn(Target::Named(program)) = capability else {
                continue;
            };
            if self.suspended.contains_key(program) {
                return Err(GrantStoreError::Suspended(program.clone()));
            }
            let identity = match self.programs.get(program) {
                Some(identity) => identity.clone(),
                None => {
                    let resolved = resolve_program(program, path_var).map_err(|source| {
                        GrantStoreError::Unresolved {
                            program: program.clone(),
                            source,
                        }
                    })?;
                    grant_tokens.insert(program.clone(), new_grant_token());
                    resolved
                }
            };
            programs.insert(program.clone(), identity);
        }
        // A suspension holds while anything requested still covers the
        // program, `process.spawn:*` included: only a grant that binds it
        // anew lifts it.
        let suspended = self
            .suspended
            .iter()
            .filter(|(program, _)| covers(requested, program))
            .map(|(program, reason)| (program.clone(), *reason))
            .collect();
        Ok(Self {
            version: Some(version),
            granted: requested.clone(),
            programs,
            wildcards_accepted: self.wildcards_accepted && has_wildcard(requested),
            declined: self.declined.clone(),
            suspended,
            grant_tokens,
            background: self.background,
            declined_background: self.declined_background,
        })
    }
}

/// A token no other grant has, in this process or another, whatever was removed since: the clock,
/// the process and a count within it.
fn new_grant_token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    let now = now.unwrap_or_default().as_nanos();
    let (process, next) = (std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    format!("{now:x}-{process:x}-{next:x}")
}

/// Whether `capability` is `process.spawn:<program>`.
fn names(capability: &Capability, program: &str) -> bool {
    matches!(capability, Capability::ProcessSpawn(Target::Named(name)) if name == program)
}

/// Whether `grants` reach `program`, by its name or `process.spawn:*`.
fn covers(grants: &Grants, program: &str) -> bool {
    grants.allows(&Capability::ProcessSpawn(Target::Named(program.to_owned())))
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
/// A program whose grant is suspended is never covered, even by
/// `process.spawn:*`: the user is asked about it again.
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
    let added: Grants = requested
        .iter()
        .filter(|capability| {
            !record.granted.allows(capability)
                || record
                    .suspended
                    .keys()
                    .any(|program| names(capability, program))
        })
        .cloned()
        .collect();
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

/// [`evaluate`], where an update that loads under the grants but raises `background`'s memory
/// or wall time over what the record accepted is an escalation too, with nothing in `added`:
/// the prompt names the raised limits ([`BackgroundLimits::raises`]). Lowering one never asks.
/// A record with no accepted limits accepts the defaults.
pub fn evaluate_background(
    record: Option<&GrantRecord>,
    requested: &Grants,
    background: &BackgroundLimits,
) -> Decision {
    // A decline covers the whole request: the capabilities and the limits. A record written
    // before limits were kept declined the defaults.
    let declined = record.is_some_and(|record| {
        record.declined.as_ref() == Some(requested)
            && record.declined_background.unwrap_or_default() == *background
    });
    let Some(record) = record else {
        return evaluate(None, requested);
    };
    let mut open = record.clone();
    if !declined {
        open.declined = None;
    }
    let decision = evaluate(Some(&open), requested);
    if !matches!(decision, Decision::Load(_)) {
        return decision;
    }
    let accepted = record.background.unwrap_or_default();
    if background.raises(&accepted).is_empty() {
        decision
    } else if declined {
        Decision::Declined
    } else {
        Decision::Escalation {
            added: Grants::new(),
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
    /// The program that runs the file, when it is not run directly: `cmd.exe` for a `.bat` or
    /// `.cmd` file, which reads its arguments again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<String>,
}

/// The interpreter a file named `path` is run through: `cmd.exe` for `.bat` and `.cmd`.
pub fn interpreter_for(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?;
    (extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd"))
        .then(|| "cmd.exe".to_owned())
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
    NotAProgramName {
        name: String,
    },
    NotFound {
        name: String,
    },
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// In the active locale.
impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::NotAProgramName { name } => {
                tr!("kiln-plugin-program-not-a-name", name = format!("{name:?}"))
            }
            Self::NotFound { name } => tr!("kiln-plugin-program-not-found", name = name),
            Self::Io { path, source } => tr!(
                "kiln-plugin-program-unreadable",
                path = path.display().to_string(),
                reason = source.to_string()
            ),
        })
    }
}

/// The first executable file named `name` in an absolute directory of
/// `path_var` (a `PATH` value; relative entries, which follow the working
/// directory, are skipped), with its identity and content hash.
pub fn resolve_program(name: &str, path_var: &OsStr) -> Result<ProgramIdentity, ResolveError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(std::path::is_separator) {
        return Err(ResolveError::NotAProgramName {
            name: name.to_owned(),
        });
    }
    let candidates = std::env::split_paths(path_var)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| kiln_core::platform::program_candidates(&dir, name));
    for path in candidates {
        // Anything that isn't an executable file, or can't be opened, is
        // passed over, as a shell would.
        let Ok((mut file, meta)) = open_executable(&path) else {
            continue;
        };
        let Ok((device, inode)) = kiln_core::platform::file_identity(&file) else {
            continue;
        };
        let sha256 = hash(&mut file).map_err(|source| ResolveError::Io {
            path: path.clone(),
            source,
        })?;
        return Ok(ProgramIdentity {
            name: name.to_owned(),
            interpreter: interpreter_for(&path),
            path,
            device,
            inode,
            size: meta.len(),
            modified: kiln_core::platform::modified_parts(&meta),
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
    if !kiln_core::platform::is_executable(path, &meta) {
        return Err(io::Error::other(
            tr!("kiln-plugin-program-not-executable").to_string(),
        ));
    }
    Ok((file, meta))
}

/// The SHA-256 of what's left to read of `file`, in lower-case hex.
fn hash(file: &mut File) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// [`hash`], read in chunks, none once `by` has passed: none if it did.
fn hash_by(file: &mut File, by: Instant) -> io::Result<Option<String>> {
    let mut hasher = Sha256::new();
    let mut chunk = vec![0; HASH_CHUNK];
    loop {
        if Instant::now() >= by {
            return Ok(None);
        }
        match file.read(&mut chunk) {
            Ok(0) => return Ok(Some(format!("{:x}", hasher.finalize()))),
            Ok(read) => {
                hasher.update(&chunk[..read]);
                #[cfg(test)]
                crate::tests::hash_probe::after_chunk();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

/// How much of a program a deadlined check reads between looks at the clock.
const HASH_CHUNK: usize = 64 << 10;

/// A check by a deadline that the deadline ended before it finished: the program isn't known to
/// have changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub struct CheckTimedOut;

/// In the active locale.
impl std::fmt::Display for CheckTimedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!("kiln-plugin-program-check-timed-out"))
    }
}

/// That a program's file is no longer the one granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub struct Suspended {
    pub reason: SuspendReason,
}

/// In the active locale.
impl std::fmt::Display for Suspended {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&tr!(
            "kiln-plugin-program-suspended",
            reason = self.reason.to_string()
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuspendReason {
    /// The file is different, or can't be read.
    Changed,
    /// There's no file at the path.
    Missing,
}

/// In the active locale.
impl std::fmt::Display for SuspendReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Changed => tr!("kiln-plugin-program-changed"),
            Self::Missing => tr!("kiln-plugin-program-gone"),
        })
    }
}

/// Whether the file at `identity.path` is still the one recorded: an
/// executable file with the same device, inode, size, modification time
/// and content.
pub fn check_program(identity: &ProgramIdentity) -> Result<(), Suspended> {
    // With no deadline it can't time out.
    check_with(identity, None).unwrap_or(Ok(()))
}

/// [`check_program`], read in chunks, stopping once `by` has passed: then it is
/// [`CheckTimedOut`], and the program, not known to have changed, is not suspended.
pub fn check_program_by(
    identity: &ProgramIdentity,
    by: Instant,
) -> Result<Result<(), Suspended>, CheckTimedOut> {
    check_with(identity, Some(by))
}

fn check_with(
    identity: &ProgramIdentity,
    by: Option<Instant>,
) -> Result<Result<(), Suspended>, CheckTimedOut> {
    let suspended = |error: io::Error| Suspended {
        reason: match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => SuspendReason::Missing,
            _ => SuspendReason::Changed,
        },
    };
    let changed = Suspended {
        reason: SuspendReason::Changed,
    };
    if passed(by) {
        return Err(CheckTimedOut);
    }
    let opened = open_executable(&identity.path);
    #[cfg(test)]
    crate::tests::seams::after_open();
    // Past the end nothing the file turned out to be counts, an error from it included.
    if passed(by) {
        return Err(CheckTimedOut);
    }
    let (mut file, meta) = match opened {
        Ok(opened) => opened,
        Err(error) => return Ok(Err(suspended(error))),
    };
    let same_file = kiln_core::platform::file_identity(&file).ok()
        == Some((identity.device, identity.inode))
        && meta.len() == identity.size
        && kiln_core::platform::modified_parts(&meta) == identity.modified;
    if !same_file {
        return Ok(Err(changed));
    }
    let hashed = match by {
        Some(by) => match hash_by(&mut file, by) {
            Ok(Some(hashed)) => Ok(hashed),
            Ok(None) => return Err(CheckTimedOut),
            Err(error) => Err(error),
        },
        None => hash(&mut file),
    };
    if hashed.is_err() && passed(by) {
        return Err(CheckTimedOut);
    }
    Ok(match hashed {
        Ok(hashed) if hashed == identity.sha256 => Ok(()),
        Ok(_) => Err(changed),
        Err(error) => Err(suspended(error)),
    })
}

/// Whether `by`, if there is one, has passed.
fn passed(by: Option<Instant>) -> bool {
    by.is_some_and(|by| Instant::now() >= by)
}

#[derive(Debug, thiserror::Error)]
pub enum GrantStoreError {
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The file isn't one this store writes; it's left as it is.
    Unreadable {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    WildcardsNotAccepted(Grants),
    ProgramNotResolved(String),
    /// A request would bind a program whose grant is suspended without a
    /// prompt re-recording it: nothing changes.
    Suspended(String),
    /// A request narrowing `process.spawn:*` names a program that can't be
    /// bound: it doesn't load, and the record is left as it was.
    Unresolved {
        program: String,
        #[source]
        source: ResolveError,
    },
}

/// In the active locale.
impl std::fmt::Display for GrantStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Io { path, source } => tr!(
                "kiln-plugin-grants-io",
                path = path.display().to_string(),
                reason = source.to_string()
            ),
            Self::Unreadable { path, source } => tr!(
                "kiln-plugin-grants-unreadable",
                path = path.display().to_string(),
                reason = source.to_string()
            ),
            Self::WildcardsNotAccepted(grants) => tr!(
                "kiln-plugin-grants-wildcard-not-accepted",
                grants = format!("{grants:?}")
            ),
            Self::ProgramNotResolved(program) => {
                tr!("kiln-plugin-grants-program-not-resolved", program = program)
            }
            Self::Suspended(program) => {
                tr!("kiln-plugin-grants-program-suspended", program = program)
            }
            Self::Unresolved { program, source } => tr!(
                "kiln-plugin-grants-program-unbound",
                program = program,
                reason = source.to_string()
            ),
        })
    }
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

    /// `id`'s record, if it has one, read under the lock so a change in flight is never seen
    /// half-made.
    pub fn get(&self, id: &str) -> Result<Option<GrantRecord>, GrantStoreError> {
        let io_error = |source| GrantStoreError::Io {
            path: self.path.clone(),
            source,
        };
        // Nothing is created for a read: with no directory there is no file to read.
        if self
            .path
            .parent()
            .is_some_and(|dir| !dir.as_os_str().is_empty() && !dir.exists())
        {
            return Ok(None);
        }
        locked_by(&self.path, None, || Ok(self.read()?.plugins.remove(id))).map_err(io_error)?
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
    ///
    /// A suspended program stays suspended while `requested` still covers
    /// it, unless it is named and bound anew here, to its identity in
    /// `programs`: that, the prompt re-recording the program, is the only
    /// thing that lifts a suspension. Naming a suspended program without
    /// one is refused.
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
            let mut rebound = Vec::new();
            let mut grant_tokens = previous.grant_tokens.clone();
            for capability in requested {
                let Capability::ProcessSpawn(Target::Named(program)) = capability else {
                    continue;
                };
                let fresh = programs.iter().find(|identity| identity.name == *program);
                // A suspended program is never bound to what was recorded before.
                let recorded = || {
                    (!previous.suspended.contains_key(program))
                        .then(|| previous.programs.get(program))
                        .flatten()
                };
                let identity = fresh.or_else(recorded).ok_or_else(|| {
                    if previous.suspended.contains_key(program) {
                        GrantStoreError::Suspended(program.clone())
                    } else {
                        GrantStoreError::ProgramNotResolved(program.clone())
                    }
                })?;
                if fresh.is_some() {
                    rebound.push(program.clone());
                    grant_tokens.insert(program.clone(), new_grant_token());
                }
                bound.insert(program.clone(), identity.clone());
            }
            let record = GrantRecord {
                version: Some(version),
                granted: requested.clone(),
                wildcards_accepted: has_wildcard(requested),
                declined: None,
                suspended: previous
                    .suspended
                    .into_iter()
                    .filter(|(program, _)| covers(requested, program) && !rebound.contains(program))
                    .collect(),
                programs: bound,
                grant_tokens,
                background: previous.background,
                declined_background: None,
            };
            granted = Some(record.clone());
            Ok(Some(record))
        })?;
        Ok(granted.expect("update ran"))
    }

    /// Records the background limits the user accepted for `id`, as at a prompt that named
    /// them. Run after [`Self::grant`]; nothing else changes.
    pub fn accept_background(
        &self,
        id: &str,
        background: BackgroundLimits,
    ) -> Result<(), GrantStoreError> {
        self.update(id, |record| {
            let mut record = record.unwrap_or_default();
            record.background = Some(background);
            Ok(Some(record))
        })?;
        Ok(())
    }

    /// [`Self::decline`], for a request that also asked for `background`: it is not asked again
    /// until the capabilities or the limits change ([`evaluate_background`]).
    pub fn decline_with_background(
        &self,
        id: &str,
        requested: &Grants,
        background: BackgroundLimits,
    ) -> Result<(), GrantStoreError> {
        self.update(id, |record| {
            let mut record = record.unwrap_or_default();
            record.declined = Some(requested.clone());
            record.declined_background = Some(background);
            Ok(Some(record))
        })?;
        Ok(())
    }

    /// Records that the user declined `id`'s `requested`, so
    /// [`evaluate`] gives [`Decision::Declined`] for it until it changes.
    /// What was granted stays granted.
    pub fn decline(&self, id: &str, requested: &Grants) -> Result<(), GrantStoreError> {
        self.update(id, |record| {
            let mut record = record.unwrap_or_default();
            record.declined = Some(requested.clone());
            record.declined_background = None;
            Ok(Some(record))
        })
    }

    /// [`check_program`]s each of `id`'s spawn grants, suspending those
    /// whose program changed or went, which are returned.
    pub fn check_programs(&self, id: &str) -> Result<Vec<(String, Suspended)>, GrantStoreError> {
        self.check_programs_by(id, None)
    }

    /// [`Self::check_programs`], taking the lock, checking each program and writing by `by`: past
    /// it nothing more is read or written, and the error is an [`io::ErrorKind::TimedOut`] one.
    /// The write itself can't be stopped once it has started, so a plugin's thread doesn't call
    /// this with one to make: it hands a suspension to the store's [`SuspensionWriter`].
    pub fn check_programs_by(
        &self,
        id: &str,
        by: Option<Instant>,
    ) -> Result<Vec<(String, Suspended)>, GrantStoreError> {
        let mut suspended = Vec::new();
        self.update_by(id, by, |record| {
            let Some(mut record) = record else {
                return Ok(None);
            };
            for (program, identity) in record.programs.clone() {
                match check_with(&identity, by) {
                    Ok(Err(found)) => {
                        record.suspend(&program, found.reason);
                        suspended.push((program, found));
                    }
                    Ok(Ok(())) => {}
                    // Out of time: nothing is written.
                    Err(CheckTimedOut) => {
                        return Err(GrantStoreError::Io {
                            path: self.path.clone(),
                            source: io::ErrorKind::TimedOut.into(),
                        });
                    }
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
        self.update_by(id, None, change)
    }

    /// [`Self::update`], taking the lock and writing only by `by` when there is one: past it,
    /// nothing is written and the error is an [`io::ErrorKind::TimedOut`] one.
    fn update_by(
        &self,
        id: &str,
        by: Option<Instant>,
        change: impl FnOnce(Option<GrantRecord>) -> Result<Option<GrantRecord>, GrantStoreError>,
    ) -> Result<(), GrantStoreError> {
        let io_error = |source| GrantStoreError::Io {
            path: self.path.clone(),
            source,
        };
        locked_by(&self.path, by, || {
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
            if passed(by) {
                return Err(io_error(io::ErrorKind::TimedOut.into()));
            }
            let header = tr!("kiln-plugin-grants-file-header");
            write(&self.path, format!("{header}\n\n{body}").as_bytes()).map_err(io_error)
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

/// How long a suspension that failed to be written first waits to try again; each failure doubles
/// it.
const RETRY_FIRST: Duration = Duration::from_millis(50);
/// The most a suspension waits between tries.
const RETRY_MOST: Duration = Duration::from_secs(30);

/// A finding that a program's file is no longer the one granted, for the store to record.
#[derive(Debug)]
pub(crate) struct SuspensionWrite {
    pub(crate) store: GrantStore,
    pub(crate) plugin: String,
    pub(crate) program: String,
    /// What the program was bound to when it was found so.
    pub(crate) identity: ProgramIdentity,
    /// The [`GrantRecord::grant_token`] it was found under.
    pub(crate) grant_token: Option<String>,
    pub(crate) reason: SuspendReason,
}

impl SuspensionWrite {
    /// Records the suspension, while the plugin's record still holds the grant it was found
    /// under. If it doesn't, because the program was granted again since or the plugin forgotten,
    /// nothing changes: the finding was about a grant that has gone.
    fn write(&self) -> Result<(), GrantStoreError> {
        #[cfg(test)]
        crate::tests::seams::store_io(self.store.path()).map_err(|source| GrantStoreError::Io {
            path: self.store.path.clone(),
            source,
        })?;
        self.store.update(&self.plugin, |record| {
            Ok(record.map(|mut record| {
                if record.programs.get(&self.program) == Some(&self.identity)
                    && record.grant_token(&self.program) == self.grant_token.as_deref()
                {
                    record.suspend(&self.program, self.reason);
                }
                record
            }))
        })
    }
}

/// Writes one store's suspensions away from the threads that find them: the lock, the read and
/// the write with its sync can each take as long as the disk does, and a plugin's thread can't
/// wait for that past its call's share.
///
/// It lasts as long as the process, whatever plugin found a suspension or has since unloaded, so
/// while its thread runs a suspension stays queued and is retried until it's written. The thread
/// starts when a plugin that records in the store loads, so finding a suspension later needs no
/// new thread. A thread that can't start leaves the queue as it is, and the next load, spawn or
/// unload on this store tries again.
///
/// An accepted limit: if the thread can't be created at load and no later load, spawn or unload on
/// this store comes before the process ends, nothing retries, as nothing here runs without a
/// thread and the engine's ticker isn't made to. The suspension then isn't recorded, and the next
/// launch's load finds it only if the program is still changed.
pub(crate) struct SuspensionWriter {
    path: PathBuf,
    state: Mutex<WriterState>,
    wake: Condvar,
}

#[derive(Default)]
struct WriterState {
    queue: Vec<Queued>,
    running: bool,
}

struct Queued {
    write: SuspensionWrite,
    written: mpsc::Sender<()>,
    wait: Duration,
    next_try: Instant,
}

static WRITERS: Mutex<BTreeMap<PathBuf, Arc<SuspensionWriter>>> = Mutex::new(BTreeMap::new());

impl SuspensionWriter {
    /// The writer of the store at `store`'s path, the same one for every plugin and window.
    pub(crate) fn of(store: &GrantStore) -> Arc<Self> {
        let mut writers = WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
        let writer = writers.entry(store.path.clone()).or_insert_with(|| {
            Arc::new(Self {
                path: store.path.clone(),
                state: Mutex::default(),
                wake: Condvar::new(),
            })
        });
        Arc::clone(writer)
    }

    fn locked(&self) -> MutexGuard<'_, WriterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts the thread if it isn't running. It finds whatever is queued.
    pub(crate) fn start(self: &Arc<Self>) {
        let mut state = self.locked();
        if state.running {
            return;
        }
        match self.spawn() {
            Ok(()) => state.running = true,
            Err(error) => log::warn!("can't start the writer of {}: {error}", self.path.display()),
        }
    }

    fn spawn(self: &Arc<Self>) -> io::Result<()> {
        #[cfg(test)]
        crate::tests::seams::thread_start(&self.path)?;
        let writer = Arc::clone(self);
        std::thread::Builder::new()
            .name("plugin-grant-suspension".to_owned())
            .spawn(move || writer.run())
            .map(drop)
    }

    /// Queues `write`. The receiver hears once it's written, if a thread is running to write it;
    /// with none, it stays queued for one, and there's nothing to wait for.
    pub(crate) fn submit(self: &Arc<Self>, write: SuspensionWrite) -> Option<mpsc::Receiver<()>> {
        let (written, receiver) = mpsc::channel();
        self.locked().queue.push(Queued {
            write,
            written,
            wait: RETRY_FIRST,
            next_try: Instant::now(),
        });
        self.wake.notify_one();
        self.start();
        self.locked().running.then_some(receiver)
    }

    fn run(&self) {
        let mut state = self.locked();
        loop {
            let now = Instant::now();
            let due = state.queue.iter().position(|queued| queued.next_try <= now);
            let Some(due) = due else {
                let soonest = state.queue.iter().map(|queued| queued.next_try).min();
                state = match soonest {
                    Some(soonest) => {
                        let wait = soonest.saturating_duration_since(now);
                        let waited = self.wake.wait_timeout(state, wait);
                        waited.unwrap_or_else(PoisonError::into_inner).0
                    }
                    None => (self.wake.wait(state)).unwrap_or_else(PoisonError::into_inner),
                };
                continue;
            };
            let queued = state.queue.remove(due);
            drop(state);
            let again = Self::attempt(queued);
            state = self.locked();
            state.queue.extend(again);
        }
    }

    /// Tries `queued`'s write: what's left to retry, if it failed.
    fn attempt(mut queued: Queued) -> Option<Queued> {
        match queued.write.write() {
            Ok(()) => {
                #[cfg(test)]
                crate::tests::seams::written(queued.write.store.path());
                // Whoever was waiting may have stopped.
                let _ = queued.written.send(());
                None
            }
            Err(error) => {
                // Only the first failure is logged: a store that stays unwritable goes on failing.
                if queued.wait == RETRY_FIRST {
                    let program = &queued.write.program;
                    log::warn!(
                        "can't yet record that process.spawn:{program} is suspended: {error}"
                    );
                }
                queued.next_try = Instant::now() + queued.wait;
                queued.wait = (queued.wait * 2).min(RETRY_MOST);
                Some(queued)
            }
        }
    }
}

// The lock and write below are `kiln_core::trust`'s, which that crate keeps
// to itself.

/// Runs `f` holding an exclusive lock on `<path>.lock` (created, mode 0600, beside `path` and
/// never removed), blocking while another, in this process or another, holds it. With `by`, it
/// neither waits for the lock nor accepts it past that, a free lock included: then the error is
/// [`io::ErrorKind::TimedOut`] and `f` doesn't run. Errs if the lock can't be taken.
fn locked_by<T>(path: &Path, by: Option<Instant>, f: impl FnOnce() -> T) -> io::Result<T> {
    if passed(by) {
        return Err(io::ErrorKind::TimedOut.into());
    }
    let mut lock_name = path.file_name().unwrap_or_default().to_owned();
    lock_name.push(".lock");
    let lock_path = path.with_file_name(lock_name);
    create_dir(lock_path.parent().unwrap_or(Path::new(".")))?;
    let lock = kiln_core::platform::private(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false),
    )
    .open(&lock_path)?;
    match by {
        None => lock.lock()?,
        Some(by) => loop {
            if Instant::now() >= by {
                return Err(io::ErrorKind::TimedOut.into());
            }
            match lock.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
        },
    }
    // The lock is released when `lock` drops, after `f`.
    Ok(f())
}

/// How long a deadlined lock waits between tries.
const LOCK_POLL: Duration = Duration::from_millis(1);

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
        let mut file =
            kiln_core::platform::private(fs::OpenOptions::new().write(true).create_new(true))
                .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        kiln_core::platform::replace_file(&temporary, path)
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
    kiln_core::platform::create_private_dir(dir)
}
