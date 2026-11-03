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

use std::{
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::{Duration, Instant},
};

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
}

/// Pixels lent.
#[derive(Debug, Clone)]
pub struct Lease(Arc<Inner>);

impl Lease {
    /// `pixels`, lent: the lender keeps them alive and does not write them
    /// until [`Lease::is_back`], or until it [`Lease::revoke`]s them.
    pub fn new(pixels: &[u8]) -> Self {
        Self::made(pixels, false)
    }

    /// The same, the lender telling the loop itself once they are free.
    pub fn quiet(pixels: &[u8]) -> Self {
        Self::made(pixels, true)
    }

    fn made(pixels: &[u8], quiet: bool) -> Self {
        Self(Arc::new(Inner {
            memory: Mutex::new(Some((pixels.as_ptr() as usize, pixels.len()))),
            back: Mutex::new(false),
            given: Condvar::new(),
            quiet,
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
        let lease = Lease::new(&pixels);
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
        let taken = Lease::quiet(&pixels);
        assert!(taken.is_quiet());
        taken.revoke();
        assert_eq!(taken.with(|p| p.len()), None);
    }
}
