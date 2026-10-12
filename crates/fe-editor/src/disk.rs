//! Saving to and reloading from disk: a save runs as a [`SaveJob`] that can
//! write on another thread and refuses a file that changed since it was
//! read.

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

use fe_buffer::{DecodedFile, DiskStamp, Origin, SaveSnapshot, TransactionId};

use crate::EditorModel;

/// Why a [`SaveJob`] didn't save.
#[derive(Debug)]
pub enum SaveError {
    /// The file isn't as it was when last read or saved: something else
    /// wrote, created or replaced it. Nothing was written.
    ChangedOnDisk,
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::ChangedOnDisk => f.write_str("the file changed on disk since it was read"),
            SaveError::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SaveError::ChangedOnDisk => None,
            SaveError::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        SaveError::Io(error)
    }
}

impl From<SaveError> for io::Error {
    /// An I/O error as it was; a conflict as an error whose inner error
    /// downcasts to [`SaveError`].
    fn from(error: SaveError) -> Self {
        match error {
            SaveError::Io(error) => error,
            conflict @ SaveError::ChangedOnDisk => io::Error::other(conflict),
        }
    }
}

/// A save to run anywhere, made by [`EditorModel::prepare_save`]: the text
/// to write and the file it was read from. A clone saves the same text.
#[derive(Clone)]
pub struct SaveJob {
    path: PathBuf,
    snapshot: SaveSnapshot,
    /// The file as the buffer last read or saved it; `None` when the
    /// buffer doesn't know.
    expected: Option<Option<DiskStamp>>,
    overwrite: bool,
}

/// A save that's on disk, for [`EditorModel::finish_save`].
pub struct Saved {
    path: PathBuf,
    snapshot: SaveSnapshot,
    stamp: Option<DiskStamp>,
}

impl Saved {
    /// What was written.
    pub fn snapshot(&self) -> &SaveSnapshot {
        &self.snapshot
    }

    /// The new file's stamp.
    pub fn stamp(&self) -> Option<DiskStamp> {
        self.stamp
    }
}

impl SaveJob {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn snapshot(&self) -> &SaveSnapshot {
        &self.snapshot
    }

    /// The same save, over the file even if it changed on disk since it
    /// was read: to retry a job refused with [`SaveError::ChangedOnDisk`].
    pub fn overwriting(mut self) -> Self {
        self.overwrite = true;
        self
    }

    /// Saves atomically: writes a new file beside the target and renames it
    /// over the original, so a failed save never truncates the file.
    ///
    /// A path that exists but isn't a regular file fails first with an I/O
    /// error, never as a change on disk.
    ///
    /// Unless the job overwrites, it then fails with
    /// [`SaveError::ChangedOnDisk`], writing nothing, if the file's stamp
    /// isn't the one it had when read, including a file that didn't exist
    /// then and does now. A file deleted since is saved. A buffer that
    /// doesn't know the file it came from isn't checked. Then, once the new
    /// file is written and synced, it fails the same way, discarding it, if
    /// the file or the one a symlink points to changed meanwhile.
    ///
    /// The new file gets a random name and is created exclusively with
    /// owner-only permissions, so it never follows a planted symlink and is
    /// never readable by others mid-save. The original's permissions are then
    /// applied through the open handle. If the path is a symlink, the file it
    /// points to is updated and the link is left in place.
    pub fn run(self) -> Result<Saved, SaveError> {
        self.run_with(|| {})
    }

