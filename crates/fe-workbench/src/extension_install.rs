//! Extensions: Install from File or Folder: an extension, declarative or a
//! plugin, from a `.zip`, a `.tar.gz` or `.tgz`, or a folder, into the
//! user's extensions directory (extensions record §2's archive form).
//! Installs are local and unsigned: a plugin's grant prompt is its trust
//! gate, a language server's consent prompt its.
//!
//! - **Sources.** The archive or folder holds `extension.toml` at its top
//!   level, or inside a single folder there. A folder is copied, never
//!   linked.
//! - **Safety.** Everything is unpacked into a fresh directory inside the
//!   extensions directory ([`stage`]), then renamed into place. An absolute
//!   entry, a `..` escape, a symlink or hard link (a folder's file with
//!   other names too), a device or other special file, whatever its name,
//!   more than [`MAX_ENTRIES`] entries (a tar's own headers too, a zip's
//!   names each used once) or more than [`MAX_UNPACKED`] bytes unpacked (a
//!   tar's every entry, read or skipped, its headers at most
//!   [`MAX_TAR_HEADER`]) refuses the install and leaves nothing behind.
//!   Nothing unpacked is ever a link, so nothing written follows one.
//! - **Check.** The manifest goes through kiln's extension check (its id,
//!   name, version, licence and `[engine]`) and a plugin entry through
//!   [`PluginManifest::from_extension`] as the user's; an error refuses the
//!   install with its text.
//! - **Place.** As `<extensions dir>/<id>` ([`Staged::place`]). The command
//!   asks first when that id is installed, in whatever directory, naming
//!   both versions; replacing moves the old directories aside first and
//!   back on a failure, and one that can't be moved back is kept, the error
//!   saying where. A `<id>` directory holding something else refuses it. The extensions
//!   service's hot reload picks it up, and a plugin then waits for its grant
//!   as any new plugin does.
//!
//! The staging directory never has an `extension.toml` of its own, so a
//! rescan while it's there doesn't take it for an extension.

use std::{
    fmt, fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{
    App, AppContext as _, AsyncApp, AsyncWindowContext, Context, Global, PathPromptOptions,
    PromptLevel, SharedString, WeakEntity, Window,
};
use kiln::core::{
    ApiVersion, Diagnostic,
    extension::{ExtensionApi, MANIFEST_FILE, load_manifest},
};
use kiln_l10n::tr;
use kiln_plugin::manifest::{ExtensionOrigin, PluginManifest};

use crate::{Workbench, plugin_host::plugin_kind, plugin_manage::PREVIEW_SETTING};

/// The most entries an archive or folder may have, folders included.
pub(crate) const MAX_ENTRIES: usize = 10_000;

/// The most bytes its files may come to, unpacked; in a tar, every
/// entry's contents, read or skipped.
pub(crate) const MAX_UNPACKED: u64 = 64 * 1024 * 1024;

/// The most bytes a tar's long-name or pax header may have.
pub(crate) const MAX_TAR_HEADER: u64 = 64 * 1024;

/// The replace prompt's buttons, and the index of Replace: an answer is
/// matched by its button's index, never its title.
fn replace_buttons() -> [SharedString; 2] {
    [
        tr!("workbench-extensions-install-replace"),
        tr!("workbench-extensions-cancel"),
    ]
}
const REPLACE: usize = 0;

/// The staging directory's name starts with this.
const STAGING_PREFIX: &str = ".installing-";

/// An extension's version, `(major, minor, patch)`.
pub(crate) type Version = (u32, u32, u32);

/// An extension installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: (u32, u32, u32),
    /// `<extensions dir>/<id>`.
    pub dir: PathBuf,
    /// Whether it contributes a plugin.
    pub plugin: bool,
}

/// Why an install was refused or failed. Nothing it unpacked is left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// What was picked isn't a `.zip`, `.tar.gz`, `.tgz` or folder.
    Unsupported,
    /// What it holds isn't installed: an entry, a limit or the manifest.
    Refused(String),
    /// Reading it or writing the extension failed.
    Io(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str(&tr!("workbench-extensions-install-unsupported")),
            Self::Refused(reason) | Self::Io(reason) => f.write_str(reason),
        }
    }
}

