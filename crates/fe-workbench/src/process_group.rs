//! A command run with what it starts, stopped together: on Unix a process
//! group the command leads, signalled as one; on Windows, which has no
//! signals, the command's process tree, terminated from its leaves up.
//!
//! The leader is never reaped before its group is stopped, so its id (the
//! group's, on Unix) can't pass to another process meanwhile; on Windows the
//! `Child` holds the leader's handle, which keeps its id from being reused.

use std::process::{Child, Command, ExitStatus};

/// What names a group: its leader's id.
#[cfg(unix)]
pub(crate) type GroupId = libc::pid_t;
#[cfg(windows)]
pub(crate) type GroupId = u32;

/// Makes `command` lead a group of its own, with no console window on
/// Windows.
pub(crate) fn lead_group(command: &mut Command) {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(command, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        command,
        windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
}

/// `command` with no console window on Windows, where a GUI app's console
/// child would otherwise get one of its own: for a command that isn't a
/// group's ([`lead_group`] does this too).
pub(crate) fn no_window(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        command,
        windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
    command
}

/// The group `child`, which leads it, names.
pub(crate) fn group_of(child: &Child) -> Option<GroupId> {
    GroupId::try_from(child.id()).ok()
}

/// The signal that ended a process, if one did: never, on Windows.
pub(crate) fn signal_of(status: &ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    return std::os::unix::process::ExitStatusExt::signal(status);
    #[cfg(windows)]
    {
        let _ = status;
        None
    }
}

/// Whether `child` has exited, without reaping it.
#[cfg(unix)]
pub(crate) fn exited(child: &Child) -> bool {
    let pid: libc::id_t = child.id();
    // SAFETY: a zeroed siginfo_t is valid, and waitid(2) only writes it.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let options = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
    // SAFETY: as above; WNOWAIT leaves the child to be reaped by `wait`.
    let waited = unsafe { libc::waitid(libc::P_PID, pid, &mut info, options) };
    // With WNOHANG, no child that has exited leaves si_pid 0.
    // SAFETY: waitid filled in the pid, or left it 0.
    waited == 0 && unsafe { info.si_pid() } != 0
}

/// Whether `child` has exited. Waiting on its handle reaps nothing.
#[cfg(windows)]
pub(crate) fn exited(child: &Child) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    // SAFETY: the child's handle is open while `child` is.
    unsafe { WaitForSingleObject(child.as_raw_handle(), 0) == WAIT_OBJECT_0 }
}

/// Asks the group to end: `SIGTERM` on Unix. Windows has no such request
/// for a console program without a console, so its tree is terminated.
pub(crate) fn terminate(group: GroupId) {
    #[cfg(unix)]
    // SAFETY: kill(2) on a group whose leader isn't reaped, as the caller
    // ensures, so it keeps its id.
    unsafe {
        libc::kill(-group, libc::SIGTERM)
    };
    #[cfg(windows)]
    win::kill_tree(group);
}

/// Ends the group for good: `SIGKILL` on Unix, the tree terminated on
/// Windows.
pub(crate) fn kill(group: GroupId) {
    #[cfg(unix)]
    // SAFETY: as in `terminate`.
    unsafe {
        libc::kill(-group, libc::SIGKILL)
    };
    #[cfg(windows)]
    win::kill_tree(group);
}

#[cfg(windows)]
mod win {
    use std::collections::{HashMap, HashSet};

    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };

    /// Terminates process `root` and every process it started, directly or
    /// not, the deepest first, so none is left to start another meanwhile.
    pub(super) fn kill_tree(root: u32) {
        let mut tree = vec![root];
        let children = children_by_parent();
        let mut seen = HashSet::from([root]);
        let mut ix = 0;
        while ix < tree.len() {
            let parent = tree[ix];
            for &child in children.get(&parent).into_iter().flatten() {
                // A process whose parent exited keeps the parent's id, which
                // another process may have since: one created before its
                // supposed parent isn't its child.
                if seen.insert(child) && created_after(child, parent) {
                    tree.push(child);
                }
            }
            ix += 1;
        }
        for &pid in tree.iter().rev() {
            terminate(pid);
        }
    }

    /// Every running process's id, by its parent's.
    fn children_by_parent() -> HashMap<u32, Vec<u32>> {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        // SAFETY: always safe to call.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return children;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY: `snapshot` is open and `entry` is valid with its size set.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while more {
            children
                .entry(entry.th32ParentProcessID)
                .or_default()
                .push(entry.th32ProcessID);
            // SAFETY: as above.
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: `snapshot` is ours and not used after this.
        unsafe { CloseHandle(snapshot) };
        children
    }

    /// When process `pid` was created, as a `FILETIME`'s count.
    fn created(pid: u32) -> Option<u64> {
        // SAFETY: always safe to call.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut times = [FILETIME::default(); 4];
        let [creation, exit, kernel, user] = &mut times;
        // SAFETY: `process` is open and each time is valid for writes.
        let ok = unsafe { GetProcessTimes(process, creation, exit, kernel, user) } != 0;
        // SAFETY: `process` is ours and not used after this.
        unsafe { CloseHandle(process) };
        ok.then(|| (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }

    fn created_after(child: u32, parent: u32) -> bool {
        match (created(child), created(parent)) {
            (Some(child), Some(parent)) => child >= parent,
            // A parent that has exited can't be told apart: its child is
            // taken as one.
            (Some(_), None) => true,
            (None, _) => false,
        }
    }

    fn terminate(pid: u32) {
        // SAFETY: always safe to call.
        let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if process.is_null() {
            return;
        }
        // SAFETY: `process` is open, and ours to close.
        unsafe {
            TerminateProcess(process, 1);
            CloseHandle(process);
        }
    }
}
