// SPDX-License-Identifier: Apache-2.0

//! A program and everything it starts, ended together.
//!
//! Killing a program's own process leaves what it started running: a grandchild that holds the
//! program's output open keeps a reader waiting, and outlives whoever started it. A
//! [`ProcessTree`] holds the whole tree.
//!
//! - **Unix.** The program leads a new process group, which what it starts joins. Ending the
//!   tree signals the group. A process that leaves the group (`setsid`, `setpgid`) leaves the
//!   tree too: a group is what Unix offers short of cgroups.
//! - **Windows.** The program is created suspended, assigned to a job object with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and only then resumed, so nothing it starts escapes
//!   the job: a job assigned after the start would miss what the program starts first. Its job
//!   doesn't allow breaking away. Ending the tree is `TerminateJobObject`.
//!
//! This is the only file in kiln-core with `unsafe` for processes; each block says why it is
//! sound.

use std::io;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};
use std::time::Duration;

/// How often [`ProcessTree::cancel_within`] looks whether the tree has ended.
#[cfg(unix)]
const POLL: Duration = Duration::from_millis(10);

/// A running program and what it started, killed together, and when this drops.
///
/// On Unix its program is reaped only right after the group's last signal, a SIGKILL: until
/// then the program, a zombie once it has exited, holds its pid and with it the group's id, so
/// no other process can take that id while this may still signal it. Nothing is signalled once
/// it is reaped. Use [`Self::try_wait`] and [`Self::wait`] for its status, never the [`Child`]'s
/// own.
pub struct ProcessTree {
    child: Child,
    /// The program's status once it is reaped.
    reaped: Option<ExitStatus>,
    #[cfg(unix)]
    group: unix::Group,
    #[cfg(windows)]
    job: windows::Job,
}

impl ProcessTree {
    /// How long [`Self::cancel`] waits after SIGTERM before SIGKILL on Unix.
    pub const GRACE: Duration = Duration::from_secs(2);

    /// Starts `command` as a tree of its own.
    ///
    /// On Unix this sets the command's process group. On Windows it sets the command's creation
    /// flags to `CREATE_SUSPENDED` and `CREATE_NO_WINDOW` (see
    /// [`without_console_window`](super::without_console_window)), replacing any set before:
    /// trees are for programs run from a GUI app, which have no console to show.
    pub fn spawn(command: &mut Command) -> io::Result<ProcessTree> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            let child = command.spawn()?;
            let group = unix::Group::led_by(&child);
            Ok(ProcessTree {
                child,
                reaped: None,
                group,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Made first, so a failure leaves nothing started.
            let job = windows::Job::new()?;
            command.creation_flags(windows::CREATE_SUSPENDED | windows::CREATE_NO_WINDOW);
            let mut child = command.spawn()?;
            // Until it is resumed it has run nothing, so it has started nothing either.
            if let Err(error) = job.adopt_and_resume(&child) {
                // Not assigned, or assigned and stuck suspended: either way it is ended here.
                job.kill();
                drop(child.kill());
                drop(child.wait());
                return Err(error);
            }
            Ok(ProcessTree {
                child,
                reaped: None,
                job,
            })
        }
    }

