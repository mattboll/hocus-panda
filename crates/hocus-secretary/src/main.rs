//! `hocus-secretary`: the secretary's process. The compositor starts it in
//! `[secretary] mode = "assistant"`, with her socket's path
//! (`HOCUS_SECRETARY_SOCKET`) and the bus whose keyring holds her memory's
//! key (`HOCUS_SECRETARY_BUS`), and starts it again when it ends.
//!
//! `hocus-secretary key PROVIDER` keeps a provider's API key, read on
//! standard input, in the keyring of that bus (§11.3).

use std::{
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use clap::{Parser, Subcommand};
use hocus_secretary::{
    MEMORY,
    brain::{Brain, Options},
    clock::Clock,
    context::{Language, Scope},
    google::{self, Google},
    key,
    model::{Fake, Model, Recorder},
    notifications::Notifications,
    providers::{Keyring, Router},
    server, settings,
    sources::{Fixture, Source},
    store::Store,
    triage::Triager,
};

/// Her model for answers, and her local triage.
type Models = (Option<Arc<dyn Model>>, Option<Arc<dyn Triager>>);

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
    /// Where the fake model records every call it gets, and where the
    /// outside effects of accepted cards are written, one JSON object per
    /// line: tests.
    #[arg(long, value_name = "FILE")]
    recorder: Option<PathBuf>,
    /// Ends once standard input closes: the compositor holds that pipe,
    /// which closes when it stops her or ends itself.
    #[arg(long)]
    until_stdin_closes: bool,
    /// A fixture source, from this TOML file (again for more): tests.
    /// With one, her Google accounts are not read.
    #[arg(long, value_name = "FILE")]
    fixture: Vec<PathBuf>,
    /// The configuration file whose `[secretary]` she reads. Default:
    /// $HOCUS_CONFIG, else $XDG_CONFIG_HOME/hocuspanda/config.toml.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Her Google accounts are not read, even with a bus.
    #[arg(long)]
    no_google: bool,
    /// People, projects, facts and rules written into an empty memory at
    /// her start, from this TOML file: tests, screenshots.
    #[arg(long, value_name = "FILE")]
    memory_fixture: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Keeps PROVIDER's API key, read on standard input, in the keyring
    /// of the bus (--bus or $HOCUS_SECRETARY_BUS).
    Key { provider: String },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("hocus_secretary=info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    let ran = match &args.command {
        Some(Command::Key { provider }) => keep_key(&args, provider),
        None => run(args),
    };
    match ran {
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
    let mut store =
        Store::open(&memory, &key, clock).map_err(|e| format!("{}: {e}", memory.display()))?;
    if let Some(file) = &args.memory_fixture {
        let seed = hocus_secretary::seed::Seed::from_file(file)?;
        seed.write(&mut store)
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    // The recorder writes down the fake model's calls and the outside
    // effects of accepted cards alike.
    let recorder = args
        .recorder
        .clone()
        .map(|f| Arc::new(Recorder::to_file(f)));
    let settings = Arc::new(settings::File::at(
        args.config.clone().or_else(settings::File::default_path),
    ));
    let (model, triage): Models = match &args.fake_model {
        Some(script) => (
            Some(Arc::new(Fake::from_file(script, recorder.clone())?)),
            None,
        ),
        None => {
            let router = Arc::new(Router::new(
                settings.clone(),
                Arc::new(Keyring::new(bus.clone())),
            ));
            (Some(router.clone()), Some(router))
        }
    };
    let mut sources: Vec<Box<dyn Source>> = Vec::new();
    for file in &args.fixture {
        sources.push(Box::new(Fixture::from_file(file)?));
    }
    if let Some(bus) = bus
        .as_deref()
        .filter(|_| args.fixture.is_empty() && !args.no_google)
    {
        let address = if bus == "session" {
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_default()
        } else {
            bus.to_owned()
        };
        if !address.is_empty() {
            sources.push(Box::new(Google::new(
                address.clone(),
                data.join(google::CONSENTS),
                hocus_sources::cycle::Env::google(&address),
            )));
        }
    }
    // The applications' notifications the shell forwards, as consented.
    let (notifications, arrivals) = Notifications::new(data.join(google::CONSENTS));
    sources.push(Box::new(notifications));
    let brain = Brain::new(
        store,
        Options {
            model,
            sources,
            key: place.word().to_owned(),
            triage,
            settings,
            scope: Scope {
                language: Language::of_env(),
                ..Scope::default()
            },
            effects: recorder,
            notifications: Some(arrivals),
            exports: Some(data.join("exports")),
            ..Options::default()
        },
    );
    server::serve(&socket, brain.sender()).map_err(|e| format!("{}: {e}", socket.display()))?;
    if args.until_stdin_closes {
        let stop = brain.sender();
        std::thread::Builder::new()
            .name("stdin".to_owned())
            .spawn(move || {
                // Nothing is ever written: a read returns at its end.
                let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
                tracing::info!("standard input closed: the secretary ends");
                let _ = stop.send(hocus_secretary::brain::Event::Stop);
            })
            .map_err(|e| format!("no thread for standard input: {e}"))?;
    }
    tracing::info!(socket = %socket.display(), memory = %memory.display(), key = place.word(), "the secretary listens");
    brain.run();
    // Her socket goes with her; a client connecting now finds none.
    let _ = std::fs::remove_file(&socket);
    Ok(())
}

/// `hocus-secretary key PROVIDER`: the key on standard input, into the
/// keyring; never echoed, never in a file.
fn keep_key(args: &Args, provider: &str) -> Result<(), String> {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let bus = args
        .bus
        .clone()
        .or_else(|| env(hocus_secretary_link::BUS_ENV))
        .ok_or("no keyring: --bus ADDRESS (or session), or HOCUS_SECRETARY_BUS")?;
    let mut secret = String::new();
    std::io::stdin()
        .read_line(&mut secret)
        .map_err(|e| format!("standard input: {e}"))?;
    let secret = secret.trim();
    if secret.is_empty() {
        return Err("no key on standard input".to_owned());
    }
    if key::keep_provider_key(&bus, provider, secret) {
        tracing::info!(provider, "the key is in the keyring");
        Ok(())
    } else {
        Err("the keyring did not keep it".to_owned())
    }
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
