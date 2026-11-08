use std::time::Duration;

use super::parse;
use crate::*;

const TEXT: &str = "top\ndef a():\n    one\n    two\nend";
const FOLD: [fe_display::Fold; 1] = [fe_display::Fold { start: 1, end: 3 }];

/// A model of `TEXT` and a split of it with rows 1..=3 folded.
fn folded() -> (EditorModel, EditorModel) {
    let a = parse(TEXT);
    let mut b = a.split();
    b.fold_rows(1..=3);
    (a, b)
}

/// Catches `b` up by syncing, or by dropping a borrow of the buffer.
fn catch_up(b: &mut EditorModel, through_buffer: bool) {
    if through_buffer {
        drop(b.buffer_mut());
    } else {
        b.sync_display();
    }
}

#[test]
fn insert_then_delete_through_one_borrow_keeps_the_fold() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut guard = a.buffer_mut();
            let buffer: &mut Buffer = &mut guard;
            buffer.edit([(6..6, "X")], Origin::User);
            buffer.edit([(6..7, "")], Origin::User);
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), TEXT);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "through_buffer: {through_buffer}"
        );
    }
}

#[test]
fn transient_header_split_through_one_borrow_unfolds() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut guard = a.buffer_mut();
            let buffer: &mut Buffer = &mut guard;
            buffer.edit([(6..6, "\n")], Origin::User);
            buffer.edit([(6..7, "")], Origin::User);
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(b.display().folds(), [], "through_buffer: {through_buffer}");
    }
}

#[test]
fn grouped_overlapping_replacements_keep_the_fold_through_undo_and_redo() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut buffer = a.buffer_mut();
            buffer.start_transaction(Origin::User);
            buffer.edit([(6..7, "X")], Origin::User);
            buffer.edit([(6..7, "Y")], Origin::User);
            buffer.end_transaction();
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "edit, through_buffer: {through_buffer}"
        );
        a.undo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), TEXT);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "undo, through_buffer: {through_buffer}"
        );
        a.redo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), "top\ndeY a():\n    one\n    two\nend");
        assert_eq!(
            b.display().folds(),
            FOLD,
            "redo, through_buffer: {through_buffer}"
        );
    }
}

#[test]
fn grouped_insertions_through_one_borrow_keep_the_fold_through_undo_and_redo() {
    for through_buffer in [false, true] {
        let (mut a, mut b) = folded();
        {
            let mut guard = a.buffer_mut();
            let buffer: &mut Buffer = &mut guard;
            buffer.start_transaction(Origin::User);
            buffer.edit([(6..6, "X")], Origin::User);
            buffer.edit([(7..7, "Y")], Origin::User);
            buffer.end_transaction();
        }
        catch_up(&mut b, through_buffer);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "edit, through_buffer: {through_buffer}"
        );
        a.undo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(b.text().to_string(), TEXT);
        assert_eq!(
            b.display().folds(),
            FOLD,
            "undo, through_buffer: {through_buffer}"
        );
        a.redo().unwrap();
        catch_up(&mut b, through_buffer);
        assert_eq!(
            b.text().to_string(),
            "top\ndeXYf a():\n    one\n    two\nend"
        );
        assert_eq!(
            b.display().folds(),
            FOLD,
            "redo, through_buffer: {through_buffer}"
        );
    }
}

/// This thread's CPU cycles, counted as nanoseconds: the tests compare only
/// ratios of these, and `QueryThreadCycleTime` counts every cycle the thread
/// runs, where `GetThreadTimes` moves in 15.6 ms clock ticks.
#[cfg(windows)]
fn thread_cpu_time() -> Duration {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> isize;
        fn QueryThreadCycleTime(thread: isize, cycles: *mut u64) -> i32;
    }
    let mut cycles = 0;
    // SAFETY: the pseudo-handle names this thread, and `cycles` is a valid,
    // writable u64 for the call to fill in.
    let ok = unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
    assert_ne!(ok, 0, "QueryThreadCycleTime failed");
    Duration::from_nanos(cycles)
}

/// CPU time this thread has used. Unlike wall clock it doesn't grow while the thread is
/// descheduled, which is what heavy machine load does to a timing.
#[cfg(unix)]
fn thread_cpu_time() -> Duration {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: i32, out: *mut Timespec) -> i32;
    }
    #[cfg(target_os = "macos")]
    const CLOCK_THREAD_CPUTIME_ID: i32 = 16;
    #[cfg(not(target_os = "macos"))]
    const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
    let mut now = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable timespec for the call to fill in.
    let status = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut now) };
    assert_eq!(status, 0, "clock_gettime failed");
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

/// Records `batches` one-char batches through one `buffer_mut` borrow, syncs a folded
/// split, and returns the CPU time that took. Checks the folds and text length on every run.
fn time_recording(batches: usize) -> Duration {
    let (mut a, mut b) = folded();
    let started = thread_cpu_time();
    {
        let mut guard = a.buffer_mut();
        let buffer: &mut Buffer = &mut guard;
        for _ in 0..batches {
            let len = buffer.len();
            buffer.edit([(len..len, "!")], Origin::User);
        }
    }
    b.sync_display();
    let took = thread_cpu_time() - started;
    assert_eq!(b.display().folds(), FOLD);
    assert_eq!(b.text().len(), TEXT.len() + batches);
    took
}

#[test]
fn many_batches_recorded_together_take_linear_work() {
    // Timed in thread CPU time, not wall clock, so a loaded machine descheduling the thread
    // doesn't count; the sizes still alternate for several rounds and each is judged by its
    // fastest run, which sheds the cache and frequency noise load leaves in CPU time.
    // Linear work gives about 10x between the sizes and quadratic about 100x, so 50x separates
    // them with headroom for the noise that remains.
    const SMALL: usize = 1_000;
    const LARGE: usize = 10_000;
    const ROUNDS: usize = 5;
    let mut small = Vec::new();
    let mut large = Vec::new();
    for _ in 0..ROUNDS {
        small.push(time_recording(SMALL));
        large.push(time_recording(LARGE));
    }
    let fastest_small = small.iter().min().copied().unwrap();
    let fastest_large = large.iter().min().copied().unwrap();
    assert!(
        fastest_large < fastest_small * 50,
        "fastest {LARGE} batches took {fastest_large:?}, over 50x the fastest {SMALL} \
         batches at {fastest_small:?}; {SMALL}: {small:?}, {LARGE}: {large:?}"
    );
}