impl From<io::Error> for InstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Installs the extension in `source` into `extensions_dir`, replacing one
/// installed with its id. Reads and writes files: for a background thread.
pub fn install_extension(source: &Path, extensions_dir: &Path) -> Result<Installed, InstallError> {
    stage(source, extensions_dir, &check_api(Vec::new()))?.place()
}

/// The check an installed extension's manifest goes through: the app's id
/// and extension API, IDs outside the `reserved` command namespaces, and
/// `[contributes.plugin]`. The app's other kinds are checked as they load.
pub(crate) fn check_api(reserved: Vec<String>) -> ExtensionApi {
    ExtensionApi {
        app: fe_config::paths::app_id().short,
        // As the app's own: 1.1 added snippets.
        version: ApiVersion { major: 1, minor: 1 },
        reserved,
        kinds: vec![plugin_kind()],
        allowed_licenses: None,
        only_kinds: None,
    }
}

/// An extension unpacked and checked, not yet in place. Dropped, what was
/// unpacked is removed.
#[derive(Debug)]
pub(crate) struct Staged {
    staging: Staging,
    /// The extension's directory, inside the staging directory.
    root: PathBuf,
    installed: Installed,
    /// The installed version it replaces, if one is installed with its id.
    replaces: Option<String>,
    /// The directories holding an extension with its id, the one that loads
    /// first.
    replaced: Vec<PathBuf>,
}

impl Staged {
    /// What it installs as.
    pub(crate) fn installed(&self) -> &Installed {
        &self.installed
    }

    /// The version of the extension already installed with its id, as its
    /// manifest has it ("unknown" if it doesn't), if there's one.
    pub(crate) fn replaces(&self) -> Option<&str> {
        self.replaces.as_deref()
    }

    /// Where the extension it replaces is installed, if one is.
    pub(crate) fn replaced_dir(&self) -> Option<&Path> {
        self.replaced.first().map(PathBuf::as_path)
    }

    /// Moves it into place: each directory installed with its id is moved
    /// aside first, and back if a move fails, then removed with the staging
    /// directory.
    pub(crate) fn place(self) -> Result<Installed, InstallError> {
        self.place_with(|from, to| fs::rename(from, to))
    }

    /// [`Staged::place`], moving with `rename`.
    fn place_with(
        mut self,
        rename: impl Fn(&Path, &Path) -> io::Result<()>,
    ) -> Result<Installed, InstallError> {
        let mut moved = Vec::new();
        for (index, dir) in self.replaced.iter().enumerate() {
            let aside = self.staging.dir.join(format!("replaced-{index}"));
            if let Err(error) = rename(dir, &aside) {
                return Err(self.put_back(moved, error, &rename));
            }
            moved.push((dir.clone(), aside));
        }
        if let Err(error) = rename(&self.root, &self.installed.dir) {
            return Err(self.put_back(moved, error, &rename));
        }
        Ok(self.installed.clone())
    }

    /// Moves each `(dir, aside)` in `moved` back, after `error`. One that
    /// can't be is kept where it is, with the staging directory, and the
    /// error says where.
    fn put_back(
        &mut self,
        moved: Vec<(PathBuf, PathBuf)>,
        error: io::Error,
        rename: impl Fn(&Path, &Path) -> io::Result<()>,
    ) -> InstallError {
        let mut kept = Vec::new();
        for (dir, aside) in moved.into_iter().rev() {
            if let Err(restoring) = rename(&aside, &dir) {
                log::warn!(
                    "extension install: putting {} back: {restoring}",
                    dir.display()
                );
                kept.push(aside.display().to_string());
            }
        }
        if kept.is_empty() {
            return error.into();
        }
        self.staging.keep = true;
        InstallError::Io(
            tr!(
                "workbench-extensions-install-kept-aside",
                error = error.to_string(),
                dirs = kept.join(", ")
            )
            .into(),
        )
    }
}

