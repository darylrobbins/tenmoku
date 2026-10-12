//! Listing one directory with gitignore rules, and the no-follow file system
//! calls the tree reconciles with.
//!
//! Everything is read through directory handles opened a component at a time
//! from `/` without following symlinks: a directory's entries, its identity
//! and the ignore files of it and every directory above it. A path swapped
//! for a symlink (in the project or above its root) therefore never leads a
//! listing or [`crate::FileTree::apply`] outside the project, whenever the
//! swap happens.

use std::{
    cmp::Ordering,
    collections::HashMap,
    ffi::{CStr, CString, OsStr, OsString, c_int},
    fs::File,
    io::{self, BufRead, BufReader, Read},
    os::{
        fd::{FromRawFd, IntoRawFd, OwnedFd, RawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt, io::AsRawFd},
    },
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering as AtomicOrdering},
    },
};

use ignore::{
    DirEntry, Match, WalkBuilder,
    gitignore::{Gitignore, GitignoreBuilder},
};

/// What a tree entry is. Symlinks are never followed, so a link to a directory
/// is a [`EntryKind::Symlink`] and can't be expanded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

/// A directory's device and inode, as `stat` reports them. Tells a directory
/// from another one later put at its path. (A `readdir` entry's inode can
/// differ: at a mount point or macOS firmlink it is the covered directory's.)
pub(crate) type Identity = (u64, u64);

/// One entry of a listed directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: OsString,
    pub kind: EntryKind,
    /// Matched by an ignore file. Only ever set when ignored entries are shown.
    pub ignored: bool,
    /// Directories only.
    pub(crate) id: Option<Identity>,
}

/// The listing of one directory, split from the tree so it can run on another
/// thread: get it with [`crate::FileTree::listing`], call [`Listing::run`]
/// anywhere, then hand the result to [`crate::FileTree::fill`].
#[derive(Clone, Debug)]
pub struct Listing {
    pub(crate) root: PathBuf,
    /// The directory, relative to `root`.
    pub(crate) rel: PathBuf,
    pub(crate) show_ignored: bool,
    /// Everything under an ignored directory is ignored, whatever its own name.
    pub(crate) parent_ignored: bool,
    /// The identities the tree has for the root and each directory down to
    /// this one, `None` where it doesn't know. (And for the root, when
    /// listing it: a root replaced by another directory is adopted.)
    pub(crate) ids: Vec<Option<Identity>>,
    /// The tree's clock, which orders listings against `apply` calls.
    pub(crate) clock: Arc<AtomicU64>,
    /// git's global excludes (`core.excludesFile`).
    pub(crate) global: Arc<Gitignore>,
}

/// The result of [`Listing::run`], for [`crate::FileTree::fill`].
#[derive(Debug)]
pub struct Listed {
    pub(crate) result: io::Result<Vec<Entry>>,
    /// The clock when the run started.
    pub(crate) seq: u64,
    /// The clock when it ended.
    pub(crate) done: u64,
    /// The directory's identity, on success.
    pub(crate) id: Option<Identity>,
    /// Set if the path no longer led to the directory the listing was for.
    pub(crate) lost: Option<Lost>,
    /// The ignore flag the run inherited from the directory, as the tree had
    /// it when the listing was made: stale if the tree's flag has changed.
    pub(crate) parent_ignored: bool,
}

/// Why a path no longer leads to the directory a listing was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lost {
    /// Its own name holds nothing, another directory, or a symlink.
    Itself,
    /// A directory above it, the root included, is missing, another
    /// directory, or a symlink, perhaps only for a moment.
    Above,
    /// The root (for a listing of the root itself, a directory above it) is
    /// missing, another directory, or a symlink, perhaps only for a moment.
    /// No events come for directories above the root.
    Root,
}

impl Listed {
    /// The entries in display order, or why the directory couldn't be read.
    pub fn result(&self) -> Result<&[Entry], &io::Error> {
        self.result.as_deref()
    }
}

impl Listing {
    /// The directory this lists.
    pub fn dir(&self) -> PathBuf {
        self.root.join(&self.rel)
    }

