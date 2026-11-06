//! The Windows side of the no-follow, handle-relative file calls the listing
//! and the file operations are built on: what `openat`, `fstatat`,
//! `fdopendir`, `mkdirat`, `unlinkat` and `renameat2` do on Unix.
//!
//! Every entry is opened with `NtCreateFile` relative to its directory's
//! handle (`RootDirectory`), as `openat` opens one, and with
//! `FILE_OPEN_REPARSE_POINT`, so a symlink or junction is opened itself rather
//! than followed. One opened as a directory fails as not a directory
//! ([`ERROR_DIRECTORY`]), and one opened as a file fails with
//! [`ERROR_STOPPED_ON_SYMLINK`], as `O_NOFOLLOW` fails with `ELOOP`. Renames
//! and deletes go through the handle of the entry found, with
//! `SetFileInformationByHandle`, the target named relative to its
//! directory's handle. Names are matched regardless of case, as Win32 matches
//! them.
//!
//! Every handle shares reading, writing and deleting, so holding a directory
//! open never stops another program renaming or removing it.

use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, PathBuf},
    ptr,
};

use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
            FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION,
            FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation, NtCreateFile,
            NtSetInformationFile,
        },
    },
    Win32::{
        Foundation::{
            ERROR_DIRECTORY, ERROR_NO_MORE_FILES, ERROR_STOPPED_ON_SYMLINK, HANDLE,
            INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING,
        },
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
            FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
            FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
            FILE_GENERIC_WRITE, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_NAME_INFO,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FILE_TRAVERSE, FileAttributeTagInfo, FileDispositionInfo, FileDispositionInfoEx,
            FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, FileNameInfo,
            GetFileInformationByHandle, GetFileInformationByHandleEx, OPEN_EXISTING, SYNCHRONIZE,
            SetFileInformationByHandle,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

use crate::listing::{Access, EntryKind, Identity, Stat};

/// `STATUS_FILE_IS_A_DIRECTORY`, which Win32 reports only as access denied.
const STATUS_FILE_IS_A_DIRECTORY: i32 = 0xC000_00BA_u32 as i32;
/// `STATUS_DELETE_PENDING`, which Win32 reports only as access denied.
const STATUS_DELETE_PENDING: i32 = 0xC000_0056_u32 as i32;

/// Whether a reparse tag makes its entry stand for another name (a symlink, a
/// junction), as `IsReparseTagNameSurrogate` tells. Other reparse points,
/// such as a cloud file's placeholder, are what they say they are.
fn is_name_surrogate(tag: u32) -> bool {
    tag & 0x2000_0000 != 0
}

/// What an entry is opened as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Open {
    /// A directory, for `access`.
    Dir(Access),
    /// An existing file, to read.
    Read,
    /// An existing file, to write, as it is (never truncated by the open).
    Write,
    /// A new file, to write; fails if anything is there.
    CreateNew,
    /// Anything, for its attributes alone.
    Attributes,
    /// Anything, to rename or delete.
    Delete { dir: bool },
}

impl Open {
    fn access(self) -> u32 {
        SYNCHRONIZE
            | FILE_READ_ATTRIBUTES
            | match self {
                Open::Dir(Access::Read) => FILE_LIST_DIRECTORY | FILE_TRAVERSE,
                Open::Dir(Access::Search) => FILE_TRAVERSE,
                Open::Read => FILE_GENERIC_READ,
                Open::Write | Open::CreateNew => FILE_GENERIC_WRITE,
                Open::Attributes => 0,
                Open::Delete { .. } => DELETE,
            }
    }

    fn options(self) -> u32 {
        FILE_SYNCHRONOUS_IO_NONALERT
            | FILE_OPEN_REPARSE_POINT
            | match self {
                Open::Dir(_) | Open::Delete { dir: true } => {
                    FILE_DIRECTORY_FILE | FILE_OPEN_FOR_BACKUP_INTENT
                }
                Open::Read | Open::Write | Open::CreateNew | Open::Delete { dir: false } => {
                    FILE_NON_DIRECTORY_FILE
                }
                Open::Attributes => FILE_OPEN_FOR_BACKUP_INTENT,
            }
    }

    fn disposition(self) -> u32 {
        match self {
            Open::CreateNew => FILE_CREATE,
            _ => FILE_OPEN,
        }
    }
}

/// The UTF-16 of `name`, which must be a single component.
fn wide_name(name: &OsStr) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = name.encode_wide().collect();
    if wide.is_empty() || wide.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a name"));
    }
    if wide
        .iter()
        .any(|&unit| unit == u16::from(b'\\') || unit == u16::from(b'/'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a separator in a name",
        ));
    }
    if wide.len() * 2 > usize::from(u16::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a name too long",
        ));
    }
    Ok(wide)
}

