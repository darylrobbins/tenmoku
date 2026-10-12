//! Reads files off the main thread. The host's launch file is read by
//! [`start`], at the top of `main`, so the window paints first and a small
//! file is usually read by the time the window is built, and is on its first
//! frame. Files opened from the project panel are read by [`read`] on GPUI's
//! background executor.
//!
//! Only the [`Buffer`] crosses threads: `EditorModel` isn't `Send` (it holds
//! its char widths as a `Box<dyn CharWidth>`), so the main thread builds it.

use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Instant,
};

use fe_buffer::Buffer;

/// Reads `path` into a buffer, detecting its encoding (`EditorModel::open`
/// doesn't), so saving writes it back the same way, with the file's stamp
/// ([`Buffer::from_file`]), so saving refuses a file changed since. A
/// missing file is `Ok(None)`: a new file, which saves as UTF-8; open it in
/// [`buffer_or_new`].
pub fn read(path: PathBuf) -> io::Result<Option<Buffer>> {
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Buffer::from_file(file).map(Some)
}

/// The buffer [`read`] read, or for a missing file an empty one that knows
/// the file was missing, so a save refuses one created since.
pub fn buffer_or_new(read: Option<Buffer>) -> Buffer {
    read.unwrap_or_else(|| {
        let mut buffer = Buffer::new("");
        buffer.set_disk_stamp(None);
        buffer
    })
}

/// A read running on its own thread.
pub struct Reading {
    path: PathBuf,
    done: mpsc::Receiver<Read>,
}

/// A finished read: what [`read`] returned, and when.
pub struct Read {
    pub result: io::Result<Option<Buffer>>,
    pub at: Instant,
}

/// Starts reading `path` on a new thread.
pub fn start(path: PathBuf) -> Reading {
    let (sender, done) = mpsc::sync_channel(1);
    let reader_path = path.clone();
    thread::Builder::new()
        .name("read file".into())
        .spawn(move || {
            let result = read(reader_path);
            sender
                .send(Read {
                    result,
                    at: Instant::now(),
                })
                .ok();
        })
        .expect("failed to start the file reader thread");
    Reading { path, done }
}

impl Reading {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The read, if it has finished. It's returned once.
    pub fn try_finish(&self) -> Option<Read> {
        self.done.try_recv().ok()
    }

    /// Waits for the read to finish; don't call it on the main thread.
    pub fn finish(self) -> Read {
        self.done.recv().expect("the reader thread panicked")
    }
}

#[cfg(test)]
#[path = "loader_encoding_tests.rs"]
mod encoding_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_file_into_buffer_off_the_calling_thread() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "one\r\ntwo\n").unwrap();

        let caller = std::thread::current().id();
        let (reader, buffer) =
            std::thread::spawn(move || (std::thread::current().id(), read(path)))
                .join()
                .unwrap();

        assert_ne!(reader, caller);
        let buffer = buffer.unwrap().expect("the file exists");
        assert_eq!(buffer.text().to_string(), "one\ntwo\n");
    }

    #[test]
    fn started_read_finishes_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "text").unwrap();

        let reading = start(path.clone());
        assert_eq!(reading.path(), path);
        let read = reading.finish();
        assert_eq!(read.result.unwrap().unwrap().text().to_string(), "text");
    }

    #[test]
    fn missing_file_is_a_new_buffer() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path().join("new.txt")).unwrap().is_none());
    }

    #[test]
    fn saves_refuse_files_changed_after_the_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "read").unwrap();
        let buffer = read(path.clone()).unwrap().unwrap();
        // Rewritten between the read and the model being made.
        std::fs::write(&path, "theirs").unwrap();
        let mut model =
            fe_editor::EditorModel::new(buffer_or_new(Some(buffer)), Some(path.clone()));
        model.insert("mine ");
        assert!(model.save().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");

        // Missing when read, created before the first save.
        let new = dir.path().join("new.txt");
        let buffer = buffer_or_new(read(new.clone()).unwrap());
        assert!(buffer.disk_stamp_known());
        let mut model = fe_editor::EditorModel::new(buffer, Some(new.clone()));
        std::fs::write(&new, "theirs").unwrap();
        model.insert("mine");
        assert!(model.save().is_err());
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "theirs");
    }

    #[test]
    fn unreadable_file_is_an_error() {
        // A directory opens, but reading it fails.
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path().to_path_buf()).is_err());
    }
}