    /// Reads the directory.
    ///
    /// Its path is opened a component at a time from `/` without following
    /// symlinks, the root and each directory below it checked to be the one
    /// the tree has, and the directory read through its handle, with the
    /// ignore files of it and of every directory above it read through
    /// theirs. Afterwards the path is opened and checked again. If it no
    /// longer leads to the directory (it, or one above it, was removed,
    /// replaced, or replaced by a symlink), the result says which.
    pub fn run(&self) -> Listed {
        let seq = tick(&self.clock);
        let mut lost = None;
        let result = self.list(&mut lost);
        let done = tick(&self.clock);
        match result {
            Ok((entries, id)) => Listed {
                result: Ok(entries),
                seq,
                done,
                id: Some(id),
                lost: None,
                parent_ignored: self.parent_ignored,
            },
            Err(err) => Listed {
                result: Err(err),
                seq,
                done,
                id: None,
                lost,
                parent_ignored: self.parent_ignored,
            },
        }
    }

    fn list(&self, lost: &mut Option<Lost>) -> io::Result<(Vec<Entry>, Identity)> {
        let path = self.dir();
        let dirs = self.reach(Access::Read, lost)?;
        let dir = dirs.last().expect("`/` is always opened");
        let id = identity(dir)?;
        checkpoint(Point::Opened);
        let names = read_dir(dir)?;
        // Everything below an ignored directory is ignored anyway.
        let rules = if self.parent_ignored {
            None
        } else {
            Rules::chain(&dirs, &path)
        };
        let mut entries = Vec::with_capacity(names.len());
        for (name, kind) in names {
            if is_git_dir(&name) {
                continue;
            }
            let kind = match kind {
                Some(kind) => kind,
                // The file system didn't say; an entry gone since is skipped.
                None => match stat_at(dir, &name) {
                    Ok(stat) => stat.kind,
                    Err(_) => continue,
                },
            };
            let is_dir = kind == EntryKind::Dir;
            let ignored = self.parent_ignored
                || rules
                    .as_ref()
                    .is_some_and(|rules| rules.ignores(&self.global, &path.join(&name), is_dir));
            if ignored && !self.show_ignored {
                continue;
            }
            let id = if is_dir {
                stat_at(dir, &name)
                    .ok()
                    .filter(|stat| stat.kind == EntryKind::Dir)
                    .map(|stat| stat.id)
            } else {
                None
            };
            entries.push(Entry {
                name,
                kind,
                ignored,
                id,
            });
        }
        checkpoint(Point::Read);
        let after = self.reach(Access::Search, lost)?;
        if identity(after.last().expect("`/` is always opened"))? != id {
            return Err(self.lost(lost, Lost::Itself));
        }
        entries.sort_by(|a, b| compare(a.kind, &a.name, b.kind, &b.name));
        Ok((entries, id))
    }

    /// Opens the directory's path from `/` (see [`open_dirs`]), checking the
    /// root and each directory below it against the identity the tree has
    /// for it before going on. Sets `lost` if the path doesn't lead to the
    /// directory.
    fn reach(&self, access: Access, lost: &mut Option<Lost>) -> io::Result<Vec<File>> {
        let names: Vec<&OsStr> = self.rel.iter().collect();
        let why = |ix: usize| {
            if ix == names.len() {
                Lost::Itself
            } else if ix == 0 {
                Lost::Root
            } else {
                Lost::Above
            }
        };
        let root_access = if names.is_empty() {
            access
        } else {
            Access::Search
        };
        let mut dirs = open_dirs(&self.root, root_access).map_err(|unreached| {
            if !is_absent(&unreached.err) {
                unreached.err
            } else if unreached.itself {
                // The root itself, which is the directory only when listing it.
                self.lost(lost, why(0))
            } else {
                self.lost(lost, Lost::Root)
            }
        })?;
        for (ix, known) in self.ids.iter().enumerate() {
            if ix > 0 {
                let access = if ix == names.len() {
                    access
                } else {
                    Access::Search
                };
                let dir = open_at(
                    dirs.last().expect("the root is open"),
                    names[ix - 1],
                    access,
                )
                .map_err(|err| {
                    if is_absent(&err) {
                        self.lost(lost, why(ix))
                    } else {
                        err
                    }
                })?;
                dirs.push(dir);
            }
            if let Some(known) = *known
                && identity(dirs.last().expect("the root is open"))? != known
            {
                return Err(self.lost(lost, why(ix)));
            }
        }
        Ok(dirs)
    }