/// Unpacks or copies `source` into a fresh directory in `extensions_dir`
/// and checks it against `api`. Refused, nothing is left.
pub(crate) fn stage(
    source: &Path,
    extensions_dir: &Path,
    api: &ExtensionApi,
) -> Result<Staged, InstallError> {
    let kind = Source::of(source)?;
    fs::create_dir_all(extensions_dir)?;
    let extensions_dir = fs::canonicalize(extensions_dir)?;
    if kind == Source::Folder && extensions_dir.starts_with(fs::canonicalize(source)?) {
        let reason = tr!("workbench-extensions-install-extensions-folder");
        return Err(refused(reason));
    }
    let staging = Staging::new(&extensions_dir)?;
    let unpacked = staging.dir.join("unpacked");
    fs::create_dir(&unpacked)?;
    let mut unpack = Unpack {
        root: unpacked.clone(),
        entries: 0,
        bytes: 0,
    };
    match kind {
        Source::Zip => unpack.zip(source)?,
        Source::TarGz => unpack.tar_gz(source)?,
        Source::Folder => unpack.folder(source, Path::new(""))?,
    }
    let root = extension_root(&unpacked)?;
    let installed = check(&root, &extensions_dir, api)?;
    let replaced = installed_with(&extensions_dir, &installed.id)?;
    // Taken by something else, it isn't replaced unasked.
    let taken = fs::symlink_metadata(&installed.dir).is_ok();
    if taken && !replaced.iter().any(|dir| same_file(dir, &installed.dir)) {
        return Err(refused(tr!(
            "workbench-extensions-install-taken",
            dir = installed.dir.display().to_string(),
            id = installed.id.as_str()
        )));
    }
    let replaces = replaced.first().map(|dir| installed_version(dir));
    Ok(Staged {
        staging,
        root,
        installed,
        replaces,
        replaced,
    })
}

/// What an install is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Zip,
    TarGz,
    Folder,
}

impl Source {
    fn of(path: &Path) -> Result<Self, InstallError> {
        if fs::metadata(path)?.is_dir() {
            return Ok(Self::Folder);
        }
        let name = (path.file_name())
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.ends_with(".zip") {
            Ok(Self::Zip)
        } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Ok(Self::TarGz)
        } else {
            Err(InstallError::Unsupported)
        }
    }
}

/// A fresh directory in the extensions directory, removed when dropped
/// unless it's to be kept.
#[derive(Debug)]
struct Staging {
    dir: PathBuf,
    keep: bool,
}

impl Staging {
    fn new(extensions_dir: &Path) -> io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos());
        for attempt in 0u32..100 {
            let name = format!("{STAGING_PREFIX}{}-{nanos}-{attempt}", std::process::id());
            let dir = extensions_dir.join(name);
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(Self { dir, keep: false }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            tr!("workbench-extensions-install-no-staging-dir").to_string(),
        ))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Err(error) = fs::remove_dir_all(&self.dir)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!(
                "extension install: removing {}: {error}",
                self.dir.display()
            );
        }
    }
}

/// Unpacking into `root`, counting what's written.
struct Unpack {
    root: PathBuf,
    entries: usize,
    bytes: u64,
}

/// What an entry is.
enum Kind {
    Dir,
    File { executable: bool },
    Symlink,
    HardLink,
    Special,
}

