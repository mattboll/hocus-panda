//! AccountsService (`org.freedesktop.Accounts`) on the system bus,
//! followed and asked on a thread of its own, never the window's (6.11):
//! the session user's name, login, type and picture.
//!
//! The user is `FindUserById` of the process's uid; its properties are
//! read again whenever it says it changed (`Changed`, or
//! `PropertiesChanged`), and its picture's file read and made round for
//! the window here. Only a change reaches the window. Without
//! AccountsService (or the bus), the window is told so once and the thread
//! tries again, slower and slower, silently.
//!
//! A picture is set as AccountsService takes it, a file it copies: a
//! panda's installed file, or one written in `$XDG_RUNTIME_DIR` (a panda
//! not installed, a picture chosen, cut square, a picture put back by
//! "Annuler"), removed once AccountsService has it.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use iced::futures::{
    StreamExt as _,
    channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
    stream,
};
use zbus::{
    Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, connection::Builder,
    message::Type,
};

use crate::account;

const DEST: &str = "org.freedesktop.Accounts";
const PATH: &str = "/org/freedesktop/Accounts";
const USER: &str = "org.freedesktop.Accounts.User";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

const RETRY: Duration = Duration::from_secs(2);
const RETRY_MOST: Duration = Duration::from_secs(60);

/// The account's picture: its file's bytes, and as the window draws it.
#[derive(Clone)]
pub struct Picture {
    pub bytes: Arc<Vec<u8>>,
    /// [`account::SHOWN`] wide, round, RGBA.
    pub rgba: Arc<Vec<u8>>,
    /// Which panda it is, if one.
    pub panda: Option<usize>,
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for Picture {}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("bytes", &self.bytes.len())
            .field("panda", &self.panda)
            .finish()
    }
}

/// What AccountsService says of the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Account {
    pub real_name: String,
    pub login: String,
    pub admin: bool,
    pub picture: Option<Picture>,
}

/// What the window asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    Name(String),
    Panda(usize),
    /// A picture chosen: cut square, made 512 wide at most, then set.
    Chosen(PathBuf),
    /// A picture's bytes set back ("Annuler").
    Bytes(Arc<Vec<u8>>),
    /// No picture: the initial.
    Remove,
}

/// What the thread says to the window.
#[derive(Debug, Clone)]
pub enum Heard {
    /// The pandas as the window draws them, once.
    Pandas(Vec<Arc<Vec<u8>>>),
    /// The account as it is now; `None`: AccountsService is not there.
    State(Option<Account>),
    /// A call answered: `Err` says why it was refused.
    Done(Ask, Result<(), String>),
}

pub type Asks = UnboundedSender<Ask>;

/// AccountsService followed on a thread of its own: what it says goes to
/// `say` (`false` once the window is gone, and the thread ends).
pub fn start(say: impl FnMut(Heard) -> bool + Send + 'static) -> Asks {
    let (asks, incoming) = unbounded();
    let started = std::thread::Builder::new()
        .name("accounts".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::warn!(%err, "no runtime for AccountsService");
                    return;
                }
            };
            runtime.block_on(follow(incoming, say));
        });
    if let Err(err) = started {
        tracing::warn!(%err, "AccountsService is not followed");
    }
    asks
}

enum Event {
    Ask(Ask),
    Changed,
    Answered(Ask, Result<(), String>),
    Gone,
    Lost,
}

async fn follow(mut incoming: UnboundedReceiver<Ask>, mut say: impl FnMut(Heard) -> bool) {
    let pandas: Vec<Arc<Vec<u8>>> = account::PANDAS
        .iter()
        .map(|p| Arc::new(account::round(p).unwrap_or_default()))
        .collect();
    if !say(Heard::Pandas(pandas)) {
        return;
    }
    let mut said: Option<Option<Account>> = None;
    let mut tell = |state: Option<Account>, say: &mut dyn FnMut(Heard) -> bool| -> bool {
        if said.as_ref() == Some(&state) {
            return true;
        }
        said = Some(state.clone());
        say(Heard::State(state))
    };
    let mut wait = RETRY;
    loop {
        let (connection, user, changes) = match connect().await {
            Ok(connected) => connected,
            Err(err) => {
                tracing::info!(%err, "no AccountsService");
                if !tell(None, &mut say) {
                    return;
                }
                let until = tokio::time::Instant::now() + wait;
                loop {
                    match tokio::time::timeout_at(until, incoming.next()).await {
                        Err(_) => break,
                        Ok(None) => return,
                        Ok(Some(ask)) => {
                            if !say(Heard::Done(ask, Err("AccountsService".to_owned()))) {
                                return;
                            }
                        }
                    }
                }
                wait = (wait * 2).min(RETRY_MOST);
                continue;
            }
        };
        wait = RETRY;
        let (answers, answered) = unbounded::<(Ask, Result<(), String>)>();
        let mut last: Option<Account> = None;
        let state = read(&connection, &user, last.as_ref()).await;
        last.clone_from(&state);
        if !tell(state, &mut say) {
            return;
        }
        let mut events = stream::select_all([
            incoming
                .by_ref()
                .map(Event::Ask)
                .chain(stream::once(async { Event::Gone }))
                .boxed(),
            changes
                .map(|_| Event::Changed)
                .chain(stream::once(async { Event::Lost }))
                .boxed(),
            answered
                .map(|(ask, result)| Event::Answered(ask, result))
                .boxed(),
        ]);
        loop {
            match events.next().await {
                None | Some(Event::Gone) => return,
                Some(Event::Lost) => break,
                Some(Event::Ask(ask)) => {
                    let connection = connection.clone();
                    let user = user.clone();
                    let answers = answers.clone();
                    tokio::spawn(async move {
                        let result = call(&connection, &user, &ask).await;
                        let _ = answers.unbounded_send((ask, result));
                    });
                }
                Some(Event::Answered(ask, result)) => {
                    if let Err(why) = &result {
                        tracing::info!(why, "AccountsService refused");
                    }
                    let state = read(&connection, &user, last.as_ref()).await;
                    last.clone_from(&state);
                    if !tell(state, &mut say) || !say(Heard::Done(ask, result)) {
                        return;
                    }
                }
                Some(Event::Changed) => {
                    let state = read(&connection, &user, last.as_ref()).await;
                    last.clone_from(&state);
                    if !tell(state, &mut say) {
                        return;
                    }
                }
            }
        }
        tracing::info!("AccountsService's bus went away");
    }
}

