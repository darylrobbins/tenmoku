//! A read-only view of a very large file, read with `pread` into owned buffers.
//!
//! Files too big to load into a rope are browsed through a [`FileView`]
//! instead (`docs/decisions/mmap-read-only.md`). The file is never mapped:
//! another process can truncate or rewrite it at any time, and a mapping
//! would raise SIGBUS or break Rust's guarantees about the mapped bytes. Every
//! read copies bytes into a buffer this crate owns, so a file that changes
//! underneath only shows up as [`ReadError::Changed`] or as stale text.
//!
//! Reads go through a bounded cache of [`CHUNK_BYTES`]-sized chunks aligned to
//! multiples of that size, keeping at most [`CACHE_CHUNKS`] of them (16 MiB)
//! and evicting the least recently used. Text is decoded lossily, as
//! [`String::from_utf8_lossy`] does, because large logs are often not valid
//! UTF-8.
//!
//! A [`LineIndex`] maps rows to offsets and back, built on a background
//! thread while the first screen is already showing. A [`FileText`] joins the
//! two into a `fe_text::TextSource`.
//!
//! It must never depend on GPUI.

use std::fs::File;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

pub use find::{Direction, FIND_BLOCK, FindOutcome, FindQuery, find};
pub use index::{IndexState, LineIndex, Progress, STRIDE, Step};
pub use source::FileText;
use stamp::Stamp;

mod find;
mod index;
mod source;
mod stamp;

/// The size of a cached chunk, and the alignment of every chunk's start.
pub const CHUNK_BYTES: u64 = 256 * 1024;

/// How many chunks the cache keeps before evicting the least recently used.
pub const CACHE_CHUNKS: usize = 64;

/// A UTF-8 char is at most 4 bytes, so a char start is at most 3 bytes back.
const MAX_SNAP_BACK: u64 = 3;

/// Why a read failed.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The file no longer holds what was recorded at open: a read came back
    /// short of the recorded length, or hit end of file inside it. Reopen the
    /// file to see its current contents.
    #[error("the file changed on disk since it was opened")]
    Changed,
    /// Any other I/O failure.
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl ReadError {
    pub fn kind(&self) -> ReadErrorKind {
        match self {
            ReadError::Changed => ReadErrorKind::Changed,
            ReadError::Io(_) => ReadErrorKind::Io,
        }
    }
}

/// A [`ReadError`] without its `io::Error`, so it can be copied and compared.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ReadErrorKind {
    Changed,
    Io,
}

/// Whether the file on disk still looks like the one that was opened. See
/// [`FileView::check`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Freshness {
    Unchanged,
    Changed,
}

/// Lines read by [`FileView::rows_from`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rows {
    /// The lines, decoded lossily, without their `\n` or `\r\n`.
    pub lines: Vec<String>,
    /// The byte offset just after the last line's terminator: the start of
    /// the next line, or the recorded length at end of file.
    pub end: u64,
}

/// A read-only, `pread`-backed view of a file. `Send + Sync`; reads may come
/// from several threads at once.
#[derive(Debug)]
pub struct FileView {
    path: PathBuf,
    file: File,
    stamp: Stamp,
    cache: Mutex<Cache>,
}