    /// Records why the path doesn't lead to the directory.
    fn lost(&self, lost: &mut Option<Lost>, why: Lost) -> io::Error {
        *lost = Some(why);
        let what = match why {
            Lost::Itself => "was replaced",
            Lost::Above | Lost::Root => "couldn't be reached",
        };
        io::Error::other(format!("{} {what} while listing", self.dir().display()))
    }
}

/// Advances `clock` and returns the new time.
pub(crate) fn tick(clock: &AtomicU64) -> u64 {
    clock.fetch_add(1, AtomicOrdering::SeqCst) + 1
}

/// The ignore rules in force in one directory: its `.ignore`, `.gitignore`
/// and git's `info/exclude`, then those of each directory above it up to `/`.
///
/// For a path, the nearest `.ignore` rule that matches decides, else the
/// nearest `.gitignore` rule, else the nearest `info/exclude` rule, else
/// git's global excludes. `.gitignore` and `info/exclude` rules are taken
/// from the nearest repository (a directory holding `.git` or `.jj`) and
/// those below it only, as git takes them; `.ignore` rules from every
/// directory. Outside any repository `.gitignore` files apply too.
///
/// This is the `ignore` crate's `WalkBuilder` with `parents(true)` and
/// `hidden(false)`, with `require_git(true)` inside a repository and
/// `require_git(false)` outside any, but for a `.git` file without a
/// `commondir` (a submodule's), which the walker doesn't follow. It is built
/// from files read through directory handles. Ignore files are opened without following
/// symlinks (as git, since 2.32, opens in-tree `.gitignore` files). A linked
/// worktree's or submodule's `.git` file is followed to its repository's
/// `info/exclude`, a directory at a time without following symlinks either;
/// a `.git` symlink isn't, though it marks a repository.
pub(crate) struct Rules {
    ignore: Gitignore,
    gitignore: Gitignore,
    exclude: Gitignore,
    /// Whether the directory is a repository's top: the rules of those above
    /// it, but for `.ignore`, don't apply in it.
    repository: bool,
    parent: Option<Arc<Rules>>,
}

impl Rules {
    /// Reads the ignore files of directory `dir`, open at `path`, through its
    /// handle, on top of `parent`'s rules.
    pub(crate) fn load(parent: Option<Arc<Rules>>, dir: &File, path: &Path) -> Arc<Rules> {
        let exclude = common_git_dir(dir)
            .and_then(|git| open_at(&git, OsStr::new("info"), Access::Search).ok())
            .and_then(|info| open_file(&info, "exclude"));
        let repository = [".git", ".jj"]
            .into_iter()
            .any(|name| stat_at(dir, OsStr::new(name)).is_ok());
        Arc::new(Rules {
            ignore: parse(path, open_file(dir, ".ignore")),
            gitignore: parse(path, open_file(dir, ".gitignore")),
            exclude: parse(path, exclude),
            repository,
            parent,
        })
    }

    /// The rules of the last of `dirs`, the handles [`open_dirs`] opened for
    /// `path`, or `None` if there are none.
    pub(crate) fn chain(dirs: &[File], path: &Path) -> Option<Arc<Rules>> {
        let paths: Vec<&Path> = path.ancestors().collect();
        dirs.iter()
            .zip(paths.into_iter().rev())
            .fold(None, |parent, (dir, path)| {
                Some(Rules::load(parent, dir, path))
            })
    }

    /// Whether `path`, an entry of the directory (absolute), is ignored.
    pub(crate) fn ignores(&self, global: &Gitignore, path: &Path, is_dir: bool) -> bool {
        let (mut ignore, mut gitignore, mut exclude) = (Match::None, Match::None, Match::None);
        // Past the nearest repository's top, only `.ignore` rules apply.
        let mut in_repository = true;
        let mut at = Some(self);
        while let Some(rules) = at {
            if ignore.is_none() {
                ignore = rules.ignore.matched(path, is_dir);
            }
            if in_repository && gitignore.is_none() {
                gitignore = rules.gitignore.matched(path, is_dir);
            }
            if in_repository && exclude.is_none() {
                exclude = rules.exclude.matched(path, is_dir);
            }
            in_repository &= !rules.repository;
            at = rules.parent.as_deref();
        }
        ignore
            .or(gitignore)
            .or(exclude)
            .or(global.matched(path, is_dir))
            .is_ignore()
    }
}