/// Opens `name` in directory `dir` as `how`, never following a symlink or
/// junction. For [`Open::CreateNew`], `dir_entry` makes a directory rather
/// than a file.
fn nt_open(dir: &File, name: &OsStr, how: Open, dir_entry: bool) -> io::Result<File> {
    let mut wide = wide_name(name)?;
    let byte_len = (wide.len() * 2) as u16;
    let unicode = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: dir.as_raw_handle() as HANDLE,
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let mut options = how.options();
    if how == Open::CreateNew && dir_entry {
        options = (options & !FILE_NON_DIRECTORY_FILE) | FILE_DIRECTORY_FILE;
    }
    let mut handle: HANDLE = ptr::null_mut();
    // SAFETY: an all-zero status block is valid; the call fills it.
    let mut status_block: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // SAFETY: every pointer is valid for the call: `attributes` names the
    // open directory handle and `unicode`, which borrows `wide`.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            how.access(),
            &attributes,
            &mut status_block,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            how.disposition(),
            options,
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(nt_error(status));
    }
    // SAFETY: `NtCreateFile` succeeded, so `handle` is a new handle that
    // nothing else owns.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

fn nt_error(status: i32) -> io::Error {
    if status == STATUS_FILE_IS_A_DIRECTORY {
        return io::Error::new(io::ErrorKind::IsADirectory, "a directory");
    }
    // Something deleted while a handle still holds it, as a folder another
    // process removed under one of ours: gone, as Unix sees it, not the
    // access denied Win32 makes of it.
    if status == STATUS_DELETE_PENDING {
        return io::Error::new(io::ErrorKind::NotFound, "deleted");
    }
    // SAFETY: always safe to call.
    io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
}

/// The attributes and reparse tag of open `file`.
fn attribute_tag(file: &File) -> io::Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `file` is open and `info` is valid for writes of its size.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            FileAttributeTagInfo,
            (&raw mut info).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

fn kind_of(info: &FILE_ATTRIBUTE_TAG_INFO) -> EntryKind {
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 && is_name_surrogate(info.ReparseTag)
    {
        EntryKind::Symlink
    } else if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Dir
    } else {
        EntryKind::File
    }
}

/// Opens `name` in `dir` as `how`. A symlink or junction there is refused:
/// as not a directory for a directory, else with
/// [`ERROR_STOPPED_ON_SYMLINK`]. Another kind of reparse point (a cloud
/// file's placeholder, say) is opened again without
/// `FILE_OPEN_REPARSE_POINT`, so its content is read as itself.
pub(crate) fn open_in(dir: &File, name: &OsStr, how: Open) -> io::Result<File> {
    let file = nt_open(dir, name, how, false)?;
    if matches!(
        how,
        Open::Attributes | Open::Delete { .. } | Open::CreateNew
    ) {
        return Ok(file);
    }
    let info = attribute_tag(&file)?;
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Ok(file);
    }
    if is_name_surrogate(info.ReparseTag) {
        let code = match how {
            Open::Dir(_) => ERROR_DIRECTORY,
            _ => ERROR_STOPPED_ON_SYMLINK,
        };
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    drop(file);
    reopen_following(dir, name, how)
}

/// [`open_in`] for a reparse point that isn't a name surrogate: opened
/// through the system's handling of it, then checked to still not be one.
fn reopen_following(dir: &File, name: &OsStr, how: Open) -> io::Result<File> {
    let path = final_path(dir)?.join(name);
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let flags = match how {
        Open::Dir(_) => FILE_FLAG_BACKUP_SEMANTICS,
        _ => FILE_ATTRIBUTE_NORMAL,
    };
    // SAFETY: `wide` is NUL-terminated.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            how.access(),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `CreateFileW` returned a new handle that nothing else owns.
    let file = File::from(unsafe { OwnedHandle::from_raw_handle(handle) });
    let info = attribute_tag(&file)?;
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 && is_name_surrogate(info.ReparseTag)
    {
        return Err(io::Error::from_raw_os_error(
            ERROR_STOPPED_ON_SYMLINK as i32,
        ));
    }
    Ok(file)
}

/// Opens directory `name` in `dir` without following a symlink or junction.
pub(crate) fn open_dir(dir: &File, name: &OsStr, access: Access) -> io::Result<File> {
    if name == ".." {
        return open_parent(dir, access);
    }
    open_in(dir, name, Open::Dir(access))
}

/// The directory above `dir`, which `NtCreateFile` can't open by `..`:
/// opened through `dir`'s final path.
fn open_parent(dir: &File, access: Access) -> io::Result<File> {
    let path = final_path(dir)?;
    let parent = path.parent().unwrap_or(&path);
    open_path(parent, access)
}

