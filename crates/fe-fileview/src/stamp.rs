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

/// Windows error codes (`winerror.h`) a filesystem answers with when it doesn't
/// implement the extended identity class: `ERROR_INVALID_FUNCTION`,
/// `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_PARAMETER`,
/// `ERROR_CALL_NOT_IMPLEMENTED`, `ERROR_INVALID_LEVEL` (what CIFS/SMB returns).
#[cfg(any(windows, test))]
fn identity_class_unsupported(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(1 | 50 | 87 | 120 | 124))
}

/// The extended identity, or the legacy one when the filesystem (FAT, some
/// CIFS/SMB shares) rejects the extended class. Any other error propagates.
/// A given file always takes the same route, so open and check agree.
#[cfg(any(windows, test))]
fn identity_with_fallback(
    extended: io::Result<(u64, u128)>,
    legacy: impl FnOnce() -> io::Result<(u64, u128)>,
) -> io::Result<(u64, u128)> {
    match extended {
        Err(error) if identity_class_unsupported(&error) => legacy(),
        other => other,
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

    /// `BY_HANDLE_FILE_INFORMATION`.
    #[repr(C)]
    #[derive(Default)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: [u32; 2],
        last_access_time: [u32; 2],
        last_write_time: [u32; 2],
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
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
        fn GetFileInformationByHandle(file: *mut c_void, info: *mut ByHandleFileInformation)
        -> i32;
    }

    /// The volume serial number and file ID of an open file: 128-bit from
    /// `FileIdInfo`, else the 64-bit file index of `GetFileInformationByHandle`.
    pub(super) fn file_id(file: &File) -> io::Result<(u64, u128)> {
        super::identity_with_fallback(extended_id(file), || legacy_id(file))
    }

    fn extended_id(file: &File) -> io::Result<(u64, u128)> {
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

    fn legacy_id(file: &File) -> io::Result<(u64, u128)> {
        let mut info = ByHandleFileInformation::default();
        // SAFETY: the handle is open for as long as `file` is borrowed, and
        // `info` is a `BY_HANDLE_FILE_INFORMATION`.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut info) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let index = (u64::from(info.file_index_high) << 32) | u64::from(info.file_index_low);
        Ok((u64::from(info.volume_serial_number), u128::from(index)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_extended_identity_falls_back_to_legacy() {
        for code in [1, 50, 87, 120, 124] {
            let id = identity_with_fallback(Err(io::Error::from_raw_os_error(code)), || Ok((7, 9)));
            assert_eq!(id.unwrap(), (7, 9), "code {code}");
        }
    }

    #[test]
    fn supported_extended_identity_is_kept() {
        let id = identity_with_fallback(Ok((1, 2)), || panic!("legacy queried"));
        assert_eq!(id.unwrap(), (1, 2));
    }

    #[test]
    fn other_errors_propagate() {
        // ERROR_ACCESS_DENIED
        let id = identity_with_fallback(Err(io::Error::from_raw_os_error(5)), || {
            panic!("legacy queried")
        });
        assert_eq!(id.unwrap_err().raw_os_error(), Some(5));
    }

    #[test]
    fn fallback_failure_propagates() {
        let id = identity_with_fallback(Err(io::Error::from_raw_os_error(87)), || {
            Err(io::Error::from_raw_os_error(5))
        });
        assert_eq!(id.unwrap_err().raw_os_error(), Some(5));
    }
}
