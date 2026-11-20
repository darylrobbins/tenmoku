//! Consent for the language servers extensions contribute (extensions
//! record `docs/decisions/vmtnmkay62-extensions-and-plugins.md` §1, Q5).
//!
//! An extension's `[contributes.lsp.<name>]` names a program fastedit spawns
//! with the user's rights, outside any sandbox, so none starts until the
//! user consents to it:
//!
//! - **What's consented to.** A server's [`Invocation`]: the executable as
//!   the launcher would resolve it, its arguments, the environment it sets,
//!   the content of each file inside the extension's directory that its
//!   arguments (or its executable) name, and a hash of the whole installed
//!   package (every file under that directory), so a changed module those
//!   scripts import is a change too. What doesn't change what runs, its
//!   languages, settings, initialization options, root markers and
//!   readiness, isn't part of it.
//! - **Withheld until granted.** [`set_extension_servers`] works out each
//!   server's invocation off the main thread, then hands the managers
//!   ([`crate::extension_servers`]) only those whose invocation is the one
//!   granted. One with no decision, a declined one, or a granted one whose
//!   invocation changed, through an update or a hot reload, is
//!   [pending](pending_extension_consents) until [granted](grant_extension_server)
//!   or [declined](decline_extension_server).
//! - **A change asks again.** A granted server whose invocation changed
//!   keeps the spec it was handed, whole, so one running isn't restarted;
//!   in a later session, where none was handed, it gets its granted
//!   command, arguments and environment, only while that invocation, worked
//!   out again, is still the one granted (its executable resolves as it did,
//!   and every file it names reads as it did), and nothing otherwise.
//! - **Every launch is checked.** Each manager's launcher is a
//!   [`ConsentGate`]: a launch of an extension's server, whether a lazy
//!   start, a crash's restart or a new window's, runs only if its invocation,
//!   worked out again then, is the one granted, and runs the granted
//!   executable by its absolute path. Once the user grants a change, its
//!   last launch at each root that isn't running what's now granted, as it
//!   runs something else, was refused, failed or ended, is made again. Only
//!   the first extension's server of a name is handed, as the managers run
//!   only that one, so a launch is checked against its own grant alone.
//! - **Decisions persist** in [`CONSENT_FILE`] in the state directory
//!   ([`set_extension_consent_file`]): the last decision for each
//!   (canonical extension directory, server name), granted or declined, and
//!   the invocation it was for, written before the decision returns, under a
//!   lock and atomically, so two windows' or processes' writes both land. A
//!   declined invocation isn't asked about again until it changes.
//!
//! [`ExtensionConsent`] is a [`Global`] that changes with each of these, so
//! a view can follow it with `cx.observe_global::<ExtensionConsent>`.

use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    fs, io,
    io::Write as _,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use fe_lsp::{
    ExitInfo, LaunchCancel, LoginPath, ServerIo, ServerLauncher, ServerProcess, ServerSpec,
};
use gpui::{App, AppContext as _, Global, Task};
use kiln_l10n::tr;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::lsp::hand_extension_servers;

/// The consent file's name in the state directory.
pub const CONSENT_FILE: &str = "extensions-lsp-consent.toml";

/// The consent file's header.
const HEADER: &str = "\
# The language servers your extensions contribute that you allowed or
# refused to start, each with exactly what it runs. Remove an entry to be
# asked again.

";

/// Executables that run whatever their arguments say, a script or `-e`
/// code, flagged as such ([`Invocation::runs_arbitrary_code`]).
const INTERPRETERS: &[&str] = &[
    "node", "nodejs", "python", "python2", "python3", "sh", "bash", "zsh", "fish", "dash", "ksh",
    "deno", "bun", "bunx", "npx", "uvx", "ruby", "perl", "php", "lua", "luajit", "pwsh", "java",
    "env",
];

/// A language server an extension contributes, as the host gives it to
/// [`set_extension_servers`].
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionServer {
    pub extension_id: String,
    /// The name people see.
    pub extension_name: String,
    /// As `major.minor.patch`.
    pub extension_version: String,
    /// Its SPDX expression.
    pub extension_license: String,
    /// The extension's directory, absolute.
    pub extension_dir: PathBuf,
    pub spec: ServerSpec,
}

