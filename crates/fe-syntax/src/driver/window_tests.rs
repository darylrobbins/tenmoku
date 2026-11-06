use std::{
    ffi::c_void,
    ops::Range,
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicIsize, Ordering},
    time::Duration,
};

use fe_buffer::{Buffer, Origin};

use super::tests::{ManualSpawner, rust};
use crate::*;

/// The parse limit of Rust, the language of every buffer here.
fn limit() -> usize {
    rust().parse_limit()
}

/// Rust over its parse limit.
fn huge_rust() -> Buffer {
    Buffer::new(&"fn f() {}\n".repeat(limit() / 10 + 100_000))
}

/// A driver that may parse on the calling thread for as long as it likes.
fn driver(buffer: &Buffer) -> SyntaxDriver<Rc<()>> {
    SyntaxDriver::new(Syntax::new(rust(), buffer.current()), Duration::MAX)
}

/// Whether `visible` shows a keyword.
fn highlighted(driver: &SyntaxDriver<Rc<()>>, visible: Range<usize>) -> bool {
    driver
        .syntax()
        .highlights(visible)
        .iter()
        .any(|span| span.highlight.name() == "keyword")
}

/// About a screen of text at `offset`.
fn screen(offset: usize) -> Range<usize> {
    offset..offset + 3000
}

#[test]
fn window_parses_never_run_on_the_caller() {
    let mut buffer = huge_rust();
    let mut driver = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    let visible = screen(4_000_000);
    driver.sync(buffer.current(), visible.clone(), &mut spawner);
    assert_eq!(driver.syntax().window(), None);
    assert_eq!(spawner.pending(), 1);

    driver.finish(spawner.run(), &mut spawner);
    assert!(highlighted(&driver, visible.clone()));
    driver.sync(buffer.current(), visible.clone(), &mut spawner);
    assert_eq!(spawner.spawned, 1, "the window covers the viewport");

    // An edit leaves the window's tree stale; it's reparsed in the
    // background too, while the edited tree keeps highlighting.
    buffer.edit([(visible.start..visible.start, "// ")], Origin::User);
    driver.sync(buffer.current(), visible.clone(), &mut spawner);
    assert_eq!(spawner.pending(), 1);
    assert!(highlighted(&driver, visible));
}

#[test]
fn scrolling_coalesces_to_the_latest_viewport() {
    let buffer = huge_rust();
    let mut driver = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    let (first, second, latest) = (screen(1_000_000), screen(4_000_000), screen(8_000_000));
    driver.sync(buffer.current(), first.clone(), &mut spawner);
    driver.finish(spawner.run(), &mut spawner);

    for visible in [second.clone(), screen(6_000_000), latest.clone()] {
        driver.sync(buffer.current(), visible, &mut spawner);
    }
    assert_eq!(spawner.spawned, 2);
    assert!(highlighted(&driver, first.clone()), "the old window stays");

    // Landing starts the one follow-up, for the latest viewport, without
    // another sync.
    driver.finish(spawner.run(), &mut spawner);
    assert!(highlighted(&driver, second));
    assert_eq!((spawner.spawned, spawner.pending()), (3, 1));
    driver.finish(spawner.run(), &mut spawner);
    assert!(highlighted(&driver, latest.clone()));
    assert!(!highlighted(&driver, first));
    assert_eq!(spawner.pending(), 0);
    driver.sync(buffer.current(), latest, &mut spawner);
    assert_eq!(spawner.spawned, 3);
}

#[test]
fn a_full_parse_and_a_window_never_run_at_once() {
    let mut buffer = Buffer::new(&"fn f() {}\n".repeat(20_000));
    let mut driver = SyntaxDriver::new(Syntax::new(rust(), buffer.current()), Duration::ZERO);
    let mut spawner = ManualSpawner::default();
    driver.sync(buffer.current(), 0..0, &mut spawner);
    assert_eq!(spawner.pending(), 1);

    // Pasting pushes the file over the limit while the full parse runs.
    buffer.edit([(0..0, "fn f() {}\n".repeat(limit() / 10))], Origin::User);
    driver.sync(buffer.current(), screen(0), &mut spawner);
    assert_eq!(spawner.spawned, 1);
    // The full parse is no use now; the window starts as it lands.
    driver.finish(spawner.run(), &mut spawner);
    assert_eq!(spawner.spawned, 2);
    driver.finish(spawner.run(), &mut spawner);
    assert!(highlighted(&driver, screen(0)));
}

