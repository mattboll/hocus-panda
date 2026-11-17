//! The shell's heap (#855): two of glibc's malloc arenas, not one a thread.
//!
//! glibc gives each thread that allocates an arena of its own, up to eight
//! a core, and an arena keeps what is freed in it for its own thread's next
//! allocations. The shell runs a hundred threads (iced's runtime, a worker
//! each for D-Bus, PipeWire, the day's accounts…; the GPU driver's): a
//! hundred arenas, each holding its own freed pieces, and its resident
//! memory rose a few megabytes an hour of use, never going back, though
//! what it holds alive did not (measured headless, wgpu on a software
//! Vulkan: the live heap flat, the resident memory not). With two, what
//! is freed is taken again: it stays flat. The threads seldom allocate at
//! once; the drawing is on one.
#![allow(unsafe_code)]

/// The arenas: the main one, and one for every other thread.
#[cfg(target_env = "gnu")]
const ARENAS: libc::c_int = 2;

/// Called in `main` before any thread starts: an arena a thread was given
/// stays its own.
pub fn bound() {
    #[cfg(target_env = "gnu")]
    {
        // SAFETY: `mallopt` only sets a parameter of glibc's allocator, and
        // is called before any other thread of the shell allocates.
        let set = unsafe { libc::mallopt(libc::M_ARENA_MAX, ARENAS) };
        if set != 1 {
            tracing::warn!("malloc's arenas could not be bounded");
        }
    }
}