    /// [`SaveJob::run`], calling `before_commit` between writing the new
    /// file and checking the target again.
    fn run_with(self, before_commit: impl FnOnce()) -> Result<Saved, SaveError> {
        let SaveJob {
            path,
            snapshot,
            expected,
            overwrite,
        } = self;
        ensure_regular(&path)?;
        let found = DiskStamp::of(&path)?;
        if !overwrite
            && found.is_some()
            && let Some(expected) = expected
            && found != expected
        {
            return Err(SaveError::ChangedOnDisk);
        }
        let target = resolve(&path)?;
        let permissions = match std::fs::metadata(&target) {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let dir = match target.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        let file_name = target.file_name().unwrap_or_default().to_string_lossy();
        let temp = tempfile::Builder::new()
            .prefix(&format!(".{file_name}."))
            .suffix(".fastedit-save")
            .tempfile_in(dir)?;
        if let Some(permissions) = permissions {
            temp.as_file().set_permissions(permissions)?;
        }
        let mut out = io::BufWriter::new(temp);
        snapshot.write_to(&mut out)?;
        let temp = out.into_inner().map_err(|error| error.into_error())?;
        temp.as_file().sync_all()?;
        // Renaming keeps the file's length, time and inode.
        let stamp = DiskStamp::from_metadata(&temp.as_file().metadata()?)?;
        before_commit();
        // Narrows the window another writer has to the rename itself.
        ensure_regular(&path)?;
        let now = resolve(&path)?;
        ensure_regular(&now)?;
        if !overwrite && (DiskStamp::of(&path)? != found || now != target) {
            return Err(SaveError::ChangedOnDisk);
        }
        // Dropping `temp` on any error above deletes it.
        temp.persist(&target).map_err(|error| error.error)?;
        Ok(Saved {
            path,
            snapshot,
            stamp: Some(stamp),
        })
    }
}

/// Fails with the I/O error a write would, if `path` (symlinks followed)
/// exists and isn't a regular file: [`io::ErrorKind::IsADirectory`] for a
/// directory, so a save never reports that as a change on disk.
fn ensure_regular(path: &Path) -> io::Result<()> {
    let file_type = match std::fs::metadata(path) {
        Ok(metadata) => metadata.file_type(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if file_type.is_file() {
        Ok(())
    } else if file_type.is_dir() {
        #[cfg(unix)]
        return Err(io::Error::from_raw_os_error(21)); // EISDIR
        #[cfg(not(unix))]
        return Err(io::Error::new(
            io::ErrorKind::IsADirectory,
            "Is a directory",
        ));
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ))
    }
}

/// The file a save to `path` writes: `path` with its symlinks resolved,
/// even when the last one's target is missing, so a save recreates the
/// target rather than replacing the link.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        resolved => return resolved,
    }
    let mut path = path.to_path_buf();
    // Linux's limit on links followed in one lookup.
    for _ in 0..40 {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                // A relative link is relative to its directory; joining an
                // absolute one replaces the base.
                let link = std::fs::read_link(&path)?;
                path = match path.parent() {
                    Some(dir) => dir.join(link),
                    None => link,
                };
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => return Ok(path),
        }
    }
    Err(io::Error::other("too many levels of symbolic links"))
}

impl EditorModel {
    /// A save of the buffer as it is now, to [run](SaveJob::run) on any
    /// thread and then [finish](EditorModel::finish_save). Unless
    /// `overwrite`, it refuses a file changed since it was read. Fails with
    /// [`io::ErrorKind::NotFound`] for a buffer with no path.
    pub fn prepare_save(&self, overwrite: bool) -> io::Result<SaveJob> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "buffer has no path"))?;
        // Typing after this point is a step of its own, in any split.
        self.history.borrow_mut().save_marks += 1;
        let buffer = self.buffer.borrow();
        Ok(SaveJob {
            path,
            snapshot: buffer.save_snapshot(),
            expected: buffer.disk_stamp_known().then(|| buffer.disk_stamp()),
            overwrite,
        })
    }

    /// Records a finished save: the buffer is clean unless edited since the
    /// job was prepared. Returns whether it was recorded. It isn't when the
    /// file is no longer the one the job wrote (another save or writer
    /// replaced it after; saves can finish out of order), or the buffer
    /// already holds it (a reload read it).
    pub fn finish_save(&mut self, saved: Saved) -> bool {
        let on_disk = DiskStamp::of(&saved.path).ok().flatten();
        let mut buffer = self.buffer.borrow_mut();
        if on_disk != saved.stamp || buffer.disk_stamp() == saved.stamp {
            return false;
        }
        buffer.did_save_snapshot(&saved.snapshot, saved.stamp);
        true
    }

    /// Saves on this thread, as [`SaveJob::run`] does. A file changed on disk
    /// since it was read is refused with an error whose inner error
    /// downcasts to [`SaveError::ChangedOnDisk`].
    pub fn save(&mut self) -> io::Result<()> {
        self.save_with(false)
    }

    /// Saves over the file even if it changed on disk since it was read.
    pub fn save_overwriting(&mut self) -> io::Result<()> {
        self.save_with(true)
    }

    fn save_with(&mut self, overwrite: bool) -> io::Result<()> {
        let saved = self.prepare_save(overwrite)?.run()?;
        // Only a write between the rename and this check goes unrecorded.
        self.finish_save(saved);
        Ok(())
    }

    /// Replaces the buffer's text with `file`'s, read from disk with
    /// `stamp`, as one user edit, so undo takes it back; see
    /// [`fe_buffer::Buffer::reload`]. Carets and folds outside the change
    /// stay put.
    pub fn reload(&mut self, file: DecodedFile, stamp: Option<DiskStamp>) -> Option<TransactionId> {
        self.sync_display();
        self.end_typing();
        let id = self.buffer.borrow_mut().reload(file, stamp, Origin::User);
        self.sync_display();
        id
    }
}