impl Unpack {
    fn zip(&mut self, source: &Path) -> Result<(), InstallError> {
        let mut archive = zip::ZipArchive::new(fs::File::open(source)?).map_err(zip_error)?;
        // The archive keeps one entry a name, so a name used twice could
        // hide an entry, and the count.
        if archive.has_duplicate_names() {
            return Err(refused(tr!("workbench-extensions-install-duplicate-names")));
        }
        if archive.len() > MAX_ENTRIES {
            return Err(too_many());
        }
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(zip_error)?;
            let name = file.name().map_err(zip_error)?.into_owned();
            let mode = file.unix_mode();
            let executable = mode.is_some_and(|mode| mode & 0o111 != 0);
            // The Unix file type, if it has one, over what its name says.
            let kind = match mode.map(|mode| mode & 0o170_000) {
                Some(0o100_000) => Kind::File { executable },
                Some(0o040_000) => Kind::Dir,
                Some(0o120_000) => Kind::Symlink,
                Some(0) | None if file.is_symlink() => Kind::Symlink,
                Some(0) | None if file.is_dir() => Kind::Dir,
                Some(0) | None => Kind::File { executable },
                Some(_) => Kind::Special,
            };
            self.entry(&name, kind, &mut file)?;
        }
        Ok(())
    }

    /// Reads the tar header by header, its own long names and pax headers
    /// too, so each counts as an entry and what it holds against the bytes
    /// unpacked before it's read or skipped.
    fn tar_gz(&mut self, source: &Path) -> Result<(), InstallError> {
        let gz = flate2::read::GzDecoder::new(io::BufReader::new(fs::File::open(source)?));
        let mut archive = tar::Archive::new(gz);
        // The next entry's name, from a GNU long-name or pax header.
        let mut long_name: Option<Vec<u8>> = None;
        // And its size, from a pax header.
        let mut pax_size: Option<u64> = None;
        let mut payloads = 0u64;
        for entry in archive.entries()?.raw(true) {
            let mut entry = entry?;
            self.count()?;
            let size = entry.header().entry_size()?;
            payloads = payloads.saturating_add(size);
            if payloads > MAX_UNPACKED {
                return Err(too_big());
            }
            let kind = entry.header().entry_type();
            if kind.is_gnu_longname()
                || kind.is_gnu_longlink()
                || kind.is_pax_local_extensions()
                || kind.is_pax_global_extensions()
            {
                if size > MAX_TAR_HEADER {
                    let reason = tr!(
                        "workbench-extensions-install-header-too-long",
                        kib = (MAX_TAR_HEADER >> 10).to_string()
                    );
                    return Err(refused(reason));
                }
                let mut payload = Vec::new();
                (&mut entry).take(size).read_to_end(&mut payload)?;
                if kind.is_gnu_longname() {
                    let end = payload.iter().position(|&byte| byte == 0);
                    payload.truncate(end.unwrap_or(payload.len()));
                    long_name = Some(payload);
                } else if kind.is_pax_local_extensions() {
                    let pax = pax_records(&payload)?;
                    long_name = pax.path.or(long_name);
                    pax_size = pax.size.or(pax_size);
                }
                continue;
            }
            // What's read next is the size this header says, so a pax size
            // can only agree with it.
            match pax_size.take() {
                Some(declared) if declared > MAX_UNPACKED => return Err(too_big()),
                Some(declared) if declared != size => {
                    let reason = tr!(
                        "workbench-extensions-install-pax-size",
                        declared = declared.to_string(),
                        size = size.to_string()
                    );
                    return Err(refused(reason));
                }
                _ => {}
            }
            let executable = entry.header().mode().is_ok_and(|mode| mode & 0o111 != 0);
            let kind = if kind.is_file() || kind.is_contiguous() {
                Kind::File { executable }
            } else if kind.is_dir() {
                Kind::Dir
            } else if kind.is_symlink() {
                Kind::Symlink
            } else if kind.is_hard_link() {
                Kind::HardLink
            } else {
                Kind::Special
            };
            let path = match long_name.take() {
                Some(name) => name,
                None => entry.path_bytes().into_owned(),
            };
            let name = std::str::from_utf8(&path)
                .map_err(|_| {
                    let name = String::from_utf8_lossy(&path);
                    refused(tr!("workbench-extensions-install-not-utf8", name = name))
                })?
                .to_owned();
            self.write(&name, kind, &mut entry)?;
        }
        Ok(())
    }

    /// Copies the folder `source`/`relative`, recursively, never following
    /// a link in it.
    fn folder(&mut self, source: &Path, relative: &Path) -> Result<(), InstallError> {
        // Counted as they're listed, so a huge folder isn't listed whole.
        let mut children = Vec::new();
        for child in fs::read_dir(source.join(relative))? {
            children.push(child?);
            if self.entries + children.len() > MAX_ENTRIES {
                return Err(too_many());
            }
        }
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            let relative = relative.join(child.file_name());
            let name = relative.to_string_lossy().into_owned();
            let file_type = child.file_type()?;
            if file_type.is_dir() {
                self.entry(&name, Kind::Dir, &mut io::empty())?;
                self.folder(source, &relative)?;
            } else if file_type.is_file() {
                let metadata = child.metadata()?;
                if is_hard_linked(&metadata) {
                    self.entry(&name, Kind::HardLink, &mut io::empty())?;
                }
                let executable = is_executable(&metadata);
                let mut file = fs::File::open(child.path())?;
                self.entry(&name, Kind::File { executable }, &mut file)?;
            } else if file_type.is_symlink() {
                self.entry(&name, Kind::Symlink, &mut io::empty())?;
            } else {
                self.entry(&name, Kind::Special, &mut io::empty())?;
            }
        }
        Ok(())
    }

    /// Counts an entry, refusing one too many.
    fn count(&mut self) -> Result<(), InstallError> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES {
            return Err(too_many());
        }
        Ok(())
    }

    /// Counts the entry `name` and writes it ([`Unpack::write`]).
    fn entry(
        &mut self,
        name: &str,
        kind: Kind,
        contents: &mut dyn Read,
    ) -> Result<(), InstallError> {
        self.count()?;
        self.write(name, kind, contents)
    }

    /// Writes the entry `name`, `kind`, its contents read from `contents`,
    /// or refuses it: a link or special file whatever its name.
    fn write(
        &mut self,
        name: &str,
        kind: Kind,
        contents: &mut dyn Read,
    ) -> Result<(), InstallError> {
        let relative = entry_path(name)?;
        let executable = match kind {
            Kind::Symlink => {
                let reason = tr!("workbench-extensions-install-symlink", name = name);
                return Err(refused(reason));
            }
            Kind::HardLink => {
                let reason = tr!("workbench-extensions-install-hard-link", name = name);
                return Err(refused(reason));
            }
            Kind::Special => {
                let reason = tr!("workbench-extensions-install-special", name = name);
                return Err(refused(reason));
            }
            Kind::File { executable } => executable,
            Kind::Dir => false,
        };
        // The top level itself, or macOS's `__MACOSX` folder.
        if relative.as_os_str().is_empty() || relative.starts_with("__MACOSX") {
            return Ok(());
        }
        if let Kind::Dir = kind {
            fs::create_dir_all(self.root.join(relative))?;
            return Ok(());
        }
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&path)?;
        let room = MAX_UNPACKED - self.bytes;
        self.bytes += io::copy(&mut contents.take(room + 1), &mut file)?;
        if self.bytes > MAX_UNPACKED {
            return Err(too_big());
        }
        set_executable(&file, executable)?;
        Ok(())
    }
}

