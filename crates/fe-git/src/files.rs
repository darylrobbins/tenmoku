//! The files git knows of in the working tree: those it tracks and those
//! it would add, through the `git` executable (see [`crate::cli`]), for a
//! picker over them ("Go to Tracked File", LazyVim's `<leader>fg`).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

use gix::bstr::ByteSlice;

use crate::cli::{Access, run};
use crate::{GitError, Repo};

impl Repo {
    /// The files in the working tree git tracks, and the untracked ones it
    /// doesn't ignore (`.gitignore`, `.git/info/exclude` and the global
    /// excludes): `git ls-files --cached --others --exclude-standard`, as
    /// absolute paths, sorted, each once. Only files and symlinks on disk
    /// are kept: a tracked file deleted from the working tree, a submodule
    /// and an untracked nested repository are left out. Blocks until git
    /// exits; callers run it off the UI thread.
    pub fn listed_files(&self) -> Result<Vec<PathBuf>, GitError> {
        let args = [
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ]
        .map(OsString::from);
        let out = run(&self.workdir, Access::Read, &args, None)?;
        let mut files: Vec<PathBuf> = (out.split(|&byte| byte == 0))
            .filter(|name| !name.is_empty() && !name.ends_with(b"/"))
            .filter_map(|name| gix::path::from_bstr(name.as_bstr()).ok())
            .map(|name| self.workdir.join(name))
            .filter(|path| {
                // Not following a symlink: one is listed whatever it names.
                path.symlink_metadata()
                    .is_ok_and(|meta| meta.is_file() || meta.is_symlink())
            })
            .collect();
        files.sort();
        // An unmerged path has an index entry per stage.
        files.dedup();
        Ok(files)
    }

    /// When the index file was last written, `None` if there's none (a
    /// repository nothing was ever added to) or it can't be read: with
    /// [`Repo::head_id`], what says [`Repo::listed_files`]' tracked files
    /// may have changed.
    pub fn index_mtime(&self) -> Option<SystemTime> {
        let index = self.repo.to_thread_local().index_path();
        index.metadata().and_then(|meta| meta.modified()).ok()
    }
}