/// The `ignore` crate's walker over `dir`, filtered by [`Rules`] as listings
/// are, for [`crate::search`]'s walk. ([`crate::FinderIndex`] walks through
/// directory handles instead.) Its own filters are off: no configuration of them
/// stops at a repository nested below a directory that isn't in one, nor
/// follows a submodule's `.git` file. `.git` is never entered. Symlinks
/// aren't followed.
pub(crate) fn walk_builder(
    dir: &Path,
    keep: impl Fn(&DirEntry) -> bool + Send + Sync + 'static,
) -> WalkBuilder {
    // Rules are read and matched by canonical paths, as the tree's are,
    // whatever spelling of `dir` the walk reports entries under.
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let filter = WalkFilter {
        walked: dir.to_path_buf(),
        canonical,
        global: global_excludes(),
        rules: Mutex::new(HashMap::new()),
    };
    let mut builder = WalkBuilder::new(dir);
    builder
        .standard_filters(false)
        .filter_entry(move |entry| keep(entry) && filter.keeps(entry));
    builder
}

/// [`Rules`] for a walk, read a directory at a time as it enters them.
struct WalkFilter {
    /// The directory walked, as given (perhaps relative).
    walked: PathBuf,
    /// Its canonical path.
    canonical: PathBuf,
    global: Arc<Gitignore>,
    /// The rules in force in each directory entered, by path.
    rules: Mutex<HashMap<PathBuf, Arc<Rules>>>,
}

impl WalkFilter {
    fn keeps(&self, entry: &DirEntry) -> bool {
        if is_git_dir(entry.file_name()) {
            return false;
        }
        if entry.depth() == 0 {
            return true;
        }
        let rel = entry
            .path()
            .strip_prefix(&self.walked)
            .unwrap_or(entry.path());
        let path = self.canonical.join(rel);
        let Some(dir) = path.parent() else {
            return true;
        };
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        // A directory that can't be opened now (gone since it was read, say)
        // has nothing to keep.
        self.rules(dir)
            .is_some_and(|rules| !rules.ignores(&self.global, &path, is_dir))
    }

    /// The rules in force in `dir`, read through handles opened from `/`
    /// without following symlinks, on top of its parent's if the walk has
    /// them.
    fn rules(&self, dir: &Path) -> Option<Arc<Rules>> {
        let lock = || self.rules.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(rules) = lock().get(dir) {
            return Some(rules.clone());
        }
        let dirs = open_dirs(dir, Access::Search).ok()?;
        let parent = dir.parent().and_then(|parent| lock().get(parent).cloned());
        let rules = match parent {
            Some(parent) => Rules::load(Some(parent), dirs.last()?, dir),
            None => Rules::chain(&dirs, dir)?,
        };
        lock().insert(dir.to_path_buf(), rules.clone());
        Some(rules)
    }
}

/// git's global excludes (`core.excludesFile`), as the `ignore` crate reads
/// them, for checking paths with [`Rules::ignores`]. None in unit tests, so
/// they don't depend on this machine's git configuration.
pub(crate) fn global_excludes() -> Arc<Gitignore> {
    Arc::new(if cfg!(test) {
        Gitignore::empty()
    } else {
        Gitignore::global().0
    })
}