/// The relative path entry `name` unpacks to, `/` or `\` separated: empty
/// for one that names the top level itself. An absolute path and a `..`
/// are refused.
fn entry_path(name: &str) -> Result<PathBuf, InstallError> {
    let drive = name.as_bytes().get(1) == Some(&b':');
    if name.starts_with(['/', '\\']) || drive || Path::new(name).has_root() {
        return Err(absolute(name));
    }
    let mut path = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match Path::new(part).components().next() {
            None | Some(Component::CurDir) => {}
            Some(Component::ParentDir) => {
                let reason = tr!("workbench-extensions-install-outside", name = name);
                return Err(refused(reason));
            }
            Some(Component::Normal(_)) => path.push(part),
            Some(Component::RootDir | Component::Prefix(_)) => return Err(absolute(name)),
        }
    }
    Ok(path)
}

/// What a tar's pax header says of the next entry.
struct PaxRecords {
    path: Option<Vec<u8>>,
    size: Option<u64>,
}

/// The `path` and `size` the pax header `payload` gives the next entry.
fn pax_records(payload: &[u8]) -> Result<PaxRecords, InstallError> {
    let unreadable = || refused(tr!("workbench-extensions-install-pax-unreadable"));
    let mut records = PaxRecords {
        path: None,
        size: None,
    };
    for record in tar::PaxExtensions::new(payload) {
        let record = record.map_err(|_| unreadable())?;
        match record.key().map_err(|_| unreadable())? {
            "path" => records.path = Some(record.value_bytes().to_owned()),
            "size" => {
                let size = std::str::from_utf8(record.value_bytes()).ok();
                let size = size.and_then(|size| size.parse().ok());
                records.size = Some(size.ok_or_else(unreadable)?);
            }
            _ => {}
        }
    }
    Ok(records)
}