/// Opens the root of absolute `path`'s volume, its prefix and `\`, without
/// following a reparse point.
pub(crate) fn open_volume_root(path: &Path, access: Access) -> io::Result<File> {
    let mut root = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => root.push(prefix.as_os_str()),
            Component::RootDir => {
                root.push(Component::RootDir.as_os_str());
                break;
            }
            _ => break,
        }
    }
    if !root.has_root() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an absolute path",
        ));
    }
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            Open::Dir(access).access(),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `CreateFileW` returned a new handle that nothing else owns.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

/// Opens directory `path`, absolute, from its volume's root a component at a
/// time without following a symlink or junction; `..` is taken lexically.
pub(crate) fn open_path(path: &Path, access: Access) -> io::Result<File> {
    let mut names: Vec<&OsStr> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                names.pop();
            }
            Component::Normal(name) => names.push(name),
        }
    }
    let last = names.len();
    let mut dir = open_volume_root(path, if last == 0 { access } else { Access::Search })?;
    for (ix, name) in names.into_iter().enumerate() {
        let access = if ix + 1 == last {
            access
        } else {
            Access::Search
        };
        dir = open_in(&dir, name, Open::Dir(access))?;
    }
    Ok(dir)
}

/// The path open `file` has now, without the `\\?\` the system gives it.
pub(crate) fn final_path(file: &File) -> io::Result<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: `file` is open and `buffer` is valid for writes of its
        // length.
        let len = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle() as HANDLE,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        if len < buffer.len() {
            buffer.truncate(len);
            break;
        }
        buffer.resize(len + 1, 0);
    }
    let path = PathBuf::from(OsString::from_wide(&buffer));
    Ok(strip_verbatim(path))
}

/// `path` without a `\\?\` (or `\\?\UNC\`) prefix, where it has one.
pub(crate) fn strip_verbatim(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

/// The names in open directory `dir`, with their kinds.
pub(crate) fn read_dir(dir: &File) -> io::Result<Vec<(OsString, Option<EntryKind>)>> {
    // `FILE_ID_BOTH_DIR_INFO` holds 8-byte fields: the buffer is aligned for
    // them.
    let mut buffer = vec![0u64; 8 * 1024];
    let byte_len = (buffer.len() * 8) as u32;
    let mut entries = Vec::new();
    let mut class = FileIdBothDirectoryRestartInfo;
    loop {
        // SAFETY: `dir` is open and `buffer` is valid for writes of
        // `byte_len` bytes.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle() as HANDLE,
                class,
                buffer.as_mut_ptr().cast(),
                byte_len,
            )
        };
        if ok == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(entries);
            }
            return Err(err);
        }
        class = FileIdBothDirectoryInfo;
        let base = buffer.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        loop {
            // SAFETY: the call wrote whole entries into the buffer, chained by
            // their `NextEntryOffset`, each aligned for its fields.
            let entry = unsafe { &*base.add(offset).cast::<FILE_ID_BOTH_DIR_INFO>() };
            let name_len = entry.FileNameLength as usize / 2;
            // SAFETY: the name's `FileNameLength` bytes follow in the entry.
            let name = unsafe {
                std::slice::from_raw_parts((&raw const entry.FileName).cast::<u16>(), name_len)
            };
            let dot = u16::from(b'.');
            if name != [dot] && name != [dot, dot] {
                // For a reparse point, `EaSize` holds its tag.
                let kind = if entry.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                    && is_name_surrogate(entry.EaSize)
                {
                    EntryKind::Symlink
                } else if entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                };
                entries.push((OsString::from_wide(name), Some(kind)));
            }
            if entry.NextEntryOffset == 0 {
                break;
            }
            offset += entry.NextEntryOffset as usize;
        }
    }
}

