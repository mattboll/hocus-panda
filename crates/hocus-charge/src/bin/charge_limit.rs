//! The battery's charge limit's helper (issue #896, packaging/README.md):
//! `charge-limit 80` or `charge-limit 100`, run as root through pkexec by
//! the compositor, which polkit lets the local active session do without
//! a password (`org.hocuspanda.charge-limit`).
//!
//! It writes that value, and only it, into the charge thresholds of the
//! system's batteries, and no other file ([`hocus_charge::apply`]). It
//! takes no path and reads no environment variable: the kernel's supplies
//! are always `/sys/class/power_supply`.

use std::{ffi::OsString, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(target) = hocus_charge::read_args(&args) else {
        eprintln!("usage: charge-limit 80|100");
        return ExitCode::from(2);
    };
    match hocus_charge::apply(Path::new(hocus_charge::SUPPLIES), target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("charge-limit {}: {err}", target.arg());
            ExitCode::FAILURE
        }
    }
}
