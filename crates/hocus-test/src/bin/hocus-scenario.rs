//! Runs scenario files; `hocus scenario` calls it.

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use hocus_test::scenario::{RunOptions, Scenario};

/// Run Hocuspanda scenarios on headless compositors.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run scenario files, each on its own compositor.
    Run {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
        /// Record screenshots as new reference images.
        #[arg(long)]
        bless: bool,
        /// Where failed comparisons leave their images.
        #[arg(long, value_name = "DIR", default_value = "target/test-artifacts")]
        artifacts: PathBuf,
    },
}

fn main() -> ExitCode {
    let Command::Run {
        files,
        bless,
        artifacts,
    } = Args::parse().command;
    // The test client sits next to this program.
    let test_client = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("hocus-test-client")))
        .filter(|p| p.is_file());
    let options = RunOptions {
        artifacts,
        bless,
        test_client,
    };
    let mut failed = 0;
    for file in &files {
        match Scenario::load(file).and_then(|s| s.run(&options)) {
            Ok(()) => println!("ok      {}", file.display()),
            Err(err) => {
                failed += 1;
                println!("FAILED  {}: {err:#}", file.display());
            }
        }
    }
    println!("{} passed, {failed} failed", files.len() - failed);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