#[cfg(test)]
mod tests {
    use fe_buffer::Buffer;

    use super::*;

    fn job(path: &Path, text: &str) -> SaveJob {
        let model = EditorModel::open(path).unwrap();
        let mut buffer = model.buffer.borrow_mut();
        let len = buffer.len();
        buffer.edit([(0..len, text)], Origin::User);
        drop(buffer);
        model.prepare_save(false).unwrap()
    }

    #[test]
    fn a_write_during_the_save_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "old").unwrap();

        let result = job(&path, "mine").run_with(|| std::fs::write(&path, "theirs").unwrap());
        assert!(matches!(result, Err(SaveError::ChangedOnDisk)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "temp file gone"
        );

        // Created during a save to a file that was missing.
        let new = dir.path().join("new.txt");
        let mut model = EditorModel::new(Buffer::new("mine"), Some(new.clone()));
        model.buffer_mut().set_disk_stamp(None);
        let result = model
            .prepare_save(false)
            .unwrap()
            .run_with(|| std::fs::write(&new, "theirs").unwrap());
        assert!(matches!(result, Err(SaveError::ChangedOnDisk)));
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "theirs");

        // Overwriting doesn't check.
        let mut job = job(&path, "mine");
        job.overwrite = true;
        job.run_with(|| std::fs::write(&path, "again").unwrap())
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_retargeted_during_the_save_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, link) = (
            dir.path().join("a.txt"),
            dir.path().join("b.txt"),
            dir.path().join("link.txt"),
        );
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        std::os::unix::fs::symlink(&a, &link).unwrap();

        let result = job(&link, "mine").run_with(|| {
            std::fs::remove_file(&link).unwrap();
            std::os::unix::fs::symlink(&b, &link).unwrap();
        });
        assert!(matches!(result, Err(SaveError::ChangedOnDisk)));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b");
    }

    #[cfg(unix)]
    #[test]
    fn a_target_that_became_a_fifo_during_the_save_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, link) = (
            dir.path().join("a.txt"),
            dir.path().join("b.txt"),
            dir.path().join("link.txt"),
        );
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        std::os::unix::fs::symlink(&a, &link).unwrap();

        for overwrite in [false, true] {
            let mut job = job(&link, "mine");
            job.overwrite = overwrite;
            let result = job.run_with(|| {
                std::fs::remove_file(&a).unwrap();
                let made = std::process::Command::new("mkfifo").arg(&a).status();
                assert!(made.unwrap().success(), "mkfifo");
            });
            let error = match result {
                Err(SaveError::Io(error)) => error,
                _ => panic!("expected an I/O error, overwrite={overwrite}"),
            };
            assert_ne!(error.kind(), io::ErrorKind::Other, "{error}");
            assert!(!std::fs::metadata(&a).unwrap().is_file());
            assert_eq!(
                std::fs::read_dir(dir.path()).unwrap().count(),
                3,
                "temp gone"
            );
            std::fs::remove_file(&a).unwrap();
            std::fs::write(&a, "a").unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_deleted_file_recreates_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let (real, link, chain) = (
            dir.path().join("sub/real.txt"),
            dir.path().join("link.txt"),
            dir.path().join("chain.txt"),
        );
        std::fs::write(&real, "real").unwrap();
        std::os::unix::fs::symlink("sub/real.txt", &link).unwrap();
        std::os::unix::fs::symlink(&link, &chain).unwrap();

        let mut model = EditorModel::open(&chain).unwrap();
        std::fs::remove_file(&real).unwrap();
        model.insert("new ");
        model.save().unwrap();
        for path in [&link, &chain] {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert!(metadata.file_type().is_symlink(), "{path:?} kept");
        }
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new real");
        assert!(!model.is_dirty());
    }
}