/// The extension's directory in what was unpacked to `unpacked`: itself, if
/// it has the manifest, else its single folder, if that has it.
fn extension_root(unpacked: &Path) -> Result<PathBuf, InstallError> {
    if unpacked.join(MANIFEST_FILE).is_file() {
        return Ok(unpacked.to_owned());
    }
    let mut children = fs::read_dir(unpacked)?.collect::<io::Result<Vec<_>>>()?;
    if children.len() == 1 && children[0].file_type()?.is_dir() {
        let dir = children.remove(0).path();
        if dir.join(MANIFEST_FILE).is_file() {
            return Ok(dir);
        }
    }
    Err(refused(tr!(
        "workbench-extensions-install-no-manifest",
        file = MANIFEST_FILE
    )))
}

/// Checks the manifest in `root`: what it installs as in `extensions_dir`,
/// or the errors refusing it.
pub(crate) fn check(
    root: &Path,
    extensions_dir: &Path,
    api: &ExtensionApi,
) -> Result<Installed, InstallError> {
    let (manifest, diagnostics) = load_manifest(root, api);
    let Some(manifest) = manifest else {
        let errors: Vec<String> = (diagnostics.iter())
            .filter(|diagnostic| diagnostic.is_error())
            .map(diagnostic_text)
            .collect();
        return Err(InstallError::Refused(errors.join("\n")));
    };
    let plugin = PluginManifest::from_extension(&manifest, ExtensionOrigin::User)
        .map_err(|error| InstallError::Refused(error.to_string()))?;
    let mut components = Path::new(&manifest.id).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        let reason = tr!(
            "workbench-extensions-install-bad-id",
            id = manifest.id.as_str()
        );
        return Err(refused(reason));
    }
    Ok(Installed {
        dir: extensions_dir.join(&manifest.id),
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        plugin: plugin.is_some(),
    })
}

/// `extension.toml:3:11: version: …`, without the staging directory.
fn diagnostic_text(diagnostic: &Diagnostic) -> String {
    let (line, column) = (diagnostic.line.to_string(), diagnostic.column.to_string());
    let message = diagnostic.message.as_str();
    match &diagnostic.key {
        Some(key) => tr!(
            "workbench-extensions-install-manifest-key-error",
            file = MANIFEST_FILE,
            line = line,
            column = column,
            key = key.as_str(),
            message = message
        ),
        None => tr!(
            "workbench-extensions-install-manifest-error",
            file = MANIFEST_FILE,
            line = line,
            column = column,
            message = message
        ),
    }
    .into()
}