/// The identity of open file `file`: its volume serial number and file
/// index.
pub(crate) fn identity(file: &File) -> io::Result<Identity> {
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` is open and `info` is valid for writes.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, info.as_mut_ptr()) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so it filled `info`.
    let info = unsafe { info.assume_init() };
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

/// The entry `name` in `dir`, without following a symlink or junction.
pub(crate) fn stat_at(dir: &File, name: &OsStr) -> io::Result<Stat> {
    let file = nt_open(dir, name, Open::Attributes, false)?;
    let kind = kind_of(&attribute_tag(&file)?);
    let id = identity(&file)?;
    let exact = file_name(&file)
        .map(|real| {
            let real = real.as_os_str().encode_wide();
            real.eq(name.encode_wide())
        })
        .unwrap_or(true);
    Ok(Stat { kind, id, exact })
}

/// The last component of the name open `file` has on its volume.
fn file_name(file: &File) -> io::Result<OsString> {
    // The header's length field, then the name.
    let mut buffer = vec![0u32; 1 + 1024];
    // SAFETY: `file` is open and `buffer` is valid for writes of its size.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            FileNameInfo,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 4) as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call wrote a `FILE_NAME_INFO` at the buffer's start.
    let info = unsafe { &*buffer.as_ptr().cast::<FILE_NAME_INFO>() };
    let len = (info.FileNameLength as usize / 2).min(buffer.len() * 2 - 2);
    // SAFETY: the name's `FileNameLength` bytes follow, within the buffer.
    let name = unsafe { std::slice::from_raw_parts((&raw const info.FileName).cast::<u16>(), len) };
    let last = name
        .rsplit(|&unit| unit == u16::from(b'\\'))
        .next()
        .unwrap_or(name);
    Ok(OsString::from_wide(last))
}

/// Makes directory `name` in `dir`.
pub(crate) fn mkdir_at(dir: &File, name: &OsStr) -> io::Result<()> {
    nt_open(dir, name, Open::CreateNew, true).map(drop)
}

/// Removes `name` from `dir`: an empty directory if `is_dir`, else a file
/// (or a symlink or junction, never what it leads to).
pub(crate) fn remove_at(dir: &File, name: &OsStr, is_dir: bool) -> io::Result<()> {
    let file = nt_open(dir, name, Open::Delete { dir: is_dir }, false)?;
    let handle = file.as_raw_handle() as HANDLE;
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    // SAFETY: `handle` is open and `info` is valid for reads of its size.
    let ok = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfoEx,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    // File systems without POSIX deletes (FAT, some network shares).
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: as above.
    let ok = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Renames `from` in `from_dir` to `to` in `to_dir`, failing with
/// [`io::ErrorKind::AlreadyExists`] if `to` exists, atomically.
pub(crate) fn rename_noreplace(
    from_dir: &File,
    from: &OsStr,
    to_dir: &File,
    to: &OsStr,
) -> io::Result<()> {
    let source = nt_open(from_dir, from, Open::Attributes, false);
    let is_dir = source.is_ok_and(|file| {
        attribute_tag(&file).is_ok_and(|info| info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0)
    });
    let file = nt_open(from_dir, from, Open::Delete { dir: is_dir }, false)?;
    // NTFS renames onto another hard link of the same file by doing
    // nothing, and says it succeeded, where `renameat2` refuses: refused
    // here too.
    if let Ok(found) = stat_at(to_dir, to)
        && found.id == identity(&file)?
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the name is another link to the same file",
        ));
    }
    let name = wide_name(to)?;
    // Win32's `SetFileInformationByHandle` refuses a rename relative to a
    // `RootDirectory` as an invalid parameter; the native call takes one.
    let header = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let size = (header + name.len() * 2).max(size_of::<FILE_RENAME_INFORMATION>());
    // Aligned for the header's handle.
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: the buffer holds the header and the name, aligned for the
    // header.
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = to_dir.as_raw_handle() as HANDLE;
        (*info).FileNameLength = (name.len() * 2) as u32;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            (&raw mut (*info).FileName).cast::<u16>(),
            name.len(),
        );
    }
    // SAFETY: an all-zero status block is valid; the call fills it.
    let mut status_block: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // SAFETY: `file` is open and the buffer is valid for reads of `size`.
    let status = unsafe {
        NtSetInformationFile(
            file.as_raw_handle() as HANDLE,
            &mut status_block,
            buffer.as_ptr().cast(),
            size as u32,
            FileRenameInformation,
        )
    };
    if status < 0 {
        return Err(nt_error(status));
    }
    Ok(())
}

/// Whether `err` is an open refused because a symlink or junction was there.
pub(crate) fn is_symlink_error(err: &io::Error) -> bool {
    err.raw_os_error() == Some(ERROR_STOPPED_ON_SYMLINK as i32)
}

/// Whether `err` is a directory open that found something else there.
pub(crate) fn is_not_dir_error(err: &io::Error) -> bool {
    err.raw_os_error() == Some(ERROR_DIRECTORY as i32) || err.kind() == io::ErrorKind::NotADirectory
}

/// Moves `path` to the Recycle Bin with the shell. Where it can't be recycled
/// (on a drive with no Recycle Bin), the shell asks before deleting it for
/// good, so nothing is deleted outright unasked.
pub(crate) fn recycle(path: &Path) -> io::Result<()> {
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
        FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW, SHFileOperationW,
    };
    // A list of paths, each NUL-terminated, ended by another NUL.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: ptr::null(),
        fFlags: (FOF_ALLOWUNDO
            | FOF_NOCONFIRMATION
            | FOF_WANTNUKEWARNING
            | FOF_SILENT
            | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: ptr::null_mut(),
        lpszProgressTitle: ptr::null(),
    };
    // SAFETY: `operation` is valid and `from` is double-NUL-terminated.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(io::Error::other(format!(
            "couldn't move {} to the Recycle Bin (error {result:#x})",
            path.display()
        )));
    }
    if operation.fAnyOperationsAborted != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "moving to the Recycle Bin was cancelled",
        ));
    }
    Ok(())
}
