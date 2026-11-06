//! AccountsService, faked on a test's private bus, never the machine's:
//! one user, whatever its uid, with its name, login, type and picture.
//! A picture set is copied into a folder of its own, as AccountsService
//! copies it; what it was asked is recorded, and it refuses everything
//! while told to (polkit saying no). It says its changes as
//! AccountsService does (`Changed`).
#![allow(dead_code)]

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use zbus::{Connection, OwnedObjectPath, connection::Builder};

pub const NAME: &str = "org.freedesktop.Accounts";
pub const PATH: &str = "/org/freedesktop/Accounts";
pub const USER_PATH: &str = "/org/freedesktop/Accounts/User1000";

/// What it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Name(String),
    /// The path given (empty: none).
    Icon(String),
}

/// What it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub real_name: String,
    pub login: String,
    pub admin: bool,
    /// Its picture's file (empty: none).
    pub icon: String,
    pub refuse: bool,
    pub calls: Vec<Call>,
}

impl Default for User {
    fn default() -> Self {
        Self {
            real_name: "Alex Martin".to_owned(),
            login: "alex".to_owned(),
            admin: true,
            icon: String::new(),
            refuse: false,
            calls: Vec::new(),
        }
    }
}

struct Inner {
    user: Mutex<User>,
    /// Where the pictures set are copied.
    icons: PathBuf,
    connection: OnceLock<Connection>,
}

type Shared = Arc<Inner>;

fn with<T>(shared: &Shared, f: impl FnOnce(&mut User) -> T) -> T {
    f(&mut shared.user.lock().unwrap_or_else(PoisonError::into_inner))
}

async fn changed(shared: &Shared) {
    let Some(connection) = shared.connection.get() else {
        return;
    };
    let _ = connection
        .emit_signal(
            None::<&str>,
            USER_PATH,
            "org.freedesktop.Accounts.User",
            "Changed",
            &(),
        )
        .await;
}

fn asked(shared: &Shared, call: Call) -> zbus::fdo::Result<()> {
    with(shared, |u| {
        u.calls.push(call);
        if u.refuse {
            Err(zbus::fdo::Error::AccessDenied("Not authorized".to_owned()))
        } else {
            Ok(())
        }
    })
}

struct Accounts;

#[zbus::interface(name = "org.freedesktop.Accounts")]
impl Accounts {
    fn find_user_by_id(&self, _uid: i64) -> zbus::fdo::Result<OwnedObjectPath> {
        OwnedObjectPath::try_from(USER_PATH).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

struct UserObject(Shared);

#[zbus::interface(name = "org.freedesktop.Accounts.User")]
impl UserObject {
    #[zbus(property)]
    fn real_name(&self) -> String {
        with(&self.0, |u| u.real_name.clone())
    }

    #[zbus(property)]
    fn user_name(&self) -> String {
        with(&self.0, |u| u.login.clone())
    }

    #[zbus(property)]
    fn icon_file(&self) -> String {
        with(&self.0, |u| u.icon.clone())
    }

    #[zbus(property)]
    fn account_type(&self) -> i32 {
        with(&self.0, |u| i32::from(u.admin))
    }

    async fn set_real_name(&self, name: String) -> zbus::fdo::Result<()> {
        asked(&self.0, Call::Name(name.clone()))?;
        with(&self.0, |u| u.real_name = name);
        changed(&self.0).await;
        Ok(())
    }

    async fn set_icon_file(&self, path: String) -> zbus::fdo::Result<()> {
        asked(&self.0, Call::Icon(path.clone()))?;
        let icon = if path.is_empty() {
            String::new()
        } else {
            let login = with(&self.0, |u| u.login.clone());
            let copy = self.0.icons.join(login);
            std::fs::copy(&path, &copy).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
            copy.display().to_string()
        };
        with(&self.0, |u| u.icon = icon);
        changed(&self.0).await;
        Ok(())
    }
}

/// The fake, served on its own connection.
pub struct AccountsService {
    shared: Shared,
}

impl AccountsService {
    /// AccountsService on the bus at `address`, holding `user`, copying
    /// pictures into `icons`.
    pub fn start(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        user: User,
        icons: PathBuf,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&icons)?;
        let shared: Shared = Arc::new(Inner {
            user: Mutex::new(user),
            icons,
            connection: OnceLock::new(),
        });
        let address = address.to_owned();
        let served = shared.clone();
        runtime.block_on(async move {
            let connection = Builder::address(address.as_str())
                .name(NAME)
                .serve_at(PATH, Accounts)
                .serve_at(USER_PATH, UserObject(served.clone()))
                .build()
                .await?;
            let _ = served.connection.set(connection);
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(Self { shared })
    }

    pub fn now(&self) -> User {
        with(&self.shared, |u| u.clone())
    }

    pub fn calls(&self) -> Vec<Call> {
        with(&self.shared, |u| u.calls.clone())
    }

    pub fn refuse(&self, refuse: bool) {
        with(&self.shared, |u| u.refuse = refuse);
    }

    /// The picture's bytes it holds now.
    pub fn picture(&self) -> Option<Vec<u8>> {
        let icon = with(&self.shared, |u| u.icon.clone());
        (!icon.is_empty())
            .then(|| std::fs::read(icon).ok())
            .flatten()
    }
}