/// What a server runs, which is what consent is given to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    /// The absolute executable, as the launcher would resolve it; the
    /// command as written if it can't be.
    pub command: PathBuf,
    pub args: Vec<String>,
    /// The environment the server's spec sets, the last value of a name
    /// winning, as the launcher sets it.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// For each argument (or flag value, or the executable) that names a
    /// file inside the extension's directory, by that name as written: the
    /// file's SHA-256 in hex, then its canonical path, so a name pointed at
    /// another file is a change even if both files are named.
    #[serde(default)]
    pub script_hashes: BTreeMap<String, String>,
    /// A SHA-256 in hex over every file under the extension's directory
    /// (see [`package_hash`]), so a changed module a named script imports is
    /// a change too. Empty in a grant stored before this existed, which
    /// never equals a computed one: such a grant fails closed and asks again.
    /// [`PACKAGE_INCOMPLETE`] if the package couldn't be read whole: never
    /// granted, so it never matches a grant.
    #[serde(default)]
    pub package_hash: String,
}

impl Invocation {
    /// What `spec`, contributed by the extension in `extension_dir`, runs:
    /// `resolved_command`, else its command as written; its arguments and
    /// environment; and the hash of each file inside `extension_dir` an
    /// argument names, absolute or relative to the directory, alone or as
    /// the value of a `--flag=value`. Reads those files, so call it off the
    /// main thread.
    pub fn of(spec: &ServerSpec, extension_dir: &Path, resolved_command: Option<&Path>) -> Self {
        let command = resolved_command.unwrap_or(&spec.command).to_owned();
        let mut script_hashes = BTreeMap::new();
        let mut package = PACKAGE_INCOMPLETE.to_owned();
        if let Ok(dir) = dunce::canonicalize(extension_dir) {
            package = package_hash(&dir);
            let named = spec.args.iter().flat_map(|arg| {
                let value = arg
                    .strip_prefix('-')
                    .and_then(|flag| flag.split_once('='))
                    .map(|(_, value)| value);
                std::iter::once(arg.as_str()).chain(value)
            });
            let command_name = command.to_string_lossy();
            let names = named
                .filter(|name| !name.is_empty())
                .chain(command.is_absolute().then_some(command_name.as_ref()));
            for name in names {
                if let Some((path, hash)) = hash_inside(&dir.join(name), &dir) {
                    script_hashes.insert(name.to_owned(), format!("{hash} {path}"));
                }
            }
        }
        Self {
            command,
            args: spec.args.clone(),
            env: spec.env.iter().cloned().collect(),
            script_hashes,
            package_hash: package,
        }
    }

    /// Whether the executable is an interpreter (`node`, `python3`, `sh`,
    /// `deno`…), which runs whatever its arguments say: a script, or code
    /// given with `-e`. A versioned name, such as `python3.12`, is one too.
    pub fn runs_arbitrary_code(&self) -> bool {
        let Some(name) = self.command.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        let name = name.strip_suffix(".exe").unwrap_or(name);
        let unversioned =
            name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-');
        INTERPRETERS.contains(&name) || INTERPRETERS.contains(&unversioned)
    }
}

/// What [`package_hash`] is when it couldn't read the whole package. It
/// never equals a granted hash (a grant of it is refused, and a stored
/// grant has either a real hash or none), so a server whose package can't
/// be read completely is neither started nor matched to a grant.
pub(crate) const PACKAGE_INCOMPLETE: &str = "incomplete";

/// The most entries (files, directories and symlinks alike), and bytes of
/// content, [`package_hash`] reads. A package beyond either is [incomplete](PACKAGE_INCOMPLETE), not hashed by
/// size and time: consent is never given on less than the content.
pub const PACKAGE_MAX_FILES: usize = 200_000;
const PACKAGE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// The most content one file may have: the reload watcher reads a listed
/// file whole to hash it, so a bigger one is neither listed there nor
/// consented to ([incomplete](PACKAGE_INCOMPLETE)).
pub const PACKAGE_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// `name` as bytes, losslessly: the platform's own encoding, so two names
/// that differ only in invalid UTF-8 differ.
fn os_bytes(name: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        name.as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        name.encode_wide().flat_map(u16::to_le_bytes).collect()
    }
    #[cfg(not(any(unix, windows)))]
    {
        name.to_string_lossy().into_owned().into_bytes()
    }
}

/// A SHA-256 in hex over everything under the canonical directory `dir`:
/// each entry's relative path (sorted, by its native bytes) and, for a
/// file, its whole content, so an added, removed, renamed or changed file
/// changes it. `node_modules` and the like are included, as they run too.
/// A symlink is never followed: its target's bytes are hashed, so one
/// pointing out of `dir` can't pull in anything outside, and a loop can't
/// hang the walk. [`PACKAGE_INCOMPLETE`] if anything can't be read, or the
/// package is past [`PACKAGE_MAX_FILES`] or [`PACKAGE_MAX_BYTES`]. Reads the
/// whole package: call it off the main thread.
fn package_hash(dir: &Path) -> String {
    package_hash_limited(
        dir,
        PACKAGE_MAX_FILES,
        PACKAGE_MAX_BYTES,
        PACKAGE_MAX_FILE_BYTES,
    )
}

