// SPDX-License-Identifier: Apache-2.0

//! A thread's scheduling priority: lowered for background work, such as a plugin's jobs, so it
//! doesn't compete equally with what the user waits on.
//!
//! - **macOS.** QoS class utility (`pthread_set_qos_class_self_np`).
//! - **Linux.** `nice` 10, or the thread's own if it is higher already: on Linux a thread id given
//!   to `setpriority` names that thread alone, and raising `nice` needs no privilege.
//! - **Windows.** `THREAD_PRIORITY_BELOW_NORMAL`.
//! - **The other Unixes.** Left as it is: their `setpriority` takes the whole process.

/// The `nice` [`lower_thread_priority`] sets on Linux.
pub const BACKGROUND_NICE: i32 = 10;

/// Lowers the calling thread's priority for background work. Best effort: a failure leaves it as
/// it was, with a debug log.
pub fn lower_thread_priority() {
    if !lower() {
        log::debug!(
            "a background thread keeps its priority: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(target_os = "macos")]
fn lower() -> bool {
    // SAFETY: sets the calling thread's own QoS class; it takes no pointers.
    unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0) == 0 }
}

#[cfg(target_os = "linux")]
fn lower() -> bool {
    let Some(thread) = this_thread() else {
        return false;
    };
    if nice_of(thread).is_some_and(|nice| nice >= BACKGROUND_NICE) {
        return true;
    }
    // SAFETY: changes the `nice` of the thread `thread` names, the calling one.
    unsafe { libc::setpriority(libc::PRIO_PROCESS, thread, BACKGROUND_NICE) == 0 }
}

/// The calling thread's id, as `setpriority` takes it.
#[cfg(target_os = "linux")]
fn this_thread() -> Option<libc::id_t> {
    // SAFETY: `gettid` has no preconditions.
    libc::id_t::try_from(unsafe { libc::gettid() }).ok()
}

/// The `nice` of thread `thread`. `getpriority` returns -1 both as a value and as a failure, so
/// `errno` tells them apart.
#[cfg(target_os = "linux")]
fn nice_of(thread: libc::id_t) -> Option<i32> {
    // SAFETY: `__errno_location` is the calling thread's `errno`, valid while it runs;
    // `getpriority` takes no pointers.
    unsafe {
        *libc::__errno_location() = 0;
        let nice = libc::getpriority(libc::PRIO_PROCESS, thread);
        (nice != -1 || *libc::__errno_location() == 0).then_some(nice)
    }
}

#[cfg(windows)]
fn lower() -> bool {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: `GetCurrentThread` is a pseudo-handle to the calling thread, valid for this call.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) != 0 }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn lower() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `read` says of a new thread once it has called [`lower_thread_priority`].
    fn lowered<T: Send + 'static>(read: fn() -> T) -> T {
        std::thread::spawn(move || {
            lower_thread_priority();
            read()
        })
        .join()
        .unwrap()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_lowered_thread_runs_at_nice_10_or_more() {
        let own = this_thread().and_then(nice_of);
        let nice = lowered(|| this_thread().and_then(nice_of));
        // At least 10: a test runner already nicer than that keeps its own.
        assert!(nice.is_some_and(|nice| nice >= BACKGROUND_NICE), "{nice:?}");
        // The thread that started it is left as it was.
        assert_eq!(this_thread().and_then(nice_of), own);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_lowered_thread_runs_at_qos_utility() {
        let class = lowered(|| {
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // SAFETY: `class` and `relative` are live for the call, which writes only them.
            let status = unsafe {
                libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative)
            };
            assert_eq!(status, 0);
            class as u32
        });
        assert_eq!(class, libc::qos_class_t::QOS_CLASS_UTILITY as u32);
    }

    #[cfg(windows)]
    #[test]
    fn a_lowered_thread_runs_below_normal() {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        // SAFETY: as in `lower`, of the calling thread.
        let priority = lowered(|| unsafe { GetThreadPriority(GetCurrentThread()) });
        assert_eq!(priority, THREAD_PRIORITY_BELOW_NORMAL);
    }
}