/// The rules in an ignore file, which apply below `dir`. Read as
/// `GitignoreBuilder::add` reads a file: a leading byte order mark is
/// skipped, reading stops at a line that isn't UTF-8, and invalid globs are
/// skipped.
fn parse(dir: &Path, file: Option<File>) -> Gitignore {
    let Some(file) = file else {
        return Gitignore::empty();
    };
    let mut builder = GitignoreBuilder::new(dir);
    for (ix, line) in BufReader::new(file).lines().enumerate() {
        let Ok(line) = line else {
            break;
        };
        let line = if ix == 0 {
            line.trim_start_matches('\u{feff}')
        } else {
            &line
        };
        let _ = builder.add_line(None, line);
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// The repository directory whose `info/exclude` applies in `dir`, if `dir`
/// holds `.git`: that directory, or for a `.git` file (a linked worktree's or
/// a submodule's), the directory it names, or the common directory that one
/// names in its `commondir` file, as git resolves them. Each is opened a
/// directory at a time without following symlinks.
fn common_git_dir(dir: &File) -> Option<File> {
    if let Ok(git) = open_at(dir, OsStr::new(".git"), Access::Search) {
        return Some(git);
    }
    let git_dir = first_line(open_file(dir, ".git")?)?;
    let git_dir = open_path(dir, Path::new(git_dir.strip_prefix("gitdir: ")?)).ok()?;
    match open_file(&git_dir, "commondir").and_then(first_line) {
        Some(common) => open_path(&git_dir, Path::new(&common)).ok(),
        None => Some(git_dir),
    }
}

/// The first line of `file`, without trailing whitespace, if it is UTF-8.
fn first_line(file: File) -> Option<String> {
    let line = BufReader::new(file.take(4096)).lines().next()?.ok()?;
    Some(line.trim_end().to_owned())
}

/// Opens directory `path`, absolute (from `/`) or relative to `dir`, a
/// component at a time (`..` included) without following symlinks.
fn open_path(dir: &File, path: &Path) -> io::Result<File> {
    let mut at = if path.is_absolute() {
        open_raw(libc::AT_FDCWD, OsStr::new("/"), dir_flags(Access::Search))?
    } else {
        dir.try_clone()?
    };
    for component in path.components() {
        let name = match component {
            Component::Normal(name) => name,
            Component::ParentDir => OsStr::new(".."),
            Component::CurDir | Component::RootDir => continue,
            Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a plain path",
                ));
            }
        };
        at = open_at(&at, name, Access::Search)?;
    }
    Ok(at)
}

/// Opens regular file `name` in `dir` for reading, not through a symlink.
/// `None` if there is none (or it is something else, like a FIFO, which the
/// non-blocking open doesn't wait on).
fn open_file(dir: &File, name: &str) -> Option<File> {
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    let file = open_raw(dir.as_raw_fd(), OsStr::new(name), flags).ok()?;
    file.metadata()
        .is_ok_and(|metadata| metadata.is_file())
        .then_some(file)
}

/// The names in an open directory, with their kinds where the file system
/// reports them, read through the handle.
pub(crate) fn read_dir(dir: &File) -> io::Result<Vec<(OsString, Option<EntryKind>)>> {
    /// Closes the stream when dropped.
    struct Stream(*mut libc::DIR);
    impl Drop for Stream {
        fn drop(&mut self) {
            // SAFETY: the stream is open, and nothing uses it after this.
            unsafe { libc::closedir(self.0) };
        }
    }

    // The stream owns its descriptor, so it gets a copy.
    let fd = dir.try_clone()?.into_raw_fd();
    // SAFETY: `fd` is an open descriptor that nothing else owns.
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        let err = io::Error::last_os_error();
        // SAFETY: `fdopendir` failed, so `fd` is still ours.
        drop(unsafe { OwnedFd::from_raw_fd(fd) });
        return Err(err);
    }
    let stream = Stream(stream);
    let mut entries = Vec::new();
    loop {
        // `readdir` returns null at the end and on an error, which only
        // `errno` tells apart.
        // SAFETY: `errno` is this thread's.
        unsafe { *errno() = 0 };
        // SAFETY: the stream is open and only read here.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let err = io::Error::last_os_error();
            return match err.raw_os_error() {
                Some(0) => Ok(entries),
                _ => Err(err),
            };
        }
        // SAFETY: `readdir` returned an entry, valid until the next call,
        // whose name is NUL-terminated.
        let (name, d_type) = unsafe {
            (
                CStr::from_ptr((&raw const (*entry).d_name).cast()),
                (*entry).d_type,
            )
        };
        let name = name.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        let kind = match d_type {
            libc::DT_DIR => Some(EntryKind::Dir),
            libc::DT_LNK => Some(EntryKind::Symlink),
            libc::DT_UNKNOWN => None,
            _ => Some(EntryKind::File),
        };
        entries.push((OsStr::from_bytes(name).to_os_string(), kind));
    }
}

