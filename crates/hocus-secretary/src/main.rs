//! `hocus-secretary`: the secretary's process. The compositor starts it in
//! `[secretary] mode = "assistant"`, with her socket's path
//! (`HOCUS_SECRETARY_SOCKET`) and the bus whose keyring holds her memory's
//! key (`HOCUS_SECRETARY_BUS`), and starts it again when it ends.

use std::{
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use clap::Parser;
use hocus_secretary::{
    MEMORY,
    brain::{Brain, Options},
    clock::Clock,
    key,
    model::{Fake, Model, Recorder},
    server,
    sources::{Fixture, Source},
    store::Store,
};

/// The Hocuspanda secretary.
#[derive(Debug, Parser)]
#[command(name = "hocus-secretary", version)]
struct Args {
    /// Her socket. Default: $HOCUS_SECRETARY_SOCKET.
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,
    /// Her memory's folder. Default: $XDG_DATA_HOME/hocuspanda/secretary.
    #[arg(long, value_name = "DIR")]
    data: Option<PathBuf>,
    /// The session bus whose keyring holds her memory's key: an address,
    /// or `session`. Default: $HOCUS_SECRETARY_BUS; none: a key file.
    #[arg(long, value_name = "ADDRESS")]
    bus: Option<String>,
    /// A clock of her own, at this time (RFC 3339), moved on only by her
    /// socket's `clock` line: tests.
    #[arg(long, value_name = "TIME")]
    virtual_clock: Option<String>,
    /// The scripted fake model, from this TOML script: tests.
    #[arg(long, value_name = "SCRIPT")]
    fake_model: Option<PathBuf>,
    /// Where the fake model records every call it gets, one JSON object
    /// per line: tests.
    #[arg(long, value_name = "FILE")]
    recorder: Option<PathBuf>,
    /// A fixture source, from this TOML file (again for more): tests.
    #[arg(long, value_name = "FILE")]
    fixture: Vec<PathBuf>,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("hocus_secretary=info")),
        )
        .with_writer(std::io::stderr)
        .init();
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let socket = args
        .socket
        .or_else(|| env(hocus_secretary_link::SOCKET_ENV).map(PathBuf::from))
        .ok_or("no socket: --socket or HOCUS_SECRETARY_SOCKET")?;
    let data = args
        .data
        .or_else(hocus_secretary::data_dir)
        .ok_or("no data folder: --data, XDG_DATA_HOME or HOME")?;
    let bus = args
        .bus
        .or_else(|| env(hocus_secretary_link::BUS_ENV).and_then(|b| b.into_string().ok()));
    let clock = match &args.virtual_clock {
        Some(start) => Clock::virtual_at(
            start
                .parse()
                .map_err(|e| format!("--virtual-clock {start}: {e}"))?,
        ),
        None => Clock::Wall,
    };
    private_dir(&data)?;
    let memory = data.join(MEMORY);
    let (key, place) = key::key(&data, bus.as_deref(), memory.exists())?;
    let store =
        Store::open(&memory, &key, clock).map_err(|e| format!("{}: {e}", memory.display()))?;
    let model: Option<Arc<dyn Model>> = match &args.fake_model {
        Some(script) => {
            let recorder = args.recorder.clone().map(Recorder::to_file);
            Some(Arc::new(Fake::from_file(script, recorder)?))
        }
        None => None,
    };
    let mut sources: Vec<Box<dyn Source>> = Vec::new();
    for file in &args.fixture {
        sources.push(Box::new(Fixture::from_file(file)?));
    }
    let brain = Brain::new(
        store,
        Options {
            model,
            sources,
            key: place.word().to_owned(),
        },
    );
    server::serve(&socket, brain.sender()).map_err(|e| format!("{}: {e}", socket.display()))?;
    tracing::info!(socket = %socket.display(), memory = %memory.display(), key = place.word(), "the secretary listens");
    brain.run();
    Ok(())
}

/// Her folder, the user's alone.
fn private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .and_then(|()| std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)))
        .map_err(|e| format!("{}: {e}", dir.display()))
}