/// [`package_hash`] with the limits given, for tests to reach them.
pub(crate) fn package_hash_limited(
    dir: &Path,
    max_entries: usize,
    max_bytes: u64,
    max_file_bytes: u64,
) -> String {
    package_hash_of(dir, max_entries, max_bytes, max_file_bytes)
        .unwrap_or_else(|| PACKAGE_INCOMPLETE.to_owned())
}

fn package_hash_of(
    dir: &Path,
    max_entries: usize,
    max_bytes: u64,
    max_file_bytes: u64,
) -> Option<String> {
    let mut hasher = Sha256::new();
    let (mut seen, mut bytes) = (0usize, 0u64);
    let mut stack = vec![(dir.to_path_buf(), PathBuf::new())];
    while let Some((path, rel)) = stack.pop() {
        let mut entries = fs::read_dir(&path)
            .ok()?
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        entries.sort_by_key(|entry| os_bytes(&entry.file_name()));
        // Pushed in reverse so the walk visits them in sorted order.
        let mut subdirs = Vec::new();
        for entry in entries {
            // Every entry counts, as the reload watcher counts them, so a
            // package it truncates is one that's refused here.
            seen += 1;
            if seen > max_entries {
                return None;
            }
            let rel = rel.join(entry.file_name());
            let rel_bytes = os_bytes(rel.as_os_str());
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            let kind = meta.file_type();
            if kind.is_symlink() {
                let target = fs::read_link(entry.path()).ok()?;
                feed(
                    &mut hasher,
                    b'l',
                    &[&rel_bytes, &os_bytes(target.as_os_str())],
                );
            } else if kind.is_dir() {
                feed(&mut hasher, b'd', &[&rel_bytes]);
                subdirs.push((entry.path(), rel));
            } else if kind.is_file() {
                bytes = bytes.saturating_add(meta.len());
                if meta.len() > max_file_bytes || bytes > max_bytes {
                    return None;
                }
                let hash = hash_file(&entry.path())?;
                feed(&mut hasher, b'f', &[&rel_bytes, hash.as_bytes()]);
            } else {
                // A socket, device or the like: not something to grant.
                return None;
            }
        }
        stack.extend(subdirs.into_iter().rev());
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// Feeds `hasher` one entry: a tag byte, then each part length-prefixed, so
/// no two entries run together.
fn feed(hasher: &mut Sha256, tag: u8, parts: &[&[u8]]) {
    hasher.update([tag]);
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
}

/// The canonical path of `candidate` and its content's hash, if it is a
/// file inside the canonical directory `dir`, with symlinks followed.
fn hash_inside(candidate: &Path, dir: &Path) -> Option<(String, String)> {
    let path = dunce::canonicalize(candidate).ok()?;
    if !path.starts_with(dir) || !path.is_file() {
        return None;
    }
    let hash = hash_file(&path).unwrap_or_else(|| "unreadable".to_owned());
    Some((path.to_string_lossy().into_owned(), hash))
}

/// The SHA-256 of the file at `path`, in hex.
fn hash_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).ok()?;
    Some(format!("{:x}", hasher.finalize()))
}

/// A server waiting for the user's decision.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingConsent {
    pub extension_id: String,
    pub extension_name: String,
    pub extension_version: String,
    pub extension_license: String,
    /// The extension's directory, canonical.
    pub extension_dir: PathBuf,
    /// The server's name.
    pub server: String,
    /// The languages it serves.
    pub languages: Vec<String>,
    /// What it would run.
    pub invocation: Invocation,
    /// What the user granted it before it changed, which is what runs, if
    /// anything does, until they decide.
    pub previous: Option<Invocation>,
    /// The user declined this very invocation: it isn't to be asked about
    /// again, only offered where the extensions are listed.
    pub declined: bool,
}

/// A decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Decision {
    Granted,
    Declined,
}

/// The last decision for one extension's server, and what it was for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    /// Canonical.
    pub(crate) extension_dir: PathBuf,
    pub(crate) server: String,
    pub(crate) decision: Decision,
    pub(crate) invocation: Invocation,
}

impl Entry {
    fn key(&self) -> Key {
        (self.extension_dir.clone(), self.server.clone())
    }
}

/// An extension's canonical directory and its server's name.
pub(crate) type Key = (PathBuf, String);

/// The consent file's contents.
#[derive(Default, Serialize, Deserialize)]
struct ConsentFile {
    #[serde(default)]
    server: Vec<Entry>,
}

