//! A panic is in the program's log before it ends (#778): each program
//! of the desktop, started with `HOCUS_TEST_PANIC`, panics once its log
//! is set up, and says so there with the place and the thread, then as
//! Rust does by default. `hocus` has no log: stderr alone.
//!
//! They panic before they read anything of a session: no display, no
//! bus, a home of their own.

use std::{path::Path, process::Command};

use crate::common;

/// What `program` wrote on stderr, started to panic; it must have failed.
fn panicked(program: &Path) -> String {
    let home = tempfile::tempdir().expect("a home");
    let out = Command::new(program)
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_RUNTIME_DIR", home.path())
        .env("NO_COLOR", "1")
        // A log asked of the program alone still says its panic.
        .env("RUST_LOG", "hocus_settings=debug")
        .env(hocus_panic::TEST, "1")
        .output()
        .expect("the program starts");
    assert!(!out.status.success(), "{}: {:?}", program.display(), out);
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The log's line: an error, the message, the hook's caller's place, the
/// thread; then the default hook's.
fn assert_logged(said: &str) {
    let line = said
        .lines()
        .find(|l| l.contains("panic: HOCUS_TEST_PANIC is set"))
        .unwrap_or_else(|| panic!("no line of the log says the panic:\n{said}"));
    assert!(line.contains("ERROR"), "{line}");
    assert!(line.contains("crates/hocus-panic/src/lib.rs:"), "{line}");
    assert!(line.contains("main"), "{line}");
    assert!(said.contains("panicked at"), "{said}");
}

#[test]
fn the_compositor_logs_its_panic() {
    assert_logged(&panicked(Path::new(env!("CARGO_BIN_EXE_hocuspanda"))));
}

#[test]
fn the_shell_logs_its_panic() {
    assert_logged(&panicked(&common::shell_program()));
}

#[test]
fn the_settings_log_their_panic() {
    assert_logged(&panicked(&common::settings_program()));
}

#[test]
fn hocus_says_its_panic() {
    let said = panicked(&common::hocus_program());
    assert!(said.contains("panicked at"), "{said}");
    assert!(said.contains("HOCUS_TEST_PANIC is set"), "{said}");
}