/// Set in the process [`window_memory_stays_bounded`] measures in.
const MEASURING: &str = "FE_SYNTAX_COUNT_TREE_SITTER";

#[test]
fn window_memory_stays_bounded() {
    // tree-sitter's allocator can only be replaced before anything uses
    // tree-sitter, so the measurement runs in a fresh process of this test
    // binary, running only this test.
    if std::env::var_os(MEASURING).is_none() {
        let name = "driver::window_tests::window_memory_stays_bounded";
        let child = Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--test-threads=1", "--nocapture"])
            .env(MEASURING, "1")
            .output()
            .unwrap();
        let output =
            String::from_utf8_lossy(&child.stdout) + String::from_utf8_lossy(&child.stderr);
        assert!(child.status.success(), "{output}");
        assert!(output.contains("1 passed"), "{output}");
        return;
    }
    let live = count_tree_sitter_bytes();
    let buffer = huge_rust();
    let len = buffer.current().len();
    let mut driver = driver(&buffer);
    let mut spawner = ManualSpawner::default();
    driver.sync(buffer.current(), screen(0), &mut spawner);
    let before = live();
    driver.finish(spawner.run(), &mut spawner);
    let one = live() - before;
    assert!(one > 0, "a window's tree is counted");

    // Enough jumps that keeping every window would pass the bound.
    let jumps = 64;
    let mut peak = one;
    for jump in 1..=jumps {
        let visible = screen(jump * 7_919 * 1_013 % (len - 3000));
        driver.sync(buffer.current(), visible.clone(), &mut spawner);
        driver.finish(spawner.run(), &mut spawner);
        assert!(highlighted(&driver, visible));
        peak = peak.max(live() - before);
    }
    assert!(jumps as isize * one > len as isize / 2);
    // Only the latest window is kept, its predecessor freed as it lands.
    assert!(peak <= 2 * one, "{peak} bytes live, {one} for one window");
    assert!(peak < len as isize / 2);
}

/// Has tree-sitter count what it allocates, and returns a reader of the bytes
/// it has live: allocated and not yet freed. Call it before anything uses
/// tree-sitter.
fn count_tree_sitter_bytes() -> impl Fn() -> isize {
    static LIVE: AtomicIsize = AtomicIsize::new(0);
    unsafe extern "C" {
        fn malloc(size: usize) -> *mut c_void;
        fn calloc(count: usize, size: usize) -> *mut c_void;
        fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
        fn free(ptr: *mut c_void);
        #[cfg_attr(target_os = "macos", link_name = "malloc_size")]
        #[cfg_attr(target_os = "linux", link_name = "malloc_usable_size")]
        #[cfg_attr(windows, link_name = "_msize")]
        fn usable_size(ptr: *const c_void) -> usize;
    }
    fn size(ptr: *const c_void) -> isize {
        if ptr.is_null() {
            0
        } else {
            unsafe { usable_size(ptr) as isize }
        }
    }
    fn count(delta: isize) {
        LIVE.fetch_add(delta, Ordering::Relaxed);
    }
    unsafe extern "C" fn counted_malloc(len: usize) -> *mut c_void {
        let ptr = unsafe { malloc(len) };
        count(size(ptr));
        ptr
    }
    unsafe extern "C" fn counted_calloc(n: usize, len: usize) -> *mut c_void {
        let ptr = unsafe { calloc(n, len) };
        count(size(ptr));
        ptr
    }
    unsafe extern "C" fn counted_realloc(ptr: *mut c_void, len: usize) -> *mut c_void {
        let old = size(ptr);
        let ptr = unsafe { realloc(ptr, len) };
        count(size(ptr) - old);
        ptr
    }
    unsafe extern "C" fn counted_free(ptr: *mut c_void) {
        count(-size(ptr));
        unsafe { free(ptr) }
    }

    // SAFETY: called first in a process that runs only this test, so no
    // tree-sitter object exists yet and no other thread uses tree-sitter.
    unsafe {
        tree_sitter::set_allocator(Some(tree_sitter::Allocator {
            malloc: counted_malloc,
            calloc: counted_calloc,
            realloc: counted_realloc,
            free: counted_free,
        }));
    }
    || LIVE.load(Ordering::Relaxed)
}
