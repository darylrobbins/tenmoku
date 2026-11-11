// SPDX-License-Identifier: Apache-2.0

//! The Windows calls behind [`super::fs`]: who a process's files belong to, a directory made
//! with a private security descriptor, who a file's owner and access list let write it, and a
//! handle's file identity. The `unsafe` these need stays here, behind safe functions.

use std::{
    ffi::c_void,
    fs::File,
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GENERIC_ALL, GENERIC_WRITE, HANDLE, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, AddAccessAllowedAceEx,
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetAce, GetLengthSid,
        GetTokenInformation, INHERIT_ONLY_ACE, InitializeAcl, InitializeSecurityDescriptor,
        IsValidSid, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, SetSecurityDescriptorControl,
        SetSecurityDescriptorDacl, SetSecurityDescriptorOwner, TOKEN_INFORMATION_CLASS,
        TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, DELETE, FILE_ALL_ACCESS, FILE_APPEND_DATA,
        FILE_DELETE_CHILD, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA,
        GetFileInformationByHandle, WRITE_DAC, WRITE_OWNER,
    },
    System::{
        SystemServices::{
            ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, SECURITY_DESCRIPTOR_REVISION,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

/// A security identifier, as its bytes: two SIDs are the same when their bytes are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sid(Vec<u8>);

/// `NT AUTHORITY\SYSTEM`, S-1-5-18.
pub(crate) const SYSTEM: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];

/// `BUILTIN\Administrators`, S-1-5-32-544.
pub(crate) const ADMINISTRATORS: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 2, 0, 0];

/// Every right that changes a file or directory, its entries, or who may: what an access entry
/// must not give anyone untrusted.
const WRITE_RIGHTS: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | FILE_DELETE_CHILD
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_WRITE
    | GENERIC_ALL;

impl Sid {
    /// Whether this is `bytes`, one of the constants above.
    pub(crate) fn is(&self, bytes: &[u8]) -> bool {
        self.0 == bytes
    }

    /// A copy of the SID at `psid`, or `None` if that is not a valid one.
    ///
    /// # Safety
    ///
    /// `psid` is null or points to memory readable for as long as the SID it holds claims.
    unsafe fn copy(psid: PSID) -> Option<Sid> {
        // SAFETY: the caller's promise; IsValidSid reads the header before the length is used.
        if psid.is_null() || unsafe { IsValidSid(psid) } == 0 {
            return None;
        }
        // SAFETY: a valid SID, so its length is that of the bytes it spans.
        let len = unsafe { GetLengthSid(psid) } as usize;
        // SAFETY: as above, `len` bytes from `psid` are the SID.
        Some(Sid(unsafe {
            std::slice::from_raw_parts(psid as *const u8, len)
        }
        .to_vec()))
    }

    /// A copy aligned as the API reads a SID (its sub-authorities are u32s), whose pointer
    /// [`psid`] gives.
    fn aligned(&self) -> Vec<u32> {
        let mut words = vec![0u32; self.0.len().div_ceil(4)];
        for (word, bytes) in words.iter_mut().zip(self.0.chunks(4)) {
            let mut four = [0u8; 4];
            four[..bytes.len()].copy_from_slice(bytes);
            *word = u32::from_ne_bytes(four);
        }
        words
    }
}

/// The pointer the API takes for a SID [`Sid::aligned`] gave, valid while `words` is.
fn psid(words: &[u32]) -> PSID {
    words.as_ptr().cast_mut().cast()
}

/// An open handle, closed when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened for us and is closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

/// Memory the system allocated for us with `LocalAlloc`, freed when dropped.
struct Local(*mut c_void);

impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the system allocated it for us, and nothing reads it after this.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// The SID in this process's token's `class` information: `TokenUser` or `TokenOwner`.
fn token_sid(class: TOKEN_INFORMATION_CLASS) -> io::Result<Sid> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess's pseudo-handle is always valid, and `token` is valid for writes.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle(token);
    let mut needed = 0u32;
    // SAFETY: a null buffer of length 0 only asks for the length, which `needed` receives.
    unsafe { GetTokenInformation(token.0, class, ptr::null_mut(), 0, &mut needed) };
    // u64s, so the pointers at its start are aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    let len = (buffer.len() * 8) as u32;
    // SAFETY: `buffer` is valid for `len` bytes of writes.
    if unsafe { GetTokenInformation(token.0, class, buffer.as_mut_ptr().cast(), len, &mut needed) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so the buffer holds the TOKEN_USER or TOKEN_OWNER asked
    // for, whose SID pointer is into the same buffer.
    let psid = unsafe {
        if class == TokenUser {
            (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid
        } else {
            (*buffer.as_ptr().cast::<TOKEN_OWNER>()).Owner
        }
    };
    // SAFETY: the pointer is into `buffer`, alive until the end of this function.
    unsafe { Sid::copy(psid) }.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
}

/// The user this process runs as: its token's user.
pub(crate) fn current_user() -> io::Result<Sid> {
    token_sid(TokenUser)
}

/// Who owns what this process creates without saying: its token's default owner, the user
/// or, in an elevated process, often `Administrators`.
pub(crate) fn default_owner() -> io::Result<Sid> {
    token_sid(TokenOwner)
}

/// Who an open file's security descriptor makes its owner and lets write it.
pub(crate) struct Security {
    pub(crate) owner: Option<Sid>,
    /// Those an access entry that applies to the file itself grants a write right. `None`
    /// when anyone may: a null access list, or an entry of a kind not read here.
    pub(crate) writers: Option<Vec<Sid>>,
}

/// `file`'s owner and writers. `file` was opened with `READ_CONTROL`, as any read is.
pub(crate) fn security(file: &File) -> io::Result<Security> {
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: an open handle, and pointers valid for writes; the descriptor the call
    // allocates is freed by `Local`, and `owner` and `dacl` point into it.
    let error = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    let _descriptor = Local(descriptor);
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    // SAFETY: `owner` is null or a SID in the descriptor, alive until we return.
    let owner = unsafe { Sid::copy(owner) };
    if dacl.is_null() {
        return Ok(Security {
            owner,
            writers: None,
        });
    }
    // SAFETY: a non-null access list in the descriptor, whose header says how many entries.
    let count = unsafe { (*dacl).AceCount };
    let mut writers = Vec::new();
    for index in 0..u32::from(count) {
        let mut ace: *mut c_void = ptr::null_mut();
        // SAFETY: `index` is below the list's count, and `ace` is valid for writes.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: GetAce gave a pointer to an entry in the list, which starts with its header.
        let header = unsafe { *ace.cast::<ACE_HEADER>() };
        if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 {
            // For what is created inside, which is checked itself; not for this file.
            continue;
        }
        match u32::from(header.AceType) {
            ACCESS_DENIED_ACE_TYPE => {}
            ACCESS_ALLOWED_ACE_TYPE => {
                let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
                // SAFETY: an allowed entry: a header, its mask, then its SID within its size.
                let mask = unsafe { (*allowed).Mask };
                if mask & WRITE_RIGHTS != 0 {
                    // SAFETY: as above, the SID starts at `SidStart`, inside the entry.
                    let sid = unsafe { Sid::copy(ptr::addr_of_mut!((*allowed).SidStart).cast()) };
                    match sid {
                        Some(sid) => writers.push(sid),
                        None => {
                            return Ok(Security {
                                owner,
                                writers: None,
                            });
                        }
                    }
                }
            }
            // Object, callback and other allowing kinds: not read here, so not trusted.
            _ => {
                return Ok(Security {
                    owner,
                    writers: None,
                });
            }
        }
    }
    Ok(Security {
        owner,
        writers: Some(writers),
    })
}

/// Creates directory `dir`, whose parent exists, owned by the current user, with an access
/// list of its own that gives full control to that user and `SYSTEM` alone. The list is
/// protected, so nothing is inherited from the parent, and its entries are inherited by what
/// is created inside.
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    let user = current_user()?;
    let system = Sid(SYSTEM.to_vec());
    // The list: its header, then an allowed entry per SID, whose SID takes the place of the
    // entry's `SidStart`.
    let entry = |sid: &Sid| size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid.0.len();
    let len = size_of::<ACL>() + entry(&user) + entry(&system);
    // u32s: an access list is aligned to them.
    let mut list = vec![0u32; len.div_ceil(4)];
    let acl = list.as_mut_ptr().cast::<ACL>();
    let len = (list.len() * 4) as u32;
    // SAFETY: `acl` is valid for `len` bytes of writes, aligned for an ACL.
    if unsafe { InitializeAcl(acl, len, ACL_REVISION) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let (user, system) = (user.aligned(), system.aligned());
    for sid in [&user, &system] {
        // SAFETY: an initialized list with room for this entry, and a valid SID.
        if unsafe {
            AddAccessAllowedAceEx(
                acl,
                ACL_REVISION,
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                FILE_ALL_ACCESS,
                psid(sid),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: SECURITY_DESCRIPTOR is plain data, which InitializeSecurityDescriptor fills.
    let mut descriptor: SECURITY_DESCRIPTOR = unsafe { std::mem::zeroed() };
    let pointer: PSECURITY_DESCRIPTOR = ptr::addr_of_mut!(descriptor).cast();
    // SAFETY: `pointer` is to a descriptor of ours; the owner and list it points to (`user`
    // and `list`) outlive the CreateDirectoryW call that reads them.
    let built = unsafe {
        InitializeSecurityDescriptor(pointer, SECURITY_DESCRIPTOR_REVISION) != 0
            && SetSecurityDescriptorOwner(pointer, psid(&user), 0) != 0
            && SetSecurityDescriptorDacl(pointer, 1, acl, 0) != 0
            && SetSecurityDescriptorControl(pointer, SE_DACL_PROTECTED, SE_DACL_PROTECTED) != 0
    };
    if !built {
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: pointer,
        bInheritHandle: 0,
    };
    let wide: Vec<u16> = long_form(dir)?
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: a NUL-terminated path, and attributes whose descriptor is alive.
    if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// `dir` as CreateDirectoryW takes it at any length: absolute, and `\\?\`-prefixed when
/// longer than its limit (`MAX_PATH` less room for an 8.3 name), as std does for its own calls.
fn long_form(dir: &Path) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let absolute = std::path::absolute(dir)?;
    let wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let [slash, question, dot] = b"\\?.".map(u16::from);
    let verbatim = wide.starts_with(&[slash, slash, question, slash])
        || wide.starts_with(&[slash, slash, dot, slash]);
    if wide.len() < 248 || verbatim {
        return Ok(absolute);
    }
    // `\\server\share\x` is `\\?\UNC\server\share\x`; `C:\x` is `\\?\C:\x`.
    let (prefix, rest) = match wide.strip_prefix(&[slash, slash]) {
        Some(_) => (r"\\?\UNC", &wide[1..]),
        None => (r"\\?\", &wide[..]),
    };
    let mut long: Vec<u16> = prefix.encode_utf16().collect();
    long.extend_from_slice(rest);
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&long)))
}

/// The volume serial number and file index of open file `file`.
pub(crate) fn file_identity(file: &File) -> io::Result<(u64, u64)> {
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` is an open handle and `info` is valid for writes.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so it filled `info`.
    let info = unsafe { info.assume_init() };
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}