    /// The program's process id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Its stdin, if the command piped it and it hasn't been taken.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// Its stdout, if the command piped it and it hasn't been taken.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// Its stderr, if the command piped it and it hasn't been taken.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// The program's status if it has exited, without waiting. What it started may still run.
    ///
    /// On Unix the program isn't reaped by this: it stays a zombie holding its pid until the
    /// tree is ended ([`Self::kill`], [`Self::cancel`], or dropping this).
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.reaped {
            return Ok(Some(status));
        }
        #[cfg(unix)]
        {
            self.group.exited()
        }
        // The process handle this holds keeps its id from being reused, so std's reaping is
        // safe to use.
        #[cfg(windows)]
        {
            self.child.try_wait()
        }
    }

    /// Kills the whole tree at once (SIGKILL to the group on Unix, `TerminateJobObject` on
    /// Windows) and reaps the program. Returns its status: a signal, unless it had exited.
    ///
    /// Once the tree has been killed, this, [`Self::cancel`] and dropping the tree signal
    /// nothing more and return the same status.
    pub fn kill(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.reaped {
            return Ok(status);
        }
        #[cfg(unix)]
        {
            // The group's last signal: the program is reaped next, which frees its id.
            self.group.signal(unix::KILL);
            // A program that left its group isn't reached by that, and would be waited for
            // forever.
            drop(self.child.kill());
        }
        #[cfg(windows)]
        self.job.kill();
        let status = self.child.wait()?;
        self.reaped = Some(status);
        Ok(status)
    }

    /// Ends the tree, giving it [`Self::GRACE`] to end itself first, and reaps the program.
    ///
    /// On Unix the group gets SIGTERM, then SIGKILL if anything in it is still running 2 s
    /// later. On Windows this is [`Self::kill`]: a program with no console has no Ctrl-Break
    /// to be sent, so there is no gentler ending to wait for.
    pub fn cancel(&mut self) -> io::Result<ExitStatus> {
        self.cancel_within(Self::GRACE)
    }

    /// [`Self::cancel`], with `grace` in place of [`Self::GRACE`].
    ///
    /// On Unix the whole group has the grace, not only the program: when the program exits
    /// within it, what it started still has the rest of it. This returns as soon as nothing in
    /// the group runs. The program stays unreaped meanwhile, a zombie still in the group, so the
    /// group's members are listed to tell the rest from it: from `/proc` on Linux and with
    /// `proc_listpids` on macOS. Elsewhere, once the program has exited, the grace runs out.
    pub fn cancel_within(&mut self, grace: Duration) -> io::Result<ExitStatus> {
        #[cfg(windows)]
        {
            let _ = grace;
            self.kill()
        }
        #[cfg(unix)]
        {
            if let Some(status) = self.reaped {
                return Ok(status);
            }
            self.group.signal(unix::TERM);
            let ends = std::time::Instant::now().checked_add(grace);
            // Looked at without reaping, so the group's id stays the program's.
            while self.group.exited()?.is_none() || self.group.others_remain() {
                let now = std::time::Instant::now();
                if ends.is_some_and(|ends| now >= ends) {
                    break;
                }
                let left = ends.map_or(POLL, |ends| ends - now);
                std::thread::sleep(POLL.min(left));
            }
            self.kill()
        }
    }

    /// Waits for the program to exit, and kills what it left running. Returns its status.
    ///
    /// Its stdin, if piped and not taken, is closed first, as [`Child::wait`] closes it, so a
    /// program reading to the end of it can end.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.child.stdin.take());
        if self.reaped.is_none() {
            // On Unix without reaping, so the group's id stays the program's while what is
            // left of it is killed.
            #[cfg(unix)]
            self.group.wait_exited()?;
            #[cfg(windows)]
            self.child.wait()?;
        }
        self.kill()
    }
}

impl Drop for ProcessTree {
    /// Nothing of the tree outlives it: what is still running is killed, and the program is
    /// reaped.
    fn drop(&mut self) {
        drop(self.kill());
    }
}

impl std::fmt::Debug for ProcessTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessTree")
            .field("id", &self.child.id())
            .field("reaped", &self.reaped)
            .finish()
    }
}

