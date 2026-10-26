//! Counts what the compositor allocates on its loop, for tests' budgets
//! (#88). [`Counting`] wraps the system's allocator; a program installs it
//! as its global allocator, and only what a thread allocates while it
//! does the compositor's work ([`counting`]) is counted, on that thread:
//! test clients in threads of the same process are left out.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::atomic::{AtomicBool, Ordering},
};

/// The system's allocator, counting.
#[derive(Debug, Default, Clone, Copy)]
pub struct Counting;

/// What a thread allocated while counting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Allocations {
    /// Allocations made, a reallocation counted as one.
    pub count: u64,
    /// Bytes asked for.
    pub bytes: u64,
    /// Bytes allocated less those freed: what it holds, as far as its own
    /// frees tell.
    pub held: i64,
}

impl Allocations {
    /// What was allocated since `before`.
    pub fn since(&self, before: &Allocations) -> Allocations {
        Allocations {
            count: self.count - before.count,
            bytes: self.bytes - before.bytes,
            held: self.held - before.held,
        }
    }
}

static INSTALLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    // Const-initialised cells: reading them never allocates.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<Allocations> = const {
        Cell::new(Allocations { count: 0, bytes: 0, held: 0 })
    };
}

/// Whether [`Counting`] is the program's allocator.
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// What this thread allocated while counting, since it started.
pub fn counted() -> Allocations {
    COUNTS.try_with(Cell::get).unwrap_or_default()
}

/// Runs `work` with this thread's allocations counted.
pub fn counting<R>(work: impl FnOnce() -> R) -> R {
    let before = COUNTING.try_with(|c| c.replace(true)).unwrap_or(false);
    let result = work();
    let _ = COUNTING.try_with(|c| c.set(before));
    result
}

fn note(allocated: usize, freed: usize) {
    // Never panics nor allocates: a thread going away has no cells left.
    let _ = COUNTING.try_with(|on| {
        if !on.get() {
            return;
        }
        let _ = COUNTS.try_with(|counts| {
            let mut c = counts.get();
            if allocated > 0 {
                c.count += 1;
                c.bytes += allocated as u64;
            }
            c.held += allocated as i64 - freed as i64;
            counts.set(c);
        });
    });
}

// SAFETY: every call goes to the system's allocator with the same
// arguments; the counting around it touches only thread-local cells.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !INSTALLED.load(Ordering::Relaxed) {
            INSTALLED.store(true, Ordering::Relaxed);
        }
        // SAFETY: the caller's contract is System's.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            note(layout.size(), 0);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !INSTALLED.load(Ordering::Relaxed) {
            INSTALLED.store(true, Ordering::Relaxed);
        }
        // SAFETY: the caller's contract is System's.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            note(layout.size(), 0);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is System's.
        unsafe { System.dealloc(ptr, layout) };
        note(0, layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's contract is System's.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            note(new_size, layout.size());
        }
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_allocated_since_counts_from_there() {
        let before = Allocations {
            count: 10,
            bytes: 1000,
            held: 400,
        };
        let now = Allocations {
            count: 15,
            bytes: 1600,
            held: 300,
        };
        assert_eq!(
            now.since(&before),
            Allocations {
                count: 5,
                bytes: 600,
                held: -100,
            }
        );
    }
}
