//! `hocus day planka-token [ACCOUNT]`: a Planka board's access token, read
//! on standard input, checked against its server (`GET /api/users/me`)
//! and kept through `hocus_day::secrets`, where the bar's worker reads it:
//! the session's keyring, else a file of the user's alone. Until Day.1's
//! form asks for it.

use std::{
    io::{BufRead, IsTerminal, Read},
    path::{Path, PathBuf},
};

use hocus_day::{
    SourceError,
    config::{self, TaskKind},
    model::AccountId,
    planka,
    secrets::{self, Kept, Secrets, Token},
};

/// The most a token may weigh: the server's cookies at most.
const MOST: u64 = 64 << 10;

/// What `planka-token` is given.
#[derive(Debug)]
pub struct PlankaToken {
    pub account: Option<String>,
    pub config: Option<PathBuf>,
    pub bus: Option<String>,
    pub no_check: bool,
}

/// Keeps the token: what to say, or why not.
pub fn planka_token(args: PlankaToken) -> Result<String, String> {
    let file = args.config.or_else(config_file);
    let config = match &file {
        Some(file) => config::read(file).map_err(|err| err.to_string())?,
        None => config::Config::default(),
    };
    let base = file.as_deref().and_then(Path::parent);
    let set = config
        .tasks_account(base)
        .filter(|(account, _)| account.kind == hocus_day::Kind::Planka);
    let (account, server) = match (&args.account, &set) {
        (None, Some((account, entry))) => (account.id.clone(), server_of(entry)?),
        (Some(given), Some((account, entry))) if account.id.as_str() == given => {
            (account.id.clone(), server_of(entry)?)
        }
        (Some(given), _) => {
            eprintln!(
                "hocus: {given} is not the configuration's Planka board{}: the token is kept unchecked",
                set.as_ref()
                    .map(|(a, _)| format!(" ({})", a.id))
                    .unwrap_or_default()
            );
            (AccountId::new(given.clone()), None)
        }
        (None, None) => {
            let place = file.as_ref().map_or_else(
                || "the configuration".to_owned(),
                |f| f.display().to_string(),
            );
            return Err(format!(
                "no Planka board in {place}: set [[day.tasks]] kind = \"planka\", or name the account"
            ));
        }
    };
    let token = read_token()?;
    if let Some(server) = server.filter(|_| !args.no_check) {
        match planka::check(&server, &token) {
            Ok(user) => eprintln!("hocus: {server} takes the token: signed in as {user}"),
            Err(SourceError::Offline(why)) => {
                eprintln!("hocus: {server} does not answer ({why}): the token is kept unchecked");
            }
            Err(error) => return Err(format!("{server}: {error}; nothing kept")),
        }
    }
    let bus = match args.bus.as_deref() {
        Some("none") => None,
        Some(bus) => Some(bus.to_owned()),
        None => std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
            .filter(|a| !a.is_empty())
            .map(|_| "session".to_owned()),
    };
    let dir = secrets::accounts_dir();
    let kept = Secrets::new(bus, dir.clone())
        .keep(&account, &token)
        .map_err(|err| format!("the token is not kept: {err}"))?;
    let place = match kept {
        Kept::Keyring => "in the session's keyring".to_owned(),
        Kept::File => dir.map_or_else(
            || "in a file".to_owned(),
            |d| format!("in {}", d.join(account.as_str()).display()),
        ),
    };
    Ok(format!(
        "{account}: kept {place}; the bar reads it at its next read (within 2 minutes, or on resume)"
    ))
}

/// The configuration's server, `None` when it is not one (its worker says
/// why).
fn server_of(entry: &config::TaskEntry) -> Result<Option<planka::Server>, String> {
    debug_assert_eq!(entry.kind, TaskKind::Planka);
    match planka::address(&entry.address, &entry.board) {
        Ok((server, _)) => Ok(Some(server)),
        Err(error) => Err(format!("the configuration's board: {error}")),
    }
}

/// `$XDG_CONFIG_HOME/hocuspanda/config.toml`, or under `~/.config`.
fn config_file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let config = var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    config
        .is_absolute()
        .then(|| config.join("hocuspanda/config.toml"))
}

/// The token, on standard input: one line from a terminal, else all of it.
fn read_token() -> Result<Token, String> {
    let stdin = std::io::stdin();
    let mut text = String::new();
    if stdin.is_terminal() {
        eprintln!(
            "Planka's access token (in Planka, once signed in: the browser's tools (F12) \
             › Storage › Cookies › accessToken; with an httpOnlyToken there, paste both as \
             accessToken=…; httpOnlyToken=…):"
        );
        stdin
            .lock()
            .read_line(&mut text)
            .map_err(|err| format!("standard input: {err}"))?;
    } else {
        stdin
            .lock()
            .take(MOST)
            .read_to_string(&mut text)
            .map_err(|err| format!("standard input: {err}"))?;
    }
    let token = text.trim();
    if token.is_empty() {
        return Err("no token on standard input".to_owned());
    }
    Ok(Token::new(token))
}