#[cfg(target_vendor = "apple")]
fn errno() -> *mut c_int {
    // SAFETY: always safe to call.
    unsafe { libc::__error() }
}

#[cfg(not(target_vendor = "apple"))]
fn errno() -> *mut c_int {
    // SAFETY: always safe to call.
    unsafe { libc::__errno_location() }
}

/// Opens the file at `rel` under directory `root` for reading, never
/// following a symlink, so a path listed in the project can't be led out of
/// it by a swap made after it was listed.
///
/// `root` is opened a component at a time from `/` (relative to the current
/// directory, if it is relative), and each component of `rel` through its
/// parent's handle, all without following a symlink: canonicalize a root
/// reached through one first. A symlink at a component of `rel`, or a
/// file where a directory should be, fails with [`io::ErrorKind::NotFound`],
/// as if nothing were there. A root that can't be reached so fails with the
/// error its open gave: [`io::ErrorKind::NotADirectory`] for a symlink, or
/// [`io::ErrorKind::InvalidInput`] for a `..` in it. `rel` must be
/// relative and name something below `root`: an absolute path, or one with a
/// `..` component, fails with [`io::ErrorKind::InvalidInput`].
///
/// The file is opened non-blocking, so a FIFO doesn't wait for a writer. It
/// may be anything but a symlink, such as a directory or a FIFO: check its
/// metadata before reading.
pub fn open_file_under(root: &Path, rel: &Path) -> io::Result<File> {
    let invalid = |what| io::Error::new(io::ErrorKind::InvalidInput, what);
    let mut names = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(name) => names.push(name),
            Component::CurDir => {}
            Component::ParentDir => return Err(invalid("`..` in a path under a root")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(invalid("an absolute path under a root"));
            }
        }
    }
    let Some((last, above)) = names.split_last() else {
        return Err(invalid("no file named under a root"));
    };
    let root = std::path::absolute(root)?;
    let mut dir = match open_dirs(&root, Access::Search) {
        Ok(mut dirs) => dirs.pop().expect("`/` is always opened"),
        Err(unreached) => return Err(unreached.err),
    };
    for name in above {
        dir = open_at(&dir, name, Access::Search).map_err(|err| {
            // A symlink opened as a directory without following it fails as
            // not a directory, like a file does.
            if err.raw_os_error() == Some(libc::ENOTDIR) {
                not_found("a symlink or a file on the way")
            } else {
                err
            }
        })?;
    }
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    open_raw(dir.as_raw_fd(), last, flags).map_err(|err| {
        // `O_NOFOLLOW` fails on a symlink with `ELOOP`.
        if err.raw_os_error() == Some(libc::ELOOP) {
            not_found("a symlink")
        } else {
            err
        }
    })
}

fn not_found(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, what)
}

pub(crate) fn is_git_dir(name: &OsStr) -> bool {
    name == ".git"
}

/// How a directory is opened: to read its entries, or only to reach and
/// look up what is in it. On macOS that is `O_SEARCH`, which needs search
/// permission alone; elsewhere it is a read-only open too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    Read,
    Search,
}

fn dir_flags(access: Access) -> c_int {
    #[cfg(target_os = "macos")]
    const SEARCH: c_int = libc::O_SEARCH;
    #[cfg(not(target_os = "macos"))]
    const SEARCH: c_int = libc::O_RDONLY;
    let mode = match access {
        Access::Read => libc::O_RDONLY,
        Access::Search => SEARCH,
    };
    mode | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
}

