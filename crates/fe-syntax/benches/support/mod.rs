//! Shared by the benches.

/// Bytes allocated from malloc and not yet freed, across the process. Rust's
/// allocator and tree-sitter's both use malloc, so a tree's size is the
/// difference across parsing it. Zero except on macOS and on Linux with glibc.
#[cfg(target_os = "macos")]
pub(crate) fn allocated() -> usize {
    #[repr(C)]
    #[derive(Default)]
    struct MallocStatistics {
        blocks_in_use: u32,
        size_in_use: usize,
        max_size_in_use: usize,
        size_allocated: usize,
    }
    unsafe extern "C" {
        fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStatistics);
    }
    let mut stats = MallocStatistics::default();
    // SAFETY: a null zone asks for the sum over all zones.
    unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
    stats.size_in_use
}

/// On glibc, `uordblks` counts chunks in use from the heap arenas and
/// `hblkhd` the large ones malloc gives their own mapping; a big tree has
/// both.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn allocated() -> usize {
    #[repr(C)]
    struct Mallinfo2 {
        arena: usize,
        ordblks: usize,
        smblks: usize,
        hblks: usize,
        hblkhd: usize,
        usmblks: usize,
        fsmblks: usize,
        uordblks: usize,
        fordblks: usize,
        keepcost: usize,
    }
    unsafe extern "C" {
        fn mallinfo2() -> Mallinfo2;
    }
    // SAFETY: mallinfo2 takes nothing and returns its struct by value
    // (glibc 2.33 and later).
    let info = unsafe { mallinfo2() };
    info.uordblks + info.hblkhd
}

#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
pub(crate) fn allocated() -> usize {
    0
}

/// How a parsed tree's memory compares with its source.
pub(crate) fn tree_memory(name: &str, source_bytes: usize, tree_bytes: usize) {
    const MB: f64 = 1024.0 * 1024.0;
    eprintln!(
        "{name}: {:.1} MB source, tree {:.0} MB ({:.1}× source)",
        source_bytes as f64 / MB,
        tree_bytes as f64 / MB,
        tree_bytes as f64 / source_bytes as f64,
    );
}
