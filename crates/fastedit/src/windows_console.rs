//! Tenmoku is a GUI app on Windows (`windows_subsystem = "windows"`), which
//! the system starts with no console: a launch from Explorer or the Start
//! menu opens no console window. Started from a terminal, it writes to that
//! terminal all the same: its output and logging, `--help` and errors
//! included, go to the console of the process that started it.

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE,
    STD_OUTPUT_HANDLE, SetStdHandle,
};

/// Attaches to the console of the process that started this one, if it has
/// one, for the standard output and error that weren't given another place
/// (a file or a pipe, which are kept). Call it first thing, before anything
/// writes.
pub(crate) fn attach_to_parent() {
    let unset = |which: STD_HANDLE| {
        // SAFETY: always safe to call.
        let handle = unsafe { GetStdHandle(which) };
        handle.is_null() || handle == INVALID_HANDLE_VALUE
    };
    let (out, err) = (unset(STD_OUTPUT_HANDLE), unset(STD_ERROR_HANDLE));
    if !out && !err {
        return;
    }
    // SAFETY: always safe to call; it fails if there's no parent console.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    let console = open_console_output();
    if console.is_null() || console == INVALID_HANDLE_VALUE {
        return;
    }
    // std looks the handles up as it writes, so this takes effect at once.
    // SAFETY: `console` is an open handle, kept for the process's life.
    unsafe {
        if out {
            SetStdHandle(STD_OUTPUT_HANDLE, console);
        }
        if err {
            SetStdHandle(STD_ERROR_HANDLE, console);
        }
    }
}

/// The attached console's screen buffer, opened for writing.
fn open_console_output() -> HANDLE {
    let name: Vec<u16> = "CONOUT$".encode_utf16().chain([0]).collect();
    // SAFETY: `name` is NUL-terminated.
    unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    }
}