/// Opens `name` in directory `dir` with `flags`.
fn open_raw(dir: RawFd, name: &OsStr, flags: c_int) -> io::Result<File> {
    let c_name = CString::new(name.as_bytes())?;
    // SAFETY: `dir` is an open descriptor (or `AT_FDCWD`) and `c_name` is
    // NUL-terminated.
    let fd = unsafe { libc::openat(dir, c_name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `openat` returned a new descriptor that nothing else owns.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

/// Opens directory `name` in `dir` without following a symlink.
pub(crate) fn open_at(dir: &File, name: &OsStr, access: Access) -> io::Result<File> {
    open_raw(dir.as_raw_fd(), name, dir_flags(access))
}

/// Why [`open_dirs`] failed.
#[derive(Debug)]
pub(crate) struct Unreached {
    pub(crate) err: io::Error,
    /// Whether it was the path's last directory that couldn't be opened,
    /// rather than one above it.
    pub(crate) itself: bool,
}

/// Opens `/` and then each directory of absolute `path` in the one before,
/// never following a symlink, and returns them all, `path`'s last. It is
/// opened with `last` access, the others for search.
pub(crate) fn open_dirs(path: &Path, last: Access) -> Result<Vec<File>, Unreached> {
    let invalid = |what| Unreached {
        err: io::Error::new(io::ErrorKind::InvalidInput, what),
        itself: true,
    };
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return Err(invalid("not an absolute path"));
    }
    let names = components
        .map(|component| match component {
            Component::Normal(name) => Ok(name),
            _ => Err(invalid("not a plain path")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let depth = names.len();
    let access = |ix: usize| if ix == depth { last } else { Access::Search };
    let unreached = |ix: usize| {
        move |err| Unreached {
            err,
            itself: ix == depth,
        }
    };
    let slash = open_raw(libc::AT_FDCWD, OsStr::new("/"), dir_flags(access(0)));
    let mut dirs = vec![slash.map_err(unreached(0))?];
    for (ix, name) in names.iter().enumerate() {
        let dir = open_at(&dirs[ix], name, access(ix + 1)).map_err(unreached(ix + 1))?;
        dirs.push(dir);
    }
    Ok(dirs)
}

/// Whether names on the volume of open directory `dir` match regardless of
/// case, as on macOS by default (and, not knowing, assumed there).
#[cfg(target_os = "macos")]
pub(crate) fn ignores_case(dir: &File) -> bool {
    // SAFETY: `dir` is an open descriptor.
    unsafe { libc::fpathconf(dir.as_raw_fd(), libc::_PC_CASE_SENSITIVE) != 1 }
}

/// Whether names on the volume of open directory `dir` match regardless of
/// case. Assumed not, off macOS.
#[cfg(not(target_os = "macos"))]
pub(crate) fn ignores_case(_: &File) -> bool {
    false
}

/// The identity of an open file.
pub(crate) fn identity(file: &File) -> io::Result<Identity> {
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

/// What [`stat_at`] found.
pub(crate) struct Stat {
    pub(crate) kind: EntryKind,
    pub(crate) id: Identity,
    /// Whether the entry is spelled exactly `name` on disk. On a
    /// case-insensitive file system a lookup of `Foo` also finds `foo`.
    pub(crate) exact: bool,
}

/// The entry `name` in `dir`, without following a symlink.
#[cfg(target_os = "macos")]
pub(crate) fn stat_at(dir: &File, name: &OsStr) -> io::Result<Stat> {
    // `enum vtype` in <sys/vnode.h>.
    const VDIR: u32 = 2;
    const VLNK: u32 = 5;
    #[repr(C, align(8))]
    struct Buffer([u8; 1024]);

    let c_name = CString::new(name.as_bytes())?;
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_NAME
            | libc::ATTR_CMN_DEVID
            | libc::ATTR_CMN_OBJTYPE
            | libc::ATTR_CMN_FILEID,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buffer = Buffer([0; 1024]);
    // SAFETY: `dir` is an open descriptor, `c_name` is NUL-terminated, and
    // `attributes` and `buffer` are valid for the sizes given.
    let result = unsafe {
        libc::getattrlistat(
            dir.as_raw_fd(),
            c_name.as_ptr(),
            (&raw mut attributes).cast(),
            (&raw mut buffer).cast(),
            size_of::<Buffer>(),
            libc::c_ulong::from(libc::FSOPT_NOFOLLOW),
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // The attributes follow the buffer's length in bit order: the name's
    // `attrreference_t` (an offset from itself and a length with the NUL),
    // the device (a `dev_t`), the object type, then the file id.
    let bytes = &buffer.0;
    let word = |at: usize| -> [u8; 4] { bytes[at..at + 4].try_into().unwrap() };
    let name_offset = i32::from_ne_bytes(word(4));
    let name_len = u32::from_ne_bytes(word(8)) as usize;
    // Widened as `MetadataExt::dev` widens `st_dev`, so the two agree.
    let dev = i32::from_ne_bytes(word(12)) as u64;
    let kind = match u32::from_ne_bytes(word(16)) {
        VDIR => EntryKind::Dir,
        VLNK => EntryKind::Symlink,
        _ => EntryKind::File,
    };
    let ino = u64::from_ne_bytes(bytes[20..28].try_into().unwrap());
    let real = usize::try_from(4 + name_offset)
        .ok()
        .and_then(|start| bytes.get(start..start + name_len.saturating_sub(1)))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad getattrlist name"))?;
    Ok(Stat {
        kind,
        id: (dev, ino),
        exact: real == name.as_bytes(),
    })
}

/// The entry `name` in `dir`, without following a symlink.
#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_cast)]
pub(crate) fn stat_at(dir: &File, name: &OsStr) -> io::Result<Stat> {
    let c_name = CString::new(name.as_bytes())?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `dir` is an open descriptor, `c_name` is NUL-terminated and
    // `stat` is valid for writes of a `libc::stat`.
    let result = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            c_name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fstatat` succeeded, so it filled `stat`.
    let stat = unsafe { stat.assume_init() };
    let kind = match stat.st_mode & libc::S_IFMT {
        libc::S_IFDIR => EntryKind::Dir,
        libc::S_IFLNK => EntryKind::Symlink,
        _ => EntryKind::File,
    };
    Ok(Stat {
        kind,
        id: (stat.st_dev as u64, stat.st_ino as u64),
        exact: true,
    })
}

/// Whether an error means the path doesn't lead to a directory (any more):
/// it is missing, a file is in the way, or a symlink is.
pub(crate) fn is_absent(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    ) || err.raw_os_error() == Some(libc::ELOOP)
}

/// Display order: directories first, then by name ignoring case, with ties
/// broken by byte order so distinct names never compare equal.
pub(crate) fn compare(a_kind: EntryKind, a: &OsStr, b_kind: EntryKind, b: &OsStr) -> Ordering {
    let a_dir = a_kind == EntryKind::Dir;
    let b_dir = b_kind == EntryKind::Dir;
    b_dir.cmp(&a_dir).then_with(|| compare_names(a, b))
}

fn compare_names(a: &OsStr, b: &OsStr) -> Ordering {
    let folded = match (a.to_str(), b.to_str()) {
        (Some(a), Some(b)) => fold(a).cmp(fold(b)),
        _ => {
            let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
            fold(&a).cmp(fold(&b))
        }
    };
    folded.then_with(|| a.as_encoded_bytes().cmp(b.as_encoded_bytes()))
}

fn fold(name: &str) -> impl Iterator<Item = char> + '_ {
    name.chars().flat_map(char::to_lowercase)
}

/// Whether two distinct names differ only in case, which on a case-insensitive
/// file system can be the same file.
pub(crate) fn same_folded(a: &OsStr, b: &OsStr) -> bool {
    a != b && fold(&a.to_string_lossy()).eq(fold(&b.to_string_lossy()))
}

/// Where tests can change the disk in the middle of an operation, to check
/// what a race there can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Point {
    /// A listing opened and checked its directory and is about to read it.
    Opened,
    /// A listing read its directory and is about to check the path again.
    Read,
    /// `apply` looked an entry up in its opened directory and is about to
    /// classify it.
    Stat,
    /// `apply` couldn't reach a listed directory and is about to look its
    /// name up in its parent.
    Unreached,
}

#[cfg(test)]
pub(crate) type Hook = Box<dyn FnMut(Point)>;

#[cfg(test)]
thread_local! {
    /// Called at each [`Point`] reached on this thread.
    pub(crate) static HOOK: std::cell::RefCell<Option<Hook>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn checkpoint(point: Point) {
    // Out of its cell while it runs, so it can list (and the points it
    // reaches doing so don't call it again).
    #[cfg(test)]
    if let Some(mut hook) = HOOK.take() {
        hook(point);
        HOOK.with_borrow_mut(|slot| {
            slot.get_or_insert(hook);
        });
    }
    #[cfg(not(test))]
    let _ = point;
}
