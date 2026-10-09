//! What [`FileView::check`](crate::FileView::check) compares: a file's length,
//! mtime and identity.

use std::fs::{File, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) len: u64,
    modified: Option<SystemTime>,
    /// Device and inode on Unix, volume serial number and 128-bit file ID on
    /// Windows. A file replaced by another at the same path differs here even
    /// with the same length and mtime.
    id: (u64, u128),
}

impl Stamp {
    fn new(metadata: &Metadata, id: (u64, u128)) -> Stamp {
        Stamp {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            id,
        }
    }
}

#[cfg(unix)]
impl Stamp {
    pub(crate) fn of_file(file: &File) -> io::Result<Stamp> {
        Ok(Stamp::of_metadata(&file.metadata()?))
    }

    /// Stats the path without opening it, so a path replaced by a FIFO can't
    /// block.
    pub(crate) fn of_path(path: &Path) -> io::Result<Stamp> {
        Ok(Stamp::of_metadata(&std::fs::metadata(path)?))
    }

    fn of_metadata(metadata: &Metadata) -> Stamp {
        use std::os::unix::fs::MetadataExt;
        Stamp::new(metadata, (metadata.dev(), u128::from(metadata.ino())))
    }
}

#[cfg(windows)]
impl Stamp {
    pub(crate) fn of_file(file: &File) -> io::Result<Stamp> {
        Ok(Stamp::new(&file.metadata()?, windows::file_id(file)?))
    }

    /// std exposes a file's identity only through a handle, so the path is
    /// opened (read-only, sharing read, write and delete) to stat it.
    pub(crate) fn of_path(path: &Path) -> io::Result<Stamp> {
        Stamp::of_file(&File::open(path)?)
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;

    /// `FILE_ID_INFO`.
    #[repr(C)]
    struct FileIdInfo {
        volume_serial_number: u64,
        file_id: [u8; 16],
    }

    /// `FileIdInfo` in `FILE_INFO_BY_HANDLE_CLASS`.
    const FILE_ID_INFO_CLASS: i32 = 18;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut c_void,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
    }

    /// The volume serial number and 128-bit file ID of an open file.
    pub(super) fn file_id(file: &File) -> io::Result<(u64, u128)> {
        let mut info = FileIdInfo {
            volume_serial_number: 0,
            file_id: [0; 16],
        };
        // SAFETY: the handle is open for as long as `file` is borrowed, and
        // `info` is a `FILE_ID_INFO` of the size passed.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FILE_ID_INFO_CLASS,
                (&raw mut info).cast(),
                size_of::<FileIdInfo>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((info.volume_serial_number, u128::from_le_bytes(info.file_id)))
    }
}
