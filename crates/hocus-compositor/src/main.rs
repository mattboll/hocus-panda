//! The Hocuspanda compositor. Slice 1 (docs/design/windows.md) builds it
//! up from here: this is the workspace's first crate, so that formatting,
//! lints, tests and licences are checked from the start.

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--version") => {
            println!("{}", version());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{}: not runnable yet (slice 1 is being built)", version());
            ExitCode::FAILURE
        }
    }
}

fn version() -> String {
    format!("hocuspanda {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_names_the_program() {
        assert!(version().starts_with("hocuspanda 0.2"));
    }
}
