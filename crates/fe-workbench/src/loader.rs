//! Reads files off the main thread. The host's launch file is read by
//! [`start`], at the top of `main`, so the window paints first and a small
//! file is usually read by the time the window is built, and is on its first
//! frame. Files opened from the project panel are read by [`read`] on GPUI's
//! background executor.
//!
//! Only the [`Buffer`] crosses threads: `EditorModel` isn't `Send` (it holds
//! its char widths as a `Box<dyn CharWidth>`), so the main thread builds it.
//!
//! A file of [`LARGE_FILE_BYTES`] or more isn't read: [`open`] opens it as a
//! read-only [`FileView`] and starts its [`LineIndex`] on a thread of its
//! own (docs/decisions/mmap-read-only.md).

use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::Instant,
};

use fe_buffer::{Buffer, DecodedFile, DiskStamp, FileEncoding};
use fe_fileview::{FileView, LineIndex};

/// Files this large or larger open read-only in a viewer, unread.
pub const LARGE_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// A file [`open`] opened: read into a buffer, or too large for one.
#[allow(clippy::large_enum_variant)] // Made once per open, and moved on.
pub enum Opened {
    /// What [`read`] read: `None` for a missing file.
    Text(Option<Buffer>),
    /// A file of [`LARGE_FILE_BYTES`] or more, and its index, which is
    /// still building on a thread of its own.
    Large {
        view: Arc<FileView>,
        index: Arc<LineIndex>,
    },
    /// An image file, unread: the image viewer reads and decodes it
    /// itself, off the main thread.
    Image,
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Opened::Text(buffer) => write!(f, "Text({:?} bytes)", buffer.as_ref().map(Buffer::len)),
            Opened::Large { view, .. } => write!(f, "Large({})", view.path().display()),
            Opened::Image => write!(f, "Image"),
        }
    }
}

impl From<Buffer> for Opened {
    fn from(buffer: Buffer) -> Self {
        Opened::Text(Some(buffer))
    }
}

/// Opens `path`: a file of [`LARGE_FILE_BYTES`] or more as a view whose
/// index starts building, anything else as [`read`] does.
pub fn open(path: PathBuf) -> io::Result<Opened> {
    if is_image(&path) {
        return Ok(Opened::Image);
    }
    if !is_large(&path) {
        return read(path).map(Opened::Text);
    }
    let view = Arc::new(FileView::open(&path)?);
    let index = Arc::new(build_index(view.clone()));
    Ok(Opened::Large { view, index })
}

/// Whether `path` names an image the image viewer shows, by its extension
/// (png, jpg, jpeg, gif, webp, bmp, ico, svg or svgz, in any case).
pub(crate) fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            [
                "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "svgz",
            ]
            .iter()
            .any(|image| ext.eq_ignore_ascii_case(image))
        })
}

/// Whether `path` is an uncompressed SVG (`.svg`, in any case), the one image
/// format that is also text. (A `.svgz` is a viewer only.)
pub(crate) fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}

/// Whether `path` is a file of [`LARGE_FILE_BYTES`] or more (lower in
/// tests that lower it), which [`open`] opens in a viewer, unread.
pub(crate) fn is_large(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() >= large_bytes(path))
}

/// How many bytes make `path` a file too large to edit:
/// [`LARGE_FILE_BYTES`], or less in a test that lowers it.
pub(crate) fn large_bytes(path: &Path) -> u64 {
    threshold(path)
}

#[cfg(not(test))]
fn threshold(_: &Path) -> u64 {
    LARGE_FILE_BYTES
}

#[cfg(not(test))]
fn build_index(view: Arc<FileView>) -> LineIndex {
    LineIndex::build(view)
}

/// Lowers the threshold for the files a test makes, and holds their
/// indexes back.
#[cfg(test)]
pub(crate) mod test_support {
    use std::{
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use fe_fileview::{FileView, LineIndex, Step};

    /// The directories with a lowered threshold: the threshold, and
    /// whether their files' indexes wait to be stepped.
    static LOWERED: Mutex<Vec<(PathBuf, u64, bool)>> = Mutex::new(Vec::new());
    /// The held indexes' steppers, by path.
    static STEPS: Mutex<Vec<(PathBuf, Step)>> = Mutex::new(Vec::new());

    /// Opens files of `bytes` or more under `dir` as large ones; with
    /// `held`, their indexes scan only as [`take_steps`]'s steppers allow.
    /// Other tests' files keep the real threshold.
    pub(crate) fn lower_threshold(dir: &Path, bytes: u64, held: bool) {
        let mut lowered = LOWERED.lock().unwrap();
        lowered.retain(|(at, ..)| at != dir);
        lowered.push((dir.to_owned(), bytes, held));
    }

    /// The steppers of the held indexes of `path` built so far. Dropping
    /// one lets its index finish.
    pub(crate) fn take_steps(path: &Path) -> Vec<Step> {
        let mut steps = STEPS.lock().unwrap();
        let (taken, kept) = std::mem::take(&mut *steps)
            .into_iter()
            .partition(|(at, _)| at == path);
        *steps = kept;
        taken.into_iter().map(|(_, step)| step).collect()
    }

    fn lowered(path: &Path) -> Option<(u64, bool)> {
        let lowered = LOWERED.lock().unwrap();
        (lowered.iter())
            .find(|(dir, ..)| path.starts_with(dir))
            .map(|(_, bytes, held)| (*bytes, *held))
    }

    pub(super) fn threshold(path: &Path) -> u64 {
        lowered(path).map_or(super::LARGE_FILE_BYTES, |(bytes, _)| bytes)
    }

    pub(super) fn build_index(view: Arc<FileView>) -> LineIndex {
        if !lowered(view.path()).is_some_and(|(_, held)| held) {
            return LineIndex::build(view);
        }
        let path = view.path().to_owned();
        let (index, step) = LineIndex::build_stepped(view);
        STEPS.lock().unwrap().push((path, step));
        index
    }
}

#[cfg(test)]
use test_support::{build_index, threshold};

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

/// Reads `path` again, decoded in `encoding` rather than the one detected
/// ([`Buffer::read_file_with`]), for a buffer to reload it, with its stamp
/// taken before reading. A missing file is an error: there's nothing to
/// reopen.
pub fn read_with(path: &Path, encoding: FileEncoding) -> io::Result<(DecodedFile, DiskStamp)> {
    Buffer::read_file_with(File::open(path)?, encoding)
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

/// A finished read: what [`open`] returned, and when.
pub struct Read {
    pub result: io::Result<Opened>,
    pub at: Instant,
}

/// Starts opening `path`, as [`open`] does, on a new thread.
pub fn start(path: PathBuf) -> Reading {
    let (sender, done) = mpsc::sync_channel(1);
    let reader_path = path.clone();
    thread::Builder::new()
        .name("read file".into())
        .spawn(move || {
            let result = open(reader_path);
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
#[path = "image_viewer_open_tests.rs"]
mod image_viewer_open_tests;

#[cfg(test)]
#[path = "svg_view_as_text_tests.rs"]
mod svg_view_as_text_tests;

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
        let Ok(Opened::Text(Some(buffer))) = read.result else {
            panic!("a small file is read into a buffer");
        };
        assert_eq!(buffer.text().to_string(), "text");
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
