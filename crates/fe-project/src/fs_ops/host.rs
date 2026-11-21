//! [`LocalTreeFs`]: this machine's files with the project tree's operations,
//! [`FsHost`]'s local impl. The tree's operations are this module's parent's
//! functions and [`crate::Listing`]'s read, called as they are; the rest is
//! fe-host's [`LocalFs`] (docs/decisions/5zajsn5anw-fs-host-tree.md).

use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use fe_host::{
    Created, DirEntry, DiskStamp, FsHost, HostSet, ListReply, ListRequest, LocalFs, Placed, Stat,
    TrashRecord, WriteError, WriteExpect,
};
use ignore::gitignore::Gitignore;

use super::Trashed;
use crate::listing::{self, Listing};

/// This machine's files, the project tree's operations included: what a
/// local window's [`HostSet`] holds ([`local_hosts`]).
#[derive(Debug, Default)]
pub struct LocalTreeFs {
    /// git's global excludes (`core.excludesFile`), for a listing handed
    /// none of its tree's: read on the first such listing.
    global: OnceLock<Arc<Gitignore>>,
}

impl LocalTreeFs {
    pub fn new() -> Self {
        Self::default()
    }
}

/// This machine's hosts, with [`LocalTreeFs`] for its files.
pub fn local_hosts() -> HostSet {
    HostSet {
        fs: Arc::new(LocalTreeFs::new()),
    }
}

impl FsHost for LocalTreeFs {
    fn read(&self, path: &Path) -> io::Result<(Vec<u8>, DiskStamp)> {
        LocalFs.read(path)
    }

    fn write_atomic(
        &self,
        path: &Path,
        bytes: &[u8],
        expect: WriteExpect,
    ) -> Result<DiskStamp, WriteError> {
        LocalFs.write_atomic(path, bytes, expect)
    }

    fn stat(&self, path: &Path) -> io::Result<Option<Stat>> {
        LocalFs.stat(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        LocalFs.list_dir(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        LocalFs.canonicalize(path)
    }

    fn list_tree(&self, request: &ListRequest) -> ListReply {
        if let Err(err) = request.check() {
            return ListReply {
                result: Err(err),
                id: None,
                lost: None,
            };
        }
        // Read once, only for a request without the tree's own.
        let global = match request.global_excludes.as_ref() {
            Some(global) => match global.get::<Gitignore>() {
                Some(global) => global,
                None => {
                    return ListReply {
                        result: Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            fe_host::RequestError::NotExcludes,
                        )),
                        id: None,
                        lost: None,
                    };
                }
            },
            None => self.global.get_or_init(listing::global_excludes).clone(),
        };
        Listing::of_request(request, global).read()
    }

    fn create_recorded(
        &self,
        root: &Path,
        dir: &Path,
        name: &str,
        contents: &[u8],
        folder: bool,
    ) -> io::Result<Created> {
        super::create_recorded_with(root, dir, name, contents, folder)
    }

    fn rename(&self, root: &Path, from: &Path, new_name: &str) -> io::Result<PathBuf> {
        super::rename(root, from, new_name)
    }

    fn duplicate_recorded(&self, root: &Path, path: &Path) -> io::Result<Placed> {
        super::duplicate_recorded(root, path)
    }

    fn copy_into_recorded(&self, root: &Path, path: &Path, dir: &Path) -> io::Result<Placed> {
        super::copy_into_recorded(root, path, dir)
    }

    fn paste_name(
        &self,
        root: &Path,
        path: &Path,
        dir: &Path,
        planned: &[OsString],
    ) -> io::Result<OsString> {
        super::paste_name_besides(root, path, dir, planned)
    }

    fn move_into(&self, root: &Path, from: &Path, dir: &Path, name: &OsStr) -> io::Result<PathBuf> {
        super::move_into(root, from, dir, name)
    }

    fn trash_recorded(&self, root: &Path, paths: &[PathBuf]) -> io::Result<Vec<TrashRecord>> {
        super::trash_recorded(root, paths)
            .map(|trashed| trashed.into_iter().map(Into::into).collect())
    }

    fn restores_from_trash(&self) -> bool {
        super::restores_from_trash()
    }

    fn restore(&self, root: &Path, trashed: &mut TrashRecord) -> io::Result<PathBuf> {
        let mut local = Trashed::from(trashed.clone());
        let restored = super::restore(root, &mut local);
        // The record follows the entry, as `restore` has it follow, on
        // failure too.
        *trashed = local.into();
        restored
    }

    fn in_trash(&self, trashed: &TrashRecord) -> io::Result<bool> {
        Trashed::from(trashed.clone()).in_trash()
    }

    fn exists(&self, root: &Path, path: &Path) -> io::Result<bool> {
        super::exists(root, path)
    }

    fn truncate(&self, root: &Path, path: &Path) -> io::Result<()> {
        super::open_truncated(root, path).map(drop)
    }
}