/// The consent file at a path.
pub(crate) struct ConsentStore {
    path: PathBuf,
}

impl ConsentStore {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The decisions in the file: none if it's missing, or, with a warning,
    /// if it can't be read.
    pub(crate) fn load(&self) -> BTreeMap<Key, Entry> {
        self.read().unwrap_or_else(|error| {
            log::warn!("{error}; asking about extensions' language servers again");
            BTreeMap::new()
        })
    }

    /// The decisions in the file, none if it's missing; why if it can't be
    /// read or parsed.
    pub(crate) fn read(&self) -> Result<BTreeMap<Key, Entry>, String> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(error) => return Err(self.unreadable(&error)),
        };
        let file: ConsentFile = toml::from_str(&text).map_err(|error| self.unreadable(&error))?;
        Ok(file
            .server
            .into_iter()
            .map(|entry| (entry.key(), entry))
            .collect())
    }

    /// Why the file can't be read: `error`.
    fn unreadable(&self, error: &dyn std::fmt::Display) -> String {
        tr!(
            "workbench-extensions-lsp-consent-unreadable",
            path = self.path.display().to_string(),
            error = error.to_string()
        )
        .into()
    }

    /// Records `entry` over its server's last, keeping every other entry,
    /// another writer's too: the file is read again and written under a
    /// lock, through a temporary file renamed over it.
    pub(crate) fn record(&self, entry: Entry) -> io::Result<()> {
        locked(&self.path, || {
            let mut entries = self.load();
            entries.insert(entry.key(), entry);
            let file = ConsentFile {
                server: entries.into_values().collect(),
            };
            let text = toml::to_string(&file).map_err(io::Error::other)?;
            write(&self.path, format!("{HEADER}{text}").as_bytes())
        })?
    }
}

/// Runs `f` holding an exclusive lock on `<path>.lock`, made beside `path`
/// and never removed, blocking while another, in this process or another,
/// holds it.
fn locked<T>(path: &Path, f: impl FnOnce() -> T) -> io::Result<T> {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".lock");
    let lock_path = path.with_file_name(name);
    create_dir(lock_path.parent().unwrap_or(Path::new(".")))?;
    let lock = private(
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false),
    )
    .open(&lock_path)?;
    lock.lock()?;
    // Released when `lock` drops, after `f`.
    Ok(f())
}

/// Writes `contents` to `path` through a temporary file beside it, renamed
/// over it.
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
            private(fs::OpenOptions::new().write(true).create_new(true)).open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Creates `dir` and its missing ancestors, the new ones only the user's.
fn create_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// `options` making a file only the user may read and write.
fn private(options: &mut fs::OpenOptions) -> &mut fs::OpenOptions {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
    options
}

/// A server as last worked out off the main thread.
#[derive(Clone)]
struct Evaluated {
    server: ExtensionServer,
    invocation: Invocation,
}

impl Evaluated {
    fn key(&self) -> Key {
        (
            self.server.extension_dir.clone(),
            self.server.spec.name.clone(),
        )
    }
}

/// What an evaluation found.
struct Evaluation {
    servers: Vec<Evaluated>,
    /// The file's decisions.
    entries: BTreeMap<Key, Entry>,
    /// For each server whose granted invocation differs from its own,
    /// whether that granted invocation, worked out again from its command,
    /// arguments and environment as they'd run now, is still the one
    /// granted: its executable resolves as it did and every file it names
    /// reads as it did.
    intact: HashMap<Key, (Invocation, bool)>,
}

/// An extension's server as handed to the managers: what a launch of its
/// spec must run ([`ConsentGate`]).
#[derive(Clone, Debug)]
pub(crate) struct Grant {
    spec: ServerSpec,
    /// The extension's canonical directory.
    dir: PathBuf,
    invocation: Invocation,
}

/// The grant for each server handed to the managers, by server name: one
/// each, the first extension's to contribute that name, as the managers
/// use only that one. Shared with every manager's [`ConsentGate`].
type Grants = Arc<Mutex<HashMap<String, Grant>>>;

/// The extensions' servers, the user's decisions on them, and which are
/// handed to the managers and which wait. Changes with each, so a view can
/// observe it.
#[derive(Default)]
pub struct ExtensionConsent {
    file: Option<PathBuf>,
    /// As the host last gave them.
    servers: Vec<ExtensionServer>,
    evaluated: Vec<Evaluated>,
    /// The file's decisions as the last evaluation read them.
    entries: BTreeMap<Key, Entry>,
    /// This session's decisions, over the file's.
    decided: BTreeMap<Key, Entry>,
    intact: HashMap<Key, (Invocation, bool)>,
    /// What each server was last handed to the managers as, with the
    /// invocation granted for it.
    handed: HashMap<Key, (Invocation, ServerSpec)>,
    grants: Grants,
    pending: Vec<PendingConsent>,
    evaluating: Option<Task<()>>,
    /// What a [`ConsentGate`] tells when a launch finds what it would run
    /// changed, or was decided under a grant since replaced, so the
    /// servers are worked out again, and handed with their restarts.
    wake: Option<async_channel::Sender<()>>,
    _woken: Option<Task<()>>,
}