impl FileView {
    /// Opens `path` read-only and records its length, mtime and identity.
    /// Nothing is read until the first read.
    pub fn open(path: impl AsRef<Path>) -> io::Result<FileView> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        if file.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "a directory has no text to view",
            ));
        }
        let stamp = Stamp::of_file(&file)?;
        Ok(FileView {
            path,
            file,
            stamp,
            cache: Mutex::new(Cache::default()),
        })
    }

    /// The path the view was opened with.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's length when it was opened. Reads never go past it.
    pub fn len(&self) -> u64 {
        self.stamp.len
    }

    pub fn is_empty(&self) -> bool {
        self.stamp.len == 0
    }

    /// The bytes held by the chunk cache, at most
    /// `CHUNK_BYTES * CACHE_CHUNKS` (16 MiB).
    pub fn cached_bytes(&self) -> u64 {
        self.lock()
            .chunks
            .iter()
            .map(|slot| slot.data.len() as u64)
            .sum()
    }

    /// The text in `range`, decoded lossily like [`String::from_utf8_lossy`].
    ///
    /// The range is first clamped to the recorded length, and each end is
    /// snapped back to a char start: past at most 3 UTF-8 continuation bytes.
    /// So a range that starts or ends inside a multi-byte char includes that
    /// char's start but not its end. A char that straddles a chunk edge decodes
    /// whole.
    pub fn read(&self, range: Range<u64>) -> Result<String, ReadError> {
        let len = self.len();
        let end = range.end.min(len);
        let start = range.start.min(end);
        // Snapping needs the bytes up to 3 before each end, and the byte at
        // `end` itself to know whether `end` is a char start.
        let lo = start.saturating_sub(MAX_SNAP_BACK);
        let hi = (end + 1).min(len);
        let bytes = self.read_bytes(lo..hi)?;
        let snap = |mut at: u64| {
            for _ in 0..MAX_SNAP_BACK {
                if at == 0 || at >= len || !is_continuation(bytes[(at - lo) as usize]) {
                    break;
                }
                at -= 1;
            }
            at
        };
        let (start, end) = (snap(start), snap(end));
        Ok(
            String::from_utf8_lossy(&bytes[(start - lo) as usize..(end - lo) as usize])
                .into_owned(),
        )
    }

    /// Up to `count` lines read forward from `offset`, which must be a line
    /// start (`0`, or a previous [`Rows::end`]). Lines are returned without
    /// their `\n` or `\r\n` and decoded lossily; a last line with no
    /// terminator ends at the recorded length. This is how the first screen
    /// is shown before any line index exists.
    ///
    /// A line is read whole however long it is, so a file with no line
    /// breaks reads to its end; bounding that is the line index's job.
    pub fn rows_from(&self, offset: u64, count: usize) -> Result<Rows, ReadError> {
        let len = self.len();
        let mut at = offset.min(len);
        let mut lines = Vec::new();
        let mut line = Vec::new();
        while lines.len() < count && at < len {
            let chunk = self.chunk(at / CHUNK_BYTES)?;
            let rest = &chunk[(at % CHUNK_BYTES) as usize..];
            match rest.iter().position(|&b| b == b'\n') {
                Some(newline) => {
                    line.extend_from_slice(&rest[..newline]);
                    at += newline as u64 + 1;
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    lines.push(String::from_utf8_lossy(&line).into_owned());
                    line.clear();
                }
                None => {
                    line.extend_from_slice(rest);
                    at += rest.len() as u64;
                }
            }
        }
        // Only reached with bytes left over at the recorded end.
        if !line.is_empty() {
            lines.push(String::from_utf8_lossy(&line).into_owned());
        }
        Ok(Rows { lines, end: at })
    }

    /// Re-stats the path and compares its length, mtime and identity (device
    /// and inode, or on Windows volume and file ID) with what was recorded at
    /// open. A path that can't be stated counts as changed.
    ///
    /// This is a UX signal for a "file changed, reload" prompt, not a safety
    /// mechanism: the file can change between this check and the next read
    /// (`docs/decisions/mmap-read-only.md`, "Safety"). Reads are safe on their
    /// own because they copy into owned buffers and report short reads as
    /// [`ReadError::Changed`].
    pub fn check(&self) -> Freshness {
        match Stamp::of_path(&self.path) {
            Ok(stamp) if stamp == self.stamp => Freshness::Unchanged,
            _ => Freshness::Changed,
        }
    }

    /// The raw bytes in `range`, which lies within the recorded length.
    fn read_bytes(&self, range: Range<u64>) -> Result<Vec<u8>, ReadError> {
        let mut bytes = Vec::with_capacity((range.end - range.start) as usize);
        let mut at = range.start;
        while at < range.end {
            let index = at / CHUNK_BYTES;
            let chunk = self.chunk(index)?;
            let from = (at - index * CHUNK_BYTES) as usize;
            let to = (range.end - index * CHUNK_BYTES).min(chunk.len() as u64) as usize;
            bytes.extend_from_slice(&chunk[from..to]);
            at += (to - from) as u64;
        }
        Ok(bytes)
    }

    /// Fills `buf` with the bytes from `offset` straight from the file,
    /// bypassing the chunk cache, so a scan of the whole file doesn't evict
    /// what the screen is reading. The bytes lie within the recorded length.
    pub(crate) fn read_uncached(&self, buf: &mut [u8], offset: u64) -> Result<(), ReadError> {
        debug_assert!(offset + buf.len() as u64 <= self.len());
        fill(buf, |buf| read_at(&self.file, buf, offset))
    }

    /// Chunk `index`, from the cache or read from disk. The lock is not held
    /// while reading, so a slow disk doesn't block cache hits on other threads.
    fn chunk(&self, index: u64) -> Result<Arc<Vec<u8>>, ReadError> {
        if let Some(data) = self.lock().get(index) {
            return Ok(data);
        }
        let start = index * CHUNK_BYTES;
        let want = CHUNK_BYTES.min(self.len().saturating_sub(start)) as usize;
        let mut data = vec![0; want];
        fill(&mut data, |buf| read_at(&self.file, buf, start))?;
        Ok(self.lock().insert(index, Arc::new(data)))
    }

    fn lock(&self) -> MutexGuard<'_, Cache> {
        // Nothing panics while holding the lock, but a poisoned cache is
        // still a consistent one.
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Fills `buf` with one successful `read`, retrying only interrupted ones.
///
/// A regular file's positional read returns fewer bytes than asked for (256
/// KiB, far below any platform's per-call cap) only at end of file, so a
/// short read means the file is shorter than recorded, even if it grows back
/// before another read could finish the buffer.
fn fill(
    buf: &mut [u8],
    mut read: impl FnMut(&mut [u8]) -> io::Result<usize>,
) -> Result<(), ReadError> {
    loop {
        match read(buf) {
            Ok(n) if n == buf.len() => return Ok(()),
            Ok(_) => return Err(ReadError::Changed),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(ReadError::Io(error)),
        }
    }
}

fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

/// The least-recently-used chunk cache. A linear scan over 64 slots is
/// cheaper than reading even one chunk.
#[derive(Debug, Default)]
struct Cache {
    chunks: Vec<Slot>,
    tick: u64,
}

#[derive(Debug)]
struct Slot {
    index: u64,
    data: Arc<Vec<u8>>,
    used: u64,
}

impl Cache {
    fn get(&mut self, index: u64) -> Option<Arc<Vec<u8>>> {
        self.tick += 1;
        let slot = self.chunks.iter_mut().find(|slot| slot.index == index)?;
        slot.used = self.tick;
        Some(Arc::clone(&slot.data))
    }

    /// Caches `data` as chunk `index`, unless another thread read it first,
    /// and returns the cached copy.
    fn insert(&mut self, index: u64, data: Arc<Vec<u8>>) -> Arc<Vec<u8>> {
        if let Some(cached) = self.get(index) {
            return cached;
        }
        if self.chunks.len() >= CACHE_CHUNKS
            && let Some(oldest) = (0..self.chunks.len()).min_by_key(|&ix| self.chunks[ix].used)
        {
            self.chunks.swap_remove(oldest);
        }
        self.chunks.push(Slot {
            index,
            data: Arc::clone(&data),
            used: self.tick,
        });
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_read_is_changed_even_if_the_rest_would_follow() {
        let mut calls = 0;
        let result = fill(&mut [0; 8], |buf| {
            calls += 1;
            let n = if calls == 1 { 3 } else { buf.len() };
            buf[..n].fill(b'x');
            Ok(n)
        });
        assert!(matches!(result, Err(ReadError::Changed)));
        assert_eq!(calls, 1);

        assert!(matches!(
            fill(&mut [0; 8], |_| Ok(0)),
            Err(ReadError::Changed)
        ));
        assert!(matches!(fill(&mut [], |_| Ok(0)), Ok(())));
    }

    #[test]
    fn interrupted_reads_retry_and_other_errors_are_io() {
        let mut calls = 0;
        let mut buf = [0; 4];
        let result = fill(&mut buf, |buf| {
            calls += 1;
            if calls < 3 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            buf.copy_from_slice(b"abcd");
            Ok(4)
        });
        assert!(result.is_ok());
        assert_eq!(&buf, b"abcd");

        let result = fill(&mut [0; 4], |_| Err(io::ErrorKind::PermissionDenied.into()));
        assert!(
            matches!(result, Err(ReadError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied)
        );
    }
}