#[cfg(unix)]
mod unix {
    use std::io;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, ExitStatus};

    pub(super) const TERM: libc::c_int = libc::SIGTERM;
    pub(super) const KILL: libc::c_int = libc::SIGKILL;

    /// The process group a tree's program leads: its id is the program's pid. It is only
    /// signalled while the program is unreaped, which keeps that id the program's.
    pub(super) struct Group {
        id: libc::pid_t,
    }

    impl Group {
        pub(super) fn led_by(child: &Child) -> Group {
            Group {
                // A pid always fits: `pid_t` is what the system gave it as.
                id: libc::pid_t::try_from(child.id()).unwrap_or(libc::pid_t::MAX),
            }
        }

        /// Sends `signal` to the group while its program is unreaped, so its id is still the
        /// program's.
        pub(super) fn signal(&self, signal: libc::c_int) {
            // SAFETY: `kill` takes no pointers; a negative pid names the group whose id it is,
            // which the unreaped program holds, so it is ours.
            if unsafe { libc::kill(-self.id, signal) } == -1 {
                let error = io::Error::last_os_error();
                // None in it: the program left it, and so did everything it started. No other
                // group can have its id meanwhile, since a group's id is its leader's pid.
                if error.raw_os_error() != Some(libc::ESRCH) {
                    log::warn!("signalling process group {}: {error}", self.id);
                }
            }
        }

        /// Whether anything but the program itself still runs in the group: true where that
        /// can't be told.
        pub(super) fn others_remain(&self) -> bool {
            others_in(self.id).unwrap_or(true)
        }

        /// The program's status if it has exited, leaving it unreaped.
        pub(super) fn exited(&self) -> io::Result<Option<ExitStatus>> {
            self.wait_for(libc::WNOHANG)
        }

        /// Waits for the program to exit, leaving it unreaped.
        pub(super) fn wait_exited(&self) -> io::Result<ExitStatus> {
            loop {
                if let Some(status) = self.wait_for(0)? {
                    return Ok(status);
                }
            }
        }

        fn wait_for(&self, flags: libc::c_int) -> io::Result<Option<ExitStatus>> {
            let Ok(id) = libc::id_t::try_from(self.id) else {
                return Err(io::Error::from(io::ErrorKind::InvalidInput));
            };
            loop {
                // SAFETY: `siginfo_t` is plain data, for which all zeroes is a valid value; a
                // zero `si_pid` after the call is how no exit is told apart.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                // SAFETY: `info` is valid for writes of a `siginfo_t`; `WNOWAIT` leaves the
                // program unreaped, so std's `Child` still owns reaping it.
                let waited = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        id,
                        &mut info,
                        libc::WEXITED | libc::WNOWAIT | flags,
                    )
                };
                if waited == -1 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error);
                }
                // SAFETY: `waitid` filled in the fields of a child's state change, or left
                // them zero.
                let (pid, status) = unsafe { (info.si_pid(), info.si_status()) };
                if pid == 0 {
                    return Ok(None);
                }
                // As `waitpid` encodes it, which `ExitStatus` reads. Anything else (macOS can
                // report a stop despite `WEXITED`) is no exit.
                let raw = match info.si_code {
                    libc::CLD_EXITED => (status & 0xff) << 8,
                    libc::CLD_KILLED => status & 0x7f,
                    libc::CLD_DUMPED => (status & 0x7f) | 0x80,
                    _ if flags & libc::WNOHANG != 0 => return Ok(None),
                    _ => {
                        // Not to spin on a stop reported again and again.
                        std::thread::sleep(super::POLL);
                        continue;
                    }
                };
                return Ok(Some(ExitStatus::from_raw(raw)));
            }
        }
    }

    /// Whether a process other than `group`'s leader still runs in `group`: none when that
    /// can't be told, which the caller counts as running.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn others_in(group: libc::pid_t) -> Option<bool> {
        others_in_proc(std::path::Path::new("/proc"), group)
    }

    /// [`others_in`] read from `proc`, a `/proc` tree: `Some(true)` when a member other than
    /// the leader runs, `Some(false)` when none is confirmed to, none when it can't be told.
    /// Only a process seen to have gone since it was listed is passed over unread: any other
    /// error is unknown, not absent (review F4). A zombie has ended only once every one of its
    /// threads has: its main thread can exit while the others run on (F5).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(super) fn others_in_proc(proc: &std::path::Path, group: libc::pid_t) -> Option<bool> {
        for entry in std::fs::read_dir(proc).ok()? {
            let entry = entry.ok()?;
            let Some(pid) = pid_named(&entry.file_name()) else {
                continue;
            };
            // The leader, unreaped: whether it has exited is `waitid`'s to say.
            if pid == group {
                continue;
            }
            let dir = entry.path();
            let Some((state, pgrp)) = stat_of(&dir.join("stat"))? else {
                continue;
            };
            if pgrp != group {
                continue;
            }
            if !ended(state) || threads_run(&dir.join("task"))? {
                return Some(true);
            }
        }
        Some(false)
    }

    /// The pid or tid a `/proc` entry's name is, if it is all decimal digits.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn pid_named(name: &std::ffi::OsStr) -> Option<libc::pid_t> {
        let name = name.to_str()?;
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        name.parse().ok()
    }

    /// Whether `error`, reading a process's entry, means it went away since it was listed.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn gone(error: &io::Error) -> bool {
        error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ESRCH)
    }

    /// Whether a `stat` state is a zombie's or a dead task's.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn ended(state: u8) -> bool {
        matches!(state, b'Z' | b'X')
    }

    /// The state and process group in the `stat` record at `path`: `Some(None)` when the
    /// process has gone, none when it can't be read or parsed.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn stat_of(path: &std::path::Path) -> Option<Option<(u8, libc::pid_t)>> {
        let stat = match std::fs::read(path) {
            Ok(stat) => stat,
            Err(error) if gone(&error) => return Some(None),
            Err(_) => return None,
        };
        // `pid (comm) state ppid pgrp ...`: `comm` may be any bytes, `)` and spaces too, so
        // the fields start after the last `)`.
        let close = stat.iter().rposition(|&byte| byte == b')')?;
        let mut fields = stat[close + 1..]
            .split(u8::is_ascii_whitespace)
            .filter(|field| !field.is_empty());
        let &[state] = fields.next()? else {
            return None;
        };
        let pgrp = std::str::from_utf8(fields.nth(1)?).ok()?.parse().ok()?;
        Some(Some((state, pgrp)))
    }

    /// Whether a thread under `task`, a process's `task` directory, still runs: false when the
    /// process has gone, none when it can't be told.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn threads_run(task: &std::path::Path) -> Option<bool> {
        let threads = match std::fs::read_dir(task) {
            Ok(threads) => threads,
            Err(error) if gone(&error) => return Some(false),
            Err(_) => return None,
        };
        for thread in threads {
            let thread = thread.ok()?;
            if pid_named(&thread.file_name()).is_none() {
                continue;
            }
            // A thread gone since the listing has ended.
            if let Some((state, _)) = stat_of(&thread.path().join("stat"))?
                && !ended(state)
            {
                return Some(true);
            }
        }
        Some(false)
    }

    /// `proc_listpids` lists the group's members; a failure is unknown, not an empty group
    /// (review F4).
    #[cfg(target_os = "macos")]
    fn others_in(group: libc::pid_t) -> Option<bool> {
        /// `libproc.h`'s filter for one process group's members.
        const PROC_PGRP_ONLY: u32 = 2;
        let typeinfo = u32::try_from(group).ok()?;
        let mut pids: Vec<libc::pid_t> = vec![0; 64];
        loop {
            let size = libc::c_int::try_from(pids.len() * size_of::<libc::pid_t>()).ok()?;
            // It returns 0 both for no members and on failure, which only errno tells apart.
            // SAFETY: `__error` gives this thread's errno, valid for writes.
            unsafe { *libc::__error() = 0 };
            // SAFETY: `pids` is valid for writes of `size` bytes, which the call fills no more
            // than.
            let filled = unsafe {
                libc::proc_listpids(PROC_PGRP_ONLY, typeinfo, pids.as_mut_ptr().cast(), size)
            };
            if filled == 0 {
                // SAFETY: as above, for reads.
                let errno = unsafe { *libc::__error() };
                return (errno == 0).then_some(false);
            }
            let filled = usize::try_from(filled).ok()? / size_of::<libc::pid_t>();
            // A full buffer may have been cut short: listed again in a larger one.
            if filled < pids.len() {
                return Some(pids[..filled].iter().any(|&pid| pid != 0 && pid != group));
            }
            pids.resize(pids.len() * 2, 0);
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    pub(super) use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

    /// A job object that kills everything in it when its last handle, this, closes.
    pub(super) struct Job(OwnedHandle);

    impl Job {
        pub(super) fn new() -> io::Result<Job> {
            // SAFETY: no security attributes and no name, both of which may be null.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `job` is a handle just made, which nothing else owns.
            let job = Job(unsafe { OwnedHandle::from_raw_handle(job) });
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // No `JOB_OBJECT_LIMIT_BREAKAWAY_OK`: nothing in it may start a process outside it.
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the job handle is open, and `limits` is valid for reads of the size given.
            let set = unsafe {
                SetInformationJobObject(
                    job.handle(),
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        /// Assigns `child`, created suspended, to the job, then resumes its one thread.
        pub(super) fn adopt_and_resume(&self, child: &Child) -> io::Result<()> {
            let process = child.as_raw_handle() as HANDLE;
            // SAFETY: both handles are open: the child's for as long as `child` lives.
            if unsafe { AssignProcessToJobObject(self.handle(), process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume(child.id())
        }

        /// Kills every process in the job.
        pub(super) fn kill(&self) {
            // SAFETY: the job handle is open.
            if unsafe { TerminateJobObject(self.handle(), 1) } == 0 {
                log::warn!("terminating a job object: {}", io::Error::last_os_error());
            }
        }

        fn handle(&self) -> HANDLE {
            self.0.as_raw_handle() as HANDLE
        }
    }

    /// Resumes the threads of process `pid`, created suspended: std keeps no handle to its
    /// thread, so they are found in a snapshot of the system's threads.
    fn resume(pid: u32) -> io::Result<()> {
        // SAFETY: takes no pointers; the process id is ignored for a thread snapshot.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a handle just made, which nothing else owns.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut resumed = 0;
        // SAFETY: the snapshot is open, and `entry` is valid for writes, its size set as the
        // call requires.
        let mut more = unsafe { Thread32First(snapshot.as_raw_handle() as HANDLE, &mut entry) };
        while more != 0 {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: takes no pointers.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: `thread` is open, and ours.
                let previous = unsafe { ResumeThread(thread) };
                let error = io::Error::last_os_error();
                // SAFETY: `thread` is ours and not used after this.
                unsafe { CloseHandle(thread) };
                if previous == u32::MAX {
                    return Err(error);
                }
                resumed += 1;
            }
            // SAFETY: as for `Thread32First`.
            more = unsafe { Thread32Next(snapshot.as_raw_handle() as HANDLE, &mut entry) };
        }
        if resumed == 0 {
            // The suspended process has no thread to resume.
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(())
    }
}

/// `others_in_proc` against a fake `/proc` tree; the group is 777 throughout.
#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    use std::path::Path;

    use super::unix::others_in_proc;

    const GROUP: libc::pid_t = 777;

    /// Writes `record` as the `stat` at `path` under `proc`, making its directories.
    fn stat(proc: &Path, path: &str, record: &[u8]) {
        let dir = proc.join(path);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stat"), record).unwrap();
    }

    #[test]
    fn a_member_with_a_non_utf8_name_still_runs() {
        let proc = tempfile::tempdir().unwrap();
        stat(
            proc.path(),
            "4242",
            b"4242 (w\xff) x) S 1 777 777 0 -1 4194304 0 0",
        );
        assert_eq!(others_in_proc(proc.path(), GROUP), Some(true));
    }

    #[test]
    fn an_unreadable_stat_is_unknown() {
        let proc = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proc.path().join("4242/stat")).unwrap();
        assert_eq!(others_in_proc(proc.path(), GROUP), None);
    }

    #[test]
    fn a_malformed_stat_is_unknown() {
        let proc = tempfile::tempdir().unwrap();
        stat(proc.path(), "4242", b"4242 (worker S 1 777 777 0");
        assert_eq!(others_in_proc(proc.path(), GROUP), None);
    }

    #[test]
    fn a_listing_that_fails_is_unknown() {
        let proc = tempfile::tempdir().unwrap();
        assert_eq!(others_in_proc(&proc.path().join("missing"), GROUP), None);
    }

    #[test]
    fn a_member_gone_since_the_listing_is_skipped() {
        let proc = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proc.path().join("4242")).unwrap();
        assert_eq!(others_in_proc(proc.path(), GROUP), Some(false));
    }

    #[test]
    fn a_zombie_with_a_live_thread_still_runs() {
        let proc = tempfile::tempdir().unwrap();
        stat(proc.path(), "4242", b"4242 (worker) Z 1 777 777 0");
        stat(
            proc.path(),
            "4242/task/4242",
            b"4242 (worker) Z 1 777 777 0",
        );
        stat(
            proc.path(),
            "4242/task/4243",
            b"4243 (worker) S 1 777 777 0",
        );
        assert_eq!(others_in_proc(proc.path(), GROUP), Some(true));
    }

    #[test]
    fn a_zombie_whose_threads_all_ended_has_ended() {
        let proc = tempfile::tempdir().unwrap();
        stat(proc.path(), "4242", b"4242 (worker) Z 1 777 777 0");
        stat(
            proc.path(),
            "4242/task/4242",
            b"4242 (worker) Z 1 777 777 0",
        );
        stat(
            proc.path(),
            "4242/task/4243",
            b"4243 (worker) Z 1 777 777 0",
        );
        assert_eq!(others_in_proc(proc.path(), GROUP), Some(false));
    }

    #[test]
    fn the_leader_and_other_groups_are_not_counted() {
        let proc = tempfile::tempdir().unwrap();
        stat(proc.path(), "777", b"777 (leader) S 1 777 777 0");
        stat(proc.path(), "5000", b"5000 (other) S 1 999 999 0");
        assert_eq!(others_in_proc(proc.path(), GROUP), Some(false));
    }
}
