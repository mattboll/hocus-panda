//! The allocations a test counts on its own thread (the rain's, the moving
//! wallpapers'): the test program's allocator, one for all its tests since
//! they are one program (#765). It counts nothing unless asked.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

/// Counts the allocations of the thread that asks, while it asks: the
/// loop runs on the test's thread, the clients on theirs.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNTED: Cell<u64> = const { Cell::new(0) };
}

// SAFETY: every call goes to the system's allocator; counting touches
// thread-locals of plain integers only, which never allocate.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNTING.try_with(|on| {
            if on.get() {
                let _ = COUNTED.try_with(|n| n.set(n.get() + 1));
            }
        });
        // SAFETY: as the caller promised us.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller promised us.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The allocations `run` makes on this thread.
pub fn allocations<T>(run: impl FnOnce() -> T) -> (T, u64) {
    COUNTED.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    let out = run();
    COUNTING.with(|on| on.set(false));
    (out, COUNTED.with(Cell::get))
}
