// SPDX-License-Identifier: Apache-2.0

//! How much physical RAM the computer has: what the plugin host sizes background jobs from.
//!
//! - **Linux and the other Unixes.** `MemTotal` in `/proc/meminfo`.
//! - **macOS.** `sysctl hw.memsize`.
//! - **Windows.** `GlobalMemoryStatusEx`'s `ullTotalPhys`.

/// This computer's physical RAM in bytes, or `None` if the platform won't say.
pub fn physical_ram_bytes() -> Option<u64> {
    physical_ram()
}

/// `MemTotal` of a `/proc/meminfo`'s `text`, in bytes.
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?;
    let kib: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kib.checked_mul(1024)
}

#[cfg(target_os = "macos")]
fn physical_ram() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: the name is a NUL-terminated string; `bytes` and `size` are live, `size` is
    // `bytes`'s size, and no new value is passed, so sysctl only writes `bytes`.
    let status = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut bytes).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && bytes > 0).then_some(bytes)
}

#[cfg(windows)]
fn physical_ram() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    // SAFETY: an all-zero MEMORYSTATUSEX is valid, and `dwLength` is set as the call requires
    // before it writes the struct through the live pointer.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: see above.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn physical_ram() -> Option<u64> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_total_is_read_in_bytes() {
        let text = "MemTotal:       16384000 kB\nMemFree:         1000 kB\n";
        assert_eq!(parse_meminfo(text), Some(16_384_000 * 1024));
        assert_eq!(parse_meminfo("MemFree: 1 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: lots kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: 12\n"), None);
        assert_eq!(parse_meminfo("MemTotal: 18446744073709551615 kB\n"), None);
    }

    /// Whatever platform this runs on, it has RAM and the platform says how much.
    #[test]
    fn this_computer_reports_its_ram() {
        let bytes = physical_ram_bytes().expect("physical RAM is readable");
        assert!(bytes >= 64 << 20, "{bytes}");
    }
}