impl Global for ExtensionConsent {}

/// Has extensions' servers' decisions read from and written to `path`
/// (`<state dir>/`[`CONSENT_FILE`]). Without it, decisions last only for
/// the session. The servers already given are worked out again.
pub fn set_extension_consent_file(cx: &mut App, path: PathBuf) {
    let state = cx.default_global::<ExtensionConsent>();
    state.file = Some(path);
    state.decided.clear();
    evaluate(cx);
}

/// The language servers extensions contribute, in extension order. Each
/// one's [`Invocation`] is worked out off the main thread; then the
/// managers get only those the user granted that invocation to, plus, for
/// one whose invocation changed since its grant, what was granted, and the
/// rest are [pending](pending_extension_consents). Not to be called while a
/// workbench is being updated.
pub fn set_extension_servers(cx: &mut App, servers: Vec<ExtensionServer>) {
    cx.default_global::<ExtensionConsent>().servers = servers;
    evaluate(cx);
}

/// The servers waiting for the user's decision, in extension order; one
/// [declined](PendingConsent::declined) isn't to be asked about again.
pub fn pending_extension_consents(cx: &App) -> Vec<PendingConsent> {
    cx.try_global::<ExtensionConsent>()
        .map(|state| state.pending.clone())
        .unwrap_or_default()
}

/// An extension's server the user granted what it runs now: by its
/// extension's canonical directory and its name, the invocation granted,
/// and whether the managers have it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GrantedServer {
    pub(crate) key: Key,
    pub(crate) invocation: Invocation,
    /// Not when an earlier extension has a server of its name, which the
    /// managers run instead.
    pub(crate) running: bool,
}

/// The servers granted what they'd run now, in extension order, as last
/// evaluated: one whose invocation changed since is
/// [pending](pending_extension_consents) instead.
pub(crate) fn granted_extension_servers(cx: &App) -> Vec<GrantedServer> {
    let Some(state) = cx.try_global::<ExtensionConsent>() else {
        return Vec::new();
    };
    let mut granted = Vec::new();
    for evaluated in &state.evaluated {
        let key = evaluated.key();
        let entry = state.decided.get(&key).or_else(|| state.entries.get(&key));
        let Some(entry) = entry.filter(|entry| {
            entry.decision == Decision::Granted && entry.invocation == evaluated.invocation
        }) else {
            continue;
        };
        granted.push(GrantedServer {
            running: state.handed.contains_key(&key),
            invocation: entry.invocation.clone(),
            key,
        });
    }
    granted
}

/// Lets `pending`'s server run exactly what it shows, from now on and in
/// later sessions, and starts it, restarting what ran before it. Each
/// launch checks it still runs that ([`ConsentGate`]). The decision is
/// written to the consent file before this returns, and holds for this
/// session even if that write fails, which is returned.
pub fn grant_extension_server(cx: &mut App, pending: &PendingConsent) -> io::Result<()> {
    if pending.invocation.package_hash == PACKAGE_INCOMPLETE {
        // The whole package couldn't be read: nothing complete to consent to.
        return Err(io::ErrorKind::InvalidData.into());
    }
    decide(cx, pending, Decision::Granted)
}

/// Keeps `pending`'s server from running what it shows, and from being
/// asked about again until that changes. What was granted it before stops.
/// Written as [`grant_extension_server`] writes.
pub fn decline_extension_server(cx: &mut App, pending: &PendingConsent) -> io::Result<()> {
    decide(cx, pending, Decision::Declined)
}

/// Records a decision on `pending`, in this session at once and in the
/// consent file before returning: on the main thread, as a quit right
/// after mustn't lose it (a lost decline would let a grant it replaced
/// start the server again). The file is small and its lock held only to
/// rewrite it. The servers are then worked out again, so a change made
/// while the user decided is asked about.
fn decide(cx: &mut App, pending: &PendingConsent, decision: Decision) -> io::Result<()> {
    let entry = Entry {
        extension_dir: pending.extension_dir.clone(),
        server: pending.server.clone(),
        decision,
        invocation: pending.invocation.clone(),
    };
    let state = cx.default_global::<ExtensionConsent>();
    state.decided.insert(entry.key(), entry.clone());
    let written = match &state.file {
        Some(file) => ConsentStore::new(file).record(entry).inspect_err(|error| {
            log::error!(
                "can't record the decision on {}'s language server {} in {}: {error}",
                pending.extension_id,
                pending.server,
                file.display()
            );
        }),
        None => Ok(()),
    };
    apply(cx);
    evaluate(cx);
    written
}

