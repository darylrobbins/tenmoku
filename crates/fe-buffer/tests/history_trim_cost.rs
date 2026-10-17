use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use fe_buffer::{Buffer, Origin};

thread_local! {
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATED.try_with(|bytes| bytes.set(bytes.get() + layout.size()));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCATED.try_with(|bytes| bytes.set(bytes.get() + new_size));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn append(buffer: &mut Buffer) {
    let end = buffer.text().len();
    assert!(buffer.edit([(end..end, "x")], Origin::User).is_some());
}

/// Median bytes allocated by an append to a buffer whose history is full at
/// `limit` bytes.
fn median_allocation(limit: usize) -> usize {
    let mut buffer = Buffer::new("");
    buffer.set_history_limit(limit);
    let mut stable = 0;
    let mut held = 0;
    while stable < 100 {
        append(&mut buffer);
        let now = buffer.history_bytes(&Origin::User);
        if now == held && now <= limit && now + 1024 > limit {
            stable += 1;
        } else {
            stable = 0;
        }
        held = now;
    }
    let mut samples: Vec<usize> = (0..101)
        .map(|_| {
            let before = ALLOCATED.with(Cell::get);
            append(&mut buffer);
            ALLOCATED.with(Cell::get) - before
        })
        .collect();
    samples.sort_unstable();
    samples[50]
}

#[test]
fn an_edit_past_the_history_limit_allocates_no_more_with_a_longer_history() {
    let mut scratch = Buffer::new("");
    append(&mut scratch);
    let entry = scratch.history_bytes(&Origin::User);
    let small_median = median_allocation(500 * entry);
    let large_median = median_allocation(25_000 * entry);
    assert!(large_median <= 2 * small_median + 4096);
}
