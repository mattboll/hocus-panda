//! Programs the compositor starts end on SIGTERM (#733): the loop blocks
//! SIGTERM and SIGINT to read them itself, and a blocked mask survives
//! `exec`. The test blocks them as `hocuspanda` does before its loop
//! starts, then starts `sleep` through the launcher's way (which autostart
//! and the session restored take, and, as every program, through
//! `programs::Env::command`, as the shell, the terminal and `--spawn`):
//! it starts with no signal blocked, and SIGTERM ends it.
//!
//! `sleep` itself, never through `sh`: dash clears its mask as it starts,
//! which would hide the bug.

use crate::common;

use common::WAIT;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};

/// What makes this test's `sleep` its own among the processes.
const MARK: &str = "600.733";

/// SIGTERM and SIGINT blocked in this thread, and so in the threads it
/// starts, as the compositor's `run` does before anything else.
#[allow(unsafe_code)]
fn block_as_the_loop_does() {
    // SAFETY: this thread's own mask, from plain data.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()),
            0
        );
    }
}

/// The `sleep MARK` this process started, and what `/proc` says of it.
fn ours() -> Option<(i32, String)> {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            if cmdline != format!("sleep\0{MARK}\0").as_bytes() {
                return None;
            }
            let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
            status
                .lines()
                .any(|l| l == format!("PPid:\t{me}"))
                .then_some((pid, status))
        })
}

/// The test's `sleep`, killed (SIGKILL) if a check fails before SIGTERM
/// ends it: never left behind.
struct Reap(i32);

impl Drop for Reap {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if ours().is_some_and(|(pid, _)| pid == self.0) {
            // SAFETY: kill on the exact pid of this test's own program.
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
        }
    }
}

#[test]
fn a_program_started_has_no_signal_blocked_and_ends_on_sigterm() -> anyhow::Result<()> {
    block_as_the_loop_does();
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    let mut hp = Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.launch(hocus_link::Launch {
        id: "sleep".to_owned(),
        terminal: false,
        dir: String::new(),
        argv: vec!["sleep".to_owned(), MARK.to_owned()],
    });
    let mut found = None;
    let started = hp.dispatch_until(WAIT, |_| {
        found = ours();
        found.is_some()
    })?;
    anyhow::ensure!(started, "sleep never started");
    let (pid, status) = found.expect("found");
    let _reap = Reap(pid);
    let blocked = status.lines().find(|l| l.starts_with("SigBlk:"));
    assert_eq!(blocked, Some("SigBlk:\t0000000000000000"), "{status}");
    // The test's own program, by its exact pid.
    // SAFETY: kill on a pid this test's compositor started.
    #[allow(unsafe_code)]
    let sent = unsafe { libc::kill(pid, libc::SIGTERM) };
    assert_eq!(sent, 0, "{}", std::io::Error::last_os_error());
    // Gone: its thread waited for it.
    let gone = hp.dispatch_until(WAIT, |_| ours().is_none())?;
    anyhow::ensure!(gone, "sleep ({pid}) did not end on SIGTERM");
    Ok(())
}