/// The directories in `extensions_dir` whose manifest has `id`, whatever
/// their names, in the order the extensions service reads them: the first
/// is the one that loads. A staging directory has no manifest of its own.
pub(crate) fn installed_with(extensions_dir: &Path, id: &str) -> io::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(extensions_dir)? {
        let dir = entry?.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(text) = fs::read_to_string(dir.join(MANIFEST_FILE)) else {
            continue;
        };
        let named = (text.parse::<toml::Table>().ok())
            .and_then(|table| table.get("id")?.as_str().map(str::to_owned));
        if named.as_deref() == Some(id) {
            dirs.push(dir);
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Whether `a` and `b` are one file, links not followed: on a
/// case-insensitive filesystem, two spellings may be.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => a == b,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The `version` the manifest in `dir` names, else "unknown".
fn installed_version(dir: &Path) -> String {
    let text = fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap_or_default();
    (text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("version")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| unknown_version().into())
}

/// What an installed version unnamed by its manifest is shown as.
fn unknown_version() -> SharedString {
    tr!("workbench-extensions-install-unknown-version")
}

fn refused(reason: SharedString) -> InstallError {
    InstallError::Refused(reason.into())
}

fn absolute(name: &str) -> InstallError {
    refused(tr!("workbench-extensions-install-absolute", name = name))
}

fn too_big() -> InstallError {
    refused(tr!(
        "workbench-extensions-install-too-big",
        mib = (MAX_UNPACKED >> 20).to_string()
    ))
}

fn too_many() -> InstallError {
    refused(tr!(
        "workbench-extensions-install-too-many",
        max = MAX_ENTRIES.to_string()
    ))
}

fn zip_error(error: zip::result::ZipError) -> InstallError {
    match error {
        zip::result::ZipError::Io(error) => error.into(),
        error => InstallError::Io(
            tr!(
                "workbench-extensions-install-bad-zip",
                error = error.to_string()
            )
            .into(),
        ),
    }
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &fs::Metadata) -> bool {
    false
}

/// Whether the file has other names, linked to it.
#[cfg(unix)]
fn is_hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.nlink() > 1
}

#[cfg(not(unix))]
fn is_hard_linked(_: &fs::Metadata) -> bool {
    false
}

/// Makes `file` executable, if `executable`; what else it allows is the
/// umask's.
#[cfg(unix)]
fn set_executable(file: &fs::File, executable: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    if !executable {
        return Ok(());
    }
    let mut permissions = file.metadata()?.permissions();
    let mode = permissions.mode();
    // Executable by whoever may read it.
    permissions.set_mode(mode | ((mode & 0o444) >> 2));
    file.set_permissions(permissions)
}

#[cfg(not(unix))]
fn set_executable(_: &fs::File, _: bool) -> io::Result<()> {
    Ok(())
}

/// The version text people see: `1.2.0`.
pub(crate) fn version_text((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

/// The replace prompt's question and detail.
pub(crate) fn replace_prompt_text(staged: &Staged) -> (String, String) {
    let installed = staged.installed();
    let unknown = unknown_version();
    let old = staged.replaces().unwrap_or(&unknown);
    let new = version_text(installed.version);
    let question = tr!(
        "workbench-extensions-install-replace-question",
        name = installed.name.as_str(),
        old = old,
        new = new
    );
    let detail = tr!(
        "workbench-extensions-install-replace-detail",
        id = installed.id.as_str(),
        old = old,
        dir = (staged.replaced_dir().unwrap_or(&installed.dir).display()).to_string()
    );
    (question.into(), detail.into())
}

/// The success toast's message and detail.
pub(crate) fn installed_text(installed: &Installed, preview: bool) -> (String, Option<String>) {
    let message = tr!(
        "workbench-extensions-install-installed",
        name = installed.name.as_str(),
        version = version_text(installed.version)
    );
    let detail = installed.plugin.then(|| {
        if preview {
            tr!("workbench-extensions-install-plugin-asks").into()
        } else {
            tr!(
                "workbench-extensions-install-plugin-needs-preview",
                setting = PREVIEW_SETTING
            )
            .into()
        }
    });
    (message.into(), detail)
}

/// The extensions directory installs go to: the app's; under test, none
/// until the test sets one ([`set_extensions_dir`]). And whether one is
/// being installed.
#[derive(Default)]
struct Installs {
    dir: Option<Option<PathBuf>>,
    busy: bool,
}

impl Global for Installs {}

pub(crate) fn extensions_dir(cx: &mut App) -> Option<PathBuf> {
    let installs = cx.default_global::<Installs>();
    let dir = installs.dir.get_or_insert_with(|| {
        if cfg!(test) {
            None
        } else {
            fe_config::paths::app_paths().extensions_dir()
        }
    });
    dir.clone()
}

/// Has installs go to `dir`.
#[cfg(test)]
pub(crate) fn set_extensions_dir(cx: &mut App, dir: PathBuf) {
    cx.default_global::<Installs>().dir = Some(Some(dir));
}

/// The command namespaces an extension's id may not start with: the app's
/// commands' and the typed commands'.
pub(crate) fn reserved_namespaces(cx: &App) -> Vec<String> {
    let app = kiln_platform::commands::app_command_list(cx)
        .into_iter()
        .map(|meta| meta.id);
    let typed = kiln_platform::command_actions::typed_commands(cx);
    let mut namespaces: Vec<String> = app
        .chain(typed.into_iter().map(|typed| typed.id))
        .map(|id| id.namespace().to_owned())
        .collect();
    namespaces.sort();
    namespaces.dedup();
    namespaces
}

impl Workbench {
    /// Asks for a `.zip`, `.tar.gz`, `.tgz` or folder and installs the
    /// extension in it, off the main thread, asking first if its id is
    /// installed; the toast says how it went. Asked again while one
    /// installs, it does nothing.
    pub(crate) fn install_extension(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if cx.default_global::<Installs>().busy {
            return;
        }
        if extensions_dir(cx).is_none() {
            let (message, detail) = no_extensions_dir();
            return Workbench::notify(cx, log::Level::Error, message, Some(detail));
        }
        cx.default_global::<Installs>().busy = true;
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(tr!("workbench-extensions-install-button")),
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let source = match answer.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    log::warn!("the open panel failed: {error}");
                    None
                }
            };
            let outcome = match source {
                Some(source) => {
                    // Taken now, not before the dialog: it can stay open while
                    // a plugin registers a namespace.
                    let taken = AsyncApp::update(cx, |cx| {
                        let dir = extensions_dir(cx)?;
                        Some((dir, check_api(reserved_namespaces(cx))))
                    });
                    match taken {
                        Some((dir, api)) => {
                            let outcome = install_asking(&source, dir, api, workbench, cx).await;
                            outcome.map(|outcome| (source, outcome))
                        }
                        None => None,
                    }
                }
                None => None,
            };
            // The window may be gone by now; the toast goes to another.
            AsyncApp::update(cx, |cx| {
                cx.default_global::<Installs>().busy = false;
                match outcome {
                    None => {}
                    Some((_, Ok(installed))) => {
                        let preview = crate::plugin_host::preview_on(cx);
                        let (message, detail) = installed_text(&installed, preview);
                        Workbench::notify(cx, log::Level::Info, message, detail);
                    }
                    Some((source, Err(error))) => {
                        let message = cant_install(&source);
                        Workbench::notify(cx, log::Level::Error, message, Some(error.to_string()));
                    }
                }
            });
        })
        .detach();
    }
}

