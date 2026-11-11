//! The crate's integration tests, one program (#765): each file of this
//! directory is a module here, and what they share is built once. A test
//! is named `MODULE::TEST`; `cargo nextest run -p hocus-cli MODULE::` runs
//! a file's.
//!
//! nextest runs each test in a process of its own, and the tests count on
//! it (environment variables, a process's working directory, its
//! children): `cargo test` runs them as threads of one process, and is not
//! supported.

// A test ends on what it did not expect: its helpers too, which clippy
// does not take for tests (clippy.toml covers `#[test]` functions only).
#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod day;
mod perf;

/// A test file that is not a module here is not built, and its tests would
/// be missed in silence (the crate's `autotests = false`).
#[test]
fn every_test_file_is_a_module() {
    let dir =
        std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
    let modules = include_str!("main.rs");
    let files = std::fs::read_dir(std::path::Path::new(&dir).join("tests")).expect("tests/");
    for path in files.map(|entry| entry.expect("an entry of tests/").path()) {
        let name = path.file_stem().and_then(|n| n.to_str()).expect("a name");
        if path.extension().is_some_and(|e| e == "rs") && name != "main" {
            assert!(
                modules.contains(&format!("\nmod {name};\n")),
                "tests/{name}.rs is not a module of tests/main.rs: its tests do not run"
            );
        }
    }
}