/// Works out the servers' invocations and reads the decisions off the main
/// thread, then [`apply`]s them, dropping an evaluation under way.
fn evaluate(cx: &mut App) {
    let state = cx.default_global::<ExtensionConsent>();
    let servers = state.servers.clone();
    let file = state.file.clone();
    let decided = state.decided.clone();
    let task = cx.spawn(async move |cx| {
        let evaluation = cx
            .background_spawn(async move { evaluation(servers, file, decided) })
            .await;
        cx.update(|cx| {
            let state = cx.global_mut::<ExtensionConsent>();
            state.evaluated = evaluation.servers;
            state.entries = evaluation.entries;
            state.intact = evaluation.intact;
            apply(cx);
        });
    });
    cx.global_mut::<ExtensionConsent>().evaluating = Some(task);
}

/// What [`evaluate`] works out: blocking.
fn evaluation(
    servers: Vec<ExtensionServer>,
    file: Option<PathBuf>,
    decided: BTreeMap<Key, Entry>,
) -> Evaluation {
    let entries = file
        .map(|file| ConsentStore::new(file).load())
        .unwrap_or_default();
    let servers: Vec<Evaluated> = servers
        .into_iter()
        .map(|mut server| {
            if let Ok(dir) = dunce::canonicalize(&server.extension_dir) {
                server.extension_dir = dir;
            }
            let invocation = invocation_now(&server.spec, &server.extension_dir);
            Evaluated { server, invocation }
        })
        .collect();
    let mut intact = HashMap::new();
    for server in &servers {
        let key = server.key();
        let entry = decided.get(&key).or_else(|| entries.get(&key));
        if let Some(entry) = entry
            && entry.decision == Decision::Granted
            && entry.invocation != server.invocation
        {
            let granted = entry.invocation.clone();
            let kept = granted_spec(&granted, &server.server.spec);
            let same = invocation_now(&kept, &server.server.extension_dir) == granted;
            intact.insert(key, (granted, same));
        }
    }
    Evaluation {
        servers,
        entries,
        intact,
    }
}

/// What `spec`, of the extension in `dir`, runs now. Blocking.
fn invocation_now(spec: &ServerSpec, dir: &Path) -> Invocation {
    Invocation::of(spec, dir, resolve_command(spec).as_deref())
}

/// `spec` running `granted`'s command, arguments and environment.
fn granted_spec(granted: &Invocation, spec: &ServerSpec) -> ServerSpec {
    ServerSpec {
        command: granted.command.clone(),
        args: granted.args.clone(),
        env: granted.env.clone().into_iter().collect(),
        ..spec.clone()
    }
}

/// `spec`'s command as the launcher would run it: a bare name looked up on
/// the `PATH` its spec sets, else the login shell's; one with a `~` under
/// the home directory. `None` for a bare name not found. A relative one is
/// kept relative, as the launcher resolves it under the folder it starts
/// in. Blocking: may run the login shell.
fn resolve_command(spec: &ServerSpec) -> Option<PathBuf> {
    let spec_path = spec
        .env
        .iter()
        .rev()
        .find(|(name, _)| name == "PATH")
        .map(|(_, path)| OsString::from(path));
    // The tests don't run the user's login shell.
    if cfg!(test)
        && spec_path.is_none()
        && !spec.command.as_os_str().as_encoded_bytes().contains(&b'/')
    {
        return None;
    }
    LoginPath::system().resolve(&spec.command, spec_path.as_deref(), Path::new(""))
}

