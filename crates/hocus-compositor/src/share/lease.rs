//! A frame's pixels lent to the streams' thread, without a copy: whoever
//! lends them (the loop, for the CPU renderer's own buffer; the read-back
//! thread, for a GPU buffer it mapped) keeps the memory they are in alive
//! and untouched until the streams' thread gives them back, once it copied
//! them into a PipeWire buffer. So the loop never copies a frame (16 MB
//! at 2560 × 1600).
//!
//! Should the lender have to free the memory first (the compositor
//! ending, the streams' thread gone), it takes the lease back: past that,
//! nothing reads the memory.
//!
//! A lease is the only way from a shared frame's pixels to its stream,
//! and [`Lease::with`] the only way to read them: what it gives on the
//! loop's thread is counted ([`LoopCopies`]), so that a copy brought back
//! onto the loop shows as bytes in a count that must stay 0.

use std::{
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

/// The bytes of lent pixels read on the loop's thread: each read of them
/// is a copy (the streams' thread's, into PipeWire's buffer), which must
/// never happen on the loop.
#[derive(Debug)]
pub struct LoopCopies {
    thread: ThreadId,
    bytes: AtomicU64,
}

impl LoopCopies {
    /// For the thread this is called on: the loop's.
    pub fn here() -> Arc<Self> {
        Arc::new(Self {
            thread: thread::current().id(),
            bytes: AtomicU64::new(0),
        })
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    fn read(&self, len: usize) {
        if thread::current().id() == self.thread {
            self.bytes
                .fetch_add(u64::try_from(len).unwrap_or(u64::MAX), Ordering::Relaxed);
        }
    }
}

#[derive(Debug)]
struct Inner {
    /// Where the pixels are and how many bytes, while lent.
    memory: Mutex<Option<(usize, usize)>>,
    /// The streams' thread is done with them.
    back: Mutex<bool>,
    given: Condvar,
    /// The lender says so itself once it freed them (the read-back
    /// thread): the streams' thread does not.
    quiet: bool,
    copies: Arc<LoopCopies>,
}

/// Pixels lent.
#[derive(Debug, Clone)]
pub struct Lease(Arc<Inner>);

impl Lease {
    /// `pixels`, lent: the lender keeps them alive and does not write them
    /// until [`Lease::is_back`], or until it [`Lease::revoke`]s them.
    /// Reads of them on the loop's thread are counted in `copies`.
    pub fn new(pixels: &[u8], copies: Arc<LoopCopies>) -> Self {
        Self::made(pixels, false, copies)
    }

    /// The same, the lender telling the loop itself once they are free.
    pub fn quiet(pixels: &[u8], copies: Arc<LoopCopies>) -> Self {
        Self::made(pixels, true, copies)
    }

    fn made(pixels: &[u8], quiet: bool, copies: Arc<LoopCopies>) -> Self {
        Self(Arc::new(Inner {
            memory: Mutex::new(Some((pixels.as_ptr() as usize, pixels.len()))),
            back: Mutex::new(false),
            given: Condvar::new(),
            quiet,
            copies,
        }))
    }

    pub fn is_quiet(&self) -> bool {
        self.0.quiet
    }

    /// `with` on the pixels, if they are still lent.
    pub fn with<T>(&self, with: impl FnOnce(&[u8]) -> T) -> Option<T> {
        let memory = self.0.memory.lock().unwrap_or_else(PoisonError::into_inner);
        let (at, len) = (*memory)?;
        // SAFETY: the lease was made from a slice of `len` bytes at `at`
        // whose memory the lender keeps alive and does not write until
        // the lease is back or revoked; revoking takes this same lock, so
        // the memory cannot go while it is read here.
        #[allow(unsafe_code)]
        let pixels = unsafe { std::slice::from_raw_parts(at as *const u8, len) };
        self.0.copies.read(len);
        Some(with(pixels))
    }

    /// The streams' thread is done with them: the lender may free or write
    /// them.
    pub fn give_back(&self) {
        *self.0.memory.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *self.0.back.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.0.given.notify_all();
    }

    pub fn is_back(&self) -> bool {
        *self.0.back.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits until they are back, `timeout` at most (the read-back thread,
    /// never the loop): whether they are.
    pub fn wait_back(&self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        let mut back = self.0.back.lock().unwrap_or_else(PoisonError::into_inner);
        while !*back {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            back = self
                .0
                .given
                .wait_timeout(back, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// Taken back before they were given back (the memory is about to
    /// go): waits for a copy under way, at most one.
    pub fn revoke(&self) {
        *self.0.memory.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_lent_are_read_until_given_back_or_revoked() {
        let pixels = vec![7u8; 16];
        let lease = Lease::new(&pixels, LoopCopies::here());
        assert_eq!(
            lease.with(|p| p.iter().map(|b| u32::from(*b)).sum::<u32>()),
            Some(112)
        );
        assert!(!lease.is_back());
        assert!(!lease.wait_back(Duration::from_millis(5)));
        let other = lease.clone();
        std::thread::spawn(move || other.give_back());
        assert!(lease.wait_back(Duration::from_secs(5)));
        assert!(lease.is_back());
        assert_eq!(lease.with(|p| p.len()), None);
        let taken = Lease::quiet(&pixels, LoopCopies::here());
        assert!(taken.is_quiet());
        taken.revoke();
        assert_eq!(taken.with(|p| p.len()), None);
    }

    #[test]
    fn reads_on_the_loops_thread_are_counted_and_others_not() {
        let pixels = vec![7u8; 16];
        let copies = LoopCopies::here();
        let lease = Lease::new(&pixels, copies.clone());
        let elsewhere = lease.clone();
        std::thread::spawn(move || elsewhere.with(<[u8]>::to_vec))
            .join()
            .ok();
        assert_eq!(copies.bytes(), 0);
        let _ = lease.with(<[u8]>::to_vec);
        assert_eq!(copies.bytes(), 16);
    }
}