/// The system bus, the user's object, and its signals listened to.
async fn connect() -> zbus::Result<(Connection, OwnedObjectPath, MessageStream)> {
    let connection = Builder::system().build().await?;
    let uid = i64::from(rustix::process::getuid().as_raw());
    let reply = connection
        .call_method(Some(DEST), PATH, Some(DEST), "FindUserById", &(uid,))
        .await?;
    let user: OwnedObjectPath = reply.body().deserialize()?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(user.as_str().to_owned())
        .build()?;
    let changes = MessageStream::for_match_rule(rule, &connection, None).await?;
    Ok((connection, user, changes))
}

/// The user as AccountsService has it now; its picture made again only
/// when its file changed.
async fn read(
    connection: &Connection,
    user: &OwnedObjectPath,
    last: Option<&Account>,
) -> Option<Account> {
    let reply = connection
        .call_method(
            Some(DEST),
            user.as_str(),
            Some(PROPERTIES),
            "GetAll",
            &(USER,),
        )
        .await;
    let all: HashMap<String, OwnedVariant> = match reply {
        Ok(reply) => reply.body().deserialize().ok()?,
        Err(err) => {
            tracing::info!(%err, "AccountsService does not answer");
            return None;
        }
    };
    let text = |name: &str| {
        all.get(name)
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default()
    };
    let kind = all
        .get("AccountType")
        .and_then(|v| i32::try_from(v.try_clone().ok()?).ok())
        .unwrap_or(0);
    let icon = text("IconFile");
    let picture = if icon.is_empty() {
        None
    } else {
        let read = tokio::task::spawn_blocking(move || picture_of(&PathBuf::from(icon)))
            .await
            .ok()
            .flatten();
        match (read, last.and_then(|l| l.picture.clone())) {
            // The same file: what was made of it already.
            (Some(bytes), Some(before)) if *before.bytes == bytes => Some(before),
            (Some(bytes), _) => match account::round(&bytes) {
                Ok(rgba) => Some(Picture {
                    panda: account::which_panda(&bytes),
                    bytes: Arc::new(bytes),
                    rgba: Arc::new(rgba),
                }),
                Err(err) => {
                    tracing::info!(%err, "the account's picture does not read: the initial");
                    None
                }
            },
            (None, _) => None,
        }
    };
    Some(Account {
        real_name: text("RealName"),
        login: text("UserName"),
        admin: kind == 1,
        picture,
    })
}

/// A picture's file's bytes, if it is there and not too large.
fn picture_of(path: &std::path::Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > account::LARGEST {
        return None;
    }
    std::fs::read(path).ok()
}

/// `ask` made of AccountsService; why it was refused.
async fn call(connection: &Connection, user: &OwnedObjectPath, ask: &Ask) -> Result<(), String> {
    let set_icon = |path: String| async move {
        connection
            .call_method(
                Some(DEST),
                user.as_str(),
                Some(USER),
                "SetIconFile",
                &(path.as_str(),),
            )
            .await
            .map(|_| ())
    };
    let called = match ask {
        Ask::Name(name) => connection
            .call_method(
                Some(DEST),
                user.as_str(),
                Some(USER),
                "SetRealName",
                &(name.as_str(),),
            )
            .await
            .map(|_| ()),
        Ask::Remove => set_icon(String::new()).await,
        Ask::Panda(index) => match account::installed(*index) {
            Some(path) => set_icon(path.display().to_string()).await,
            None => {
                let bytes = account::PANDAS.get(*index).copied().unwrap_or_default();
                return set_written(bytes.to_vec(), set_icon).await;
            }
        },
        Ask::Bytes(bytes) => return set_written(bytes.to_vec(), set_icon).await,
        Ask::Chosen(path) => {
            let path = path.clone();
            let made = tokio::task::spawn_blocking(move || {
                let picture = hocus_link::wallpaper::decode(&path)?;
                account::png(&account::square(&picture))
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
            match made {
                Ok(bytes) => return set_written(bytes, set_icon).await,
                Err(why) => return Err(why),
            }
        }
    };
    called.map_err(said)
}

/// `bytes` written in the runtime directory, set, then removed.
async fn set_written<F, Fut>(bytes: Vec<u8>, set_icon: F) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = zbus::Result<()>>,
{
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from);
    let file = dir.join(format!("hocus-settings-avatar-{}.png", std::process::id()));
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    let result = set_icon(file.display().to_string()).await.map_err(said);
    if let Err(err) = std::fs::remove_file(&file) {
        tracing::debug!(%err, "the avatar's file stays in the runtime directory");
    }
    result
}

fn said(err: zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(name, message, _) => {
            message.unwrap_or_else(|| name.as_str().to_owned())
        }
        other => other.to_string(),
    }
}