/// Stages `source`, asks in `workbench`'s window whether to replace the
/// extension installed with its id if there is one, and places it: how
/// that went, or `None` if the user cancelled.
async fn install_asking(
    source: &Path,
    extensions_dir: PathBuf,
    api: ExtensionApi,
    workbench: WeakEntity<Workbench>,
    cx: &mut AsyncWindowContext,
) -> Option<Result<Installed, InstallError>> {
    let source = source.to_owned();
    let staged = cx.background_spawn(async move { stage(&source, &extensions_dir, &api) });
    let staged = match staged.await {
        Ok(staged) => staged,
        Err(error) => return Some(Err(error)),
    };
    if staged.replaces().is_some() {
        let (question, detail) = replace_prompt_text(&staged);
        let answer = workbench.update_in(cx, |_, window, cx| {
            let buttons = replace_buttons();
            window.prompt(
                PromptLevel::Warning,
                &question,
                Some(&detail),
                &buttons.each_ref().map(|button| button.as_ref()),
                cx,
            )
        });
        let replace = match answer {
            Ok(answer) => answer.await == Ok(REPLACE),
            Err(_) => false,
        };
        if !replace {
            // What was unpacked is removed off the main thread too.
            cx.background_spawn(async move { drop(staged) }).await;
            return None;
        }
    }
    Some(cx.background_spawn(async move { staged.place() }).await)
}

/// What an install says when there's no extensions directory: its message
/// and detail.
pub(crate) fn no_extensions_dir() -> (String, String) {
    let message = tr!("workbench-extensions-install-cant-install");
    let detail = tr!("workbench-extensions-install-no-state-dir");
    (message.into(), detail.into())
}

/// What an install of `source` that failed says.
pub(crate) fn cant_install(source: &Path) -> String {
    let name = file_name(source);
    tr!(
        "workbench-extensions-install-cant-install-from",
        name = name
    )
    .into()
}

/// `path`'s file name, else all of it.
pub(crate) fn file_name(path: &Path) -> String {
    (path.file_name()).map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
#[path = "extension_install_tests.rs"]
mod extension_install_tests;