/// Hands the managers the servers the decisions allow, with their grants
/// first, so that every launch from then on is checked against them; and
/// lists the others as pending.
fn apply(cx: &mut App) {
    let state = cx.default_global::<ExtensionConsent>();
    let mut specs = Vec::new();
    let mut pending = Vec::new();
    let mut handed = HashMap::new();
    let mut grants: HashMap<String, Grant> = HashMap::new();
    for evaluated in &state.evaluated {
        let key = evaluated.key();
        let server = &evaluated.server;
        let entry = state.decided.get(&key).or_else(|| state.entries.get(&key));
        let ask = |previous: Option<Invocation>, declined: bool| PendingConsent {
            extension_id: server.extension_id.clone(),
            extension_name: server.extension_name.clone(),
            extension_version: server.extension_version.clone(),
            extension_license: server.extension_license.clone(),
            extension_dir: server.extension_dir.clone(),
            server: server.spec.name.clone(),
            languages: server.spec.languages.clone(),
            invocation: evaluated.invocation.clone(),
            previous,
            declined,
        };
        let spec = match entry {
            Some(entry) if entry.invocation == evaluated.invocation => match entry.decision {
                Decision::Granted => Some(server.spec.clone()),
                Decision::Declined => {
                    pending.push(ask(None, true));
                    None
                }
            },
            Some(entry) if entry.decision == Decision::Granted => {
                pending.push(ask(Some(entry.invocation.clone()), false));
                let granted = &entry.invocation;
                match state.handed.get(&key) {
                    // As handed, whole, so the manager restarts nothing
                    // until the user decides.
                    Some((invocation, spec)) if invocation == granted => Some(spec.clone()),
                    // Nothing of it was handed, so nothing of it runs: the
                    // granted invocation, if it can still run as granted.
                    _ => state
                        .intact
                        .get(&key)
                        .filter(|(checked, intact)| checked == granted && *intact)
                        .map(|_| granted_spec(granted, &server.spec)),
                }
            }
            _ => {
                pending.push(ask(None, false));
                None
            }
        };
        let Some(spec) = spec else {
            continue;
        };
        // The managers run only the first of a name, so that's the one its
        // launches are checked against; a later one isn't handed at all.
        if grants.contains_key(&spec.name) {
            log::warn!(
                "extension {}'s language server {} is ignored: an earlier extension has one \
                 of that name",
                server.extension_id,
                spec.name
            );
            continue;
        }
        let invocation = entry.expect("granted").invocation.clone();
        let grant = Grant {
            spec: spec.clone(),
            dir: server.extension_dir.clone(),
            invocation: invocation.clone(),
        };
        grants.insert(spec.name.clone(), grant);
        handed.insert(key, (invocation, spec.clone()));
        specs.push(spec);
    }
    state.handed = handed;
    state.pending = pending;
    *lock(&state.grants) = grants;
    hand_extension_servers(cx, specs);
    // Tells the views that follow the consents.
    gpui::BorrowAppContext::update_global::<ExtensionConsent, _>(cx, |_, _| {});
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A manager's launcher, wrapping the host's: a launch of an extension's
/// server, its spec as handed (by name, command and arguments), runs only
/// if what it would run now is the invocation granted it, worked out again
/// at each launch, so a lazy start, a crash's restart or a new window's
/// manager never runs a changed script or another executable. The
/// executable is bound to the one granted: the launcher gets its absolute
/// path, not a name to look up again. Any other launch, such as a
/// built-in's or one the user's settings define, passes through.
///
/// It keeps the last launch it checked of each server at each root,
/// whatever came of it (running, ended, refused or failed), with the grant
/// it was checked against, so that once the user grants a change, those
/// not running what's granted now are restarted ([`ConsentGate::stale`]).
/// A launch that finds what it would run changed, or that was checked
/// against a grant since replaced, wakes the app to work the servers out
/// again: the change is then asked about, and the launch restarted if
/// that's due.
pub(crate) struct ConsentGate {
    inner: Arc<dyn ServerLauncher>,
    grants: Grants,
    attempts: Arc<Mutex<Attempts>>,
    wake: async_channel::Sender<()>,
}

/// The last launch checked of each server at each root.
#[derive(Default)]
struct Attempts {
    next: u64,
    last: HashMap<(String, PathBuf), Attempt>,
}

struct Attempt {
    /// Its own number, so an ended process only marks its own launch.
    id: u64,
    /// The invocation its server was granted as it was checked.
    granted: Invocation,
    /// What it runs, while it does.
    running: Option<Invocation>,
}

impl Attempt {
    /// Whether it's to be launched again under `grant`: it runs other than
    /// what's granted, or doesn't run and was checked against another
    /// grant.
    fn stale(&self, grant: &Invocation) -> bool {
        match &self.running {
            Some(running) => running != grant,
            None => &self.granted != grant,
        }
    }
}

impl ConsentGate {
    /// A gate over `inner`, checking against the app's grants.
    pub(crate) fn new(cx: &mut App, inner: Arc<dyn ServerLauncher>) -> Self {
        Self {
            inner,
            grants: cx.default_global::<ExtensionConsent>().grants.clone(),
            attempts: Arc::default(),
            wake: wake(cx),
        }
    }

    /// The server and root of each last launch to make again under the
    /// grants as they are now ([`Attempt::stale`]). Each is taken as
    /// checked against those grants, not running, so it's given once.
    pub(crate) fn stale(&self) -> Vec<(String, PathBuf)> {
        let grants = lock(&self.grants);
        let mut attempts = lock(&self.attempts);
        let mut restart = Vec::new();
        for ((name, root), attempt) in attempts.last.iter_mut() {
            let Some(grant) = grants.get(name) else {
                continue;
            };
            if attempt.stale(&grant.invocation) {
                attempt.granted = grant.invocation.clone();
                attempt.running = None;
                restart.push((name.clone(), root.clone()));
            }
        }
        restart
    }

    /// Keeps a launch of `name` at `root`, checked against `granted`,
    /// running `running` if it launched; returns its number. Wakes the app
    /// if that grant has been replaced since. One whose server was stopped
    /// meanwhile (`cancel`) isn't kept: its replacement is the last launch,
    /// and a late record of the old one would have it restarted again.
    fn record(
        &self,
        name: &str,
        root: &Path,
        granted: Invocation,
        running: Option<Invocation>,
        cancel: &LaunchCancel,
    ) -> u64 {
        let replaced = lock(&self.grants)
            .get(name)
            .is_none_or(|grant| grant.invocation != granted);
        let id = {
            let mut attempts = lock(&self.attempts);
            // Checked holding the lock `stale` takes, so a server stopped
            // for its replacement is either kept before that restarts
            // things or not at all.
            if cancel.is_cancelled() {
                return 0;
            }
            attempts.next += 1;
            let id = attempts.next;
            let attempt = Attempt {
                id,
                granted,
                running,
            };
            attempts
                .last
                .insert((name.to_owned(), root.to_owned()), attempt);
            id
        };
        if replaced {
            self.wake.try_send(()).ok();
        }
        id
    }
}

/// The channel [`ConsentGate`]s wake the app through, listened to the
/// first time.
fn wake(cx: &mut App) -> async_channel::Sender<()> {
    if let Some(wake) = &cx.default_global::<ExtensionConsent>().wake {
        return wake.clone();
    }
    let (sender, woken) = async_channel::unbounded::<()>();
    let task = cx.spawn(async move |cx| {
        while woken.recv().await.is_ok() {
            while woken.try_recv().is_ok() {}
            cx.update(evaluate);
        }
    });
    let state = cx.global_mut::<ExtensionConsent>();
    state.wake = Some(sender.clone());
    state._woken = Some(task);
    sender
}

impl ServerLauncher for ConsentGate {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        let grant = lock(&self.grants)
            .get(&spec.name)
            .filter(|grant| grant.spec.command == spec.command && grant.spec.args == spec.args)
            .cloned();
        let Some(grant) = grant else {
            return self.inner.launch(spec, root, cancel);
        };
        let resolved = resolve_command(spec);
        if Invocation::of(&grant.spec, &grant.dir, resolved.as_deref()) != grant.invocation {
            self.record(&spec.name, root, grant.invocation, None, cancel);
            // Worked out again, the change is asked about.
            self.wake.try_send(()).ok();
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                tr!(
                    "workbench-extensions-lsp-consent-changed-since",
                    server = spec.name.as_str()
                )
                .to_string(),
            ));
        }
        let Some(command) = resolved.filter(|command| command.is_absolute()) else {
            self.record(&spec.name, root, grant.invocation, None, cancel);
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                tr!(
                    "workbench-extensions-lsp-consent-not-found",
                    command = spec.command.display().to_string()
                )
                .to_string(),
            ));
        };
        let bound = ServerSpec {
            command,
            ..spec.clone()
        };
        let mut io = match self.inner.launch(&bound, root, cancel) {
            Ok(io) => io,
            Err(error) => {
                self.record(&spec.name, root, grant.invocation, None, cancel);
                return Err(error);
            }
        };
        let running = Some(grant.invocation.clone());
        let id = self.record(&spec.name, root, grant.invocation, running, cancel);
        io.process = Box::new(Tracked {
            inner: io.process,
            at: (spec.name.clone(), root.to_owned()),
            id,
            attempts: self.attempts.clone(),
        });
        Ok(io)
    }
}

/// A gated launch's process, which marks its launch no longer running
/// when it's dropped.
struct Tracked {
    inner: Box<dyn ServerProcess>,
    at: (String, PathBuf),
    id: u64,
    attempts: Arc<Mutex<Attempts>>,
}

impl ServerProcess for Tracked {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        self.inner.exited()
    }

    fn kill(&mut self) {
        self.inner.kill();
    }

    fn pid(&self) -> Option<u32> {
        self.inner.pid()
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        if let Some(attempt) = lock(&self.attempts).last.get_mut(&self.at)
            && attempt.id == self.id
        {
            attempt.running = None;
        }
    }
}
